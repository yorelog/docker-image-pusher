use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde_json::Value;
use tokio::{
    sync::mpsc,
    task,
    time::{self, Duration},
};

use crate::{
    CHUNKED_LAYER_SIZE_BYTES, ESTIMATED_SPEED_MBPS, LARGE_LAYER_PROGRESS_INTERVAL_SECS,
    LARGE_LAYER_THRESHOLD_BYTES, LARGE_LAYER_THRESHOLD_MB, MAX_CHUNKED_LAYER_SIZE_BYTES,
    NORMAL_LAYER_PROGRESS_INTERVAL_SECS, PusherError, RATE_LIMIT_DELAY_MS, state,
    tar_import::{
        ExtractedLayer, TarExtraction, TarRepoInfo, build_target_from_tar,
        extract_tar_archive_with_sender, infer_target_from_history, tar_repo_info_from_path,
    },
};

const DEFAULT_UPLOAD_CONCURRENCY: usize = 3;
const MEDIUM_LAYER_THRESHOLD_MB: f64 = 250.0;
use oci_core::{
    auth::RegistryAuth,
    client::Client,
    manifest::{OciDescriptor, OciImageManifest},
    reference::Reference,
};

/// Captures everything needed to execute a push once analysis is finished.
#[derive(Debug)]
struct PushPlan {
    source_tar: String,
    target: String,
    target_ref: Reference,
    username: String,
    password: String,
}

/// Coordinates the push workflow so helper functions remain grouped.
pub struct PushWorkflow<'a> {
    client: &'a Client,
    chunk_size_bytes: usize,
    upload_parallelism: usize,
}

impl<'a> PushWorkflow<'a> {
    pub fn new(client: &'a Client, chunk_size_bytes: usize, upload_parallelism: usize) -> Self {
        Self {
            client,
            chunk_size_bytes,
            upload_parallelism: upload_parallelism.max(1),
        }
    }

    /// High-level push orchestration invoked by the CLI entry point.
    pub async fn run(
        &self,
        tar_path: &str,
        target_image: Option<String>,
        username: Option<String>,
        password: Option<String>,
        registry_override: Option<String>,
    ) -> Result<(), PusherError> {
        let tar_path_obj = Path::new(tar_path);
        if !tar_path_obj.is_file() {
            return Err(PusherError::push_error(
                "Push requires a docker-save tar archive. Run `docker-image-pusher save` first.",
            ));
        }
        println!("📦 Preparing to push Docker archive: {}", tar_path);
        let tar_info = tar_repo_info_from_path(tar_path)?;

        let plan = self
            .prepare_push_plan(
                tar_path,
                &tar_info,
                target_image,
                username,
                password,
                registry_override,
            )
            .await?;

        self.execute_push_plan(&plan).await?;
        state::record_push_target(&plan.target).await?;
        println!("✅ Successfully pushed image: {}", plan.target);
        Ok(())
    }

    /// Resolves target, credentials, and metadata before uploading begins.
    async fn prepare_push_plan(
        &self,
        tar_path: &str,
        tar_info: &TarRepoInfo,
        target_image: Option<String>,
        username: Option<String>,
        password: Option<String>,
        registry_override: Option<String>,
    ) -> Result<PushPlan, PusherError> {
        let mut target = self
            .determine_target(target_image, tar_info, registry_override)
            .await?;
        target = Self::ensure_target_from_tar_metadata(target, tar_info).await?;
        println!("🎯 Target image resolved as: {}", target);

        let explicit_credentials_requested = username.is_some() || password.is_some();
        let (final_target, target_ref, resolved_username, resolved_password) =
            Self::resolve_target_credentials(
                target,
                tar_info,
                username,
                password,
                explicit_credentials_requested,
            )
            .await?;

        Ok(PushPlan {
            source_tar: tar_path.to_string(),
            target: final_target,
            target_ref,
            username: resolved_username,
            password: resolved_password,
        })
    }

    /// Performs the actual tar extraction followed by the streaming upload sequence.
    async fn execute_push_plan(&self, plan: &PushPlan) -> Result<(), PusherError> {
        self.push_tar_archive(
            &plan.source_tar,
            &plan.target_ref,
            &plan.username,
            &plan.password,
            &plan.target,
        )
        .await
    }

    /// Chooses a destination reference using CLI overrides, history, or tar metadata.
    async fn determine_target(
        &self,
        provided_target: Option<String>,
        tar_info: &TarRepoInfo,
        registry_override: Option<String>,
    ) -> Result<String, PusherError> {
        if let Some(target) = provided_target {
            return Ok(target);
        }

        if let Some(history_target) = infer_target_from_history(tar_info).await? {
            return Ok(history_target);
        }

        let fallback = build_target_from_tar(tar_info, registry_override.as_deref());
        println!(
            "💡 No recent history matched. Using target derived from tar metadata: {}",
            fallback
        );
        Ok(fallback)
    }

    /// Ensures valid username/password material for the chosen registry.
    async fn resolve_target_credentials(
        target: String,
        tar_info: &TarRepoInfo,
        username: Option<String>,
        password: Option<String>,
        explicit_credentials_requested: bool,
    ) -> Result<(String, Reference, String, String), PusherError> {
        loop {
            let target_ref: Reference = target.parse().map_err(|e| {
                PusherError::PushError(format!("Invalid target image reference: {}", e))
            })?;

            match Self::load_credentials_for_registry(
                username.clone(),
                password.clone(),
                target_ref.registry_host(),
            )
            .await
            {
                Ok((resolved_username, resolved_password)) => {
                    return Ok((target, target_ref, resolved_username, resolved_password));
                }
                Err(err) => {
                    if !explicit_credentials_requested && Self::is_missing_credentials_error(&err) {
                        if let Some((replacement_target, creds)) =
                            Self::attempt_registry_inference(tar_info).await?
                        {
                            if replacement_target != target {
                                println!("🔁 Switching to inferred target: {}", replacement_target);
                            }
                            let replacement_ref = replacement_target.parse().map_err(|e| {
                                PusherError::PushError(format!(
                                    "Invalid target image reference: {}",
                                    e
                                ))
                            })?;
                            return Ok((replacement_target, replacement_ref, creds.0, creds.1));
                        }
                    }
                    return Err(err);
                }
            }
        }
    }

    /// Uploads all layers/config extracted from a docker-save tarball.
    async fn push_tar_archive(
        &self,
        tar_path: &str,
        target_ref: &Reference,
        username: &str,
        password: &str,
        target_label: &str,
    ) -> Result<(), PusherError> {
        let auth = Arc::new(RegistryAuth::basic(username, password));
        println!(
            "🔐 Preparing registry session with {}",
            target_ref.registry_host()
        );
        println!(
            "📤 Uploading layers with up to {} concurrent streams...",
            self.upload_parallelism
        );

        let (layer_tx, mut layer_rx) = mpsc::channel::<ExtractedLayer>(self.upload_parallelism * 2);
        let tar_path_string = tar_path.to_string();
        let extraction_handle = task::spawn_blocking(move || {
            extract_tar_archive_with_sender(&tar_path_string, Some(layer_tx))
        });

        let mut in_flight: FuturesUnordered<_> = FuturesUnordered::new();
        let mut uploaded_layers = Vec::new();
        let mut skipped_uploads = 0usize;
        let mut receiver_closed = false;

        loop {
            while in_flight.len() < self.upload_parallelism && !receiver_closed {
                match layer_rx.recv().await {
                    Some(layer) => {
                        let auth = Arc::clone(&auth);
                        in_flight.push(self.upload_layer_task(layer, target_ref, auth));
                    }
                    None => {
                        receiver_closed = true;
                    }
                }
            }

            match in_flight.next().await {
                Some(result) => {
                    let outcome = result?;
                    if outcome.skipped {
                        skipped_uploads += 1;
                    }
                    uploaded_layers.push(outcome.digest);
                }
                None => {
                    if receiver_closed {
                        break;
                    }
                }
            }
        }

        let extraction = extraction_handle.await.map_err(|err| {
            PusherError::push_error(format!("Tar extraction task failed: {}", err))
        })??;

        if skipped_uploads > 0 {
            println!(
                "💡 Skipped {} layer(s) that already existed in the registry",
                skipped_uploads
            );
        }

        println!("⚙️  Uploading config: {}", extraction.config_digest);
        self.client
            .push_blob(
                target_ref,
                auth.as_ref(),
                &extraction.config_contents,
                &extraction.config_digest,
            )
            .await
            .map_err(|e| PusherError::PushError(format!("Failed to upload config: {}", e)))?;

        println!("📋 Pushing manifest to registry: {}", target_label);
        let manifest = build_manifest(&extraction);
        let manifest_url = self
            .client
            .push_manifest(target_ref, &manifest, auth.as_ref())
            .await
            .map_err(|e| PusherError::PushError(format!("Failed to push manifest: {}", e)))?;

        println!(
            "🎉 Successfully pushed {} layers to {}",
            uploaded_layers.len(),
            manifest_url
        );
        Ok(())
    }

    /// Provides a best-effort existence check before uploading a layer.
    async fn blob_exists_in_registry(
        &self,
        target_ref: &Reference,
        auth: &RegistryAuth,
        digest: &str,
    ) -> Result<bool, PusherError> {
        match self.client.blob_exists(target_ref, digest, auth).await {
            Ok(exists) => Ok(exists),
            Err(err) => {
                println!(
                    "   ⚠️  Unable to check blob {} presence in registry (continuing with upload): {}",
                    digest, err
                );
                Ok(false)
            }
        }
    }

    /// Streams large layers via the chunked upload pipeline with telemetry.
    async fn upload_large_layer(
        &self,
        target_ref: &Reference,
        auth: &RegistryAuth,
        layer_path: &Path,
        digest: &str,
        layer_size_mb: f64,
        layer_size_bytes: u64,
    ) -> Result<(), PusherError> {
        println!(
            "   🔄 Chunk-streaming large layer ({:.1} MB) in {:.0} MB chunks (auto-adjusting)...",
            layer_size_mb,
            self.chunk_size_bytes as f64 / (1024.0 * 1024.0)
        );

        let upload_start = Instant::now();
        let file = tokio::fs::File::open(layer_path).await.map_err(|e| {
            PusherError::CacheError(format!("Failed to open extracted layer {}: {}", digest, e))
        })?;
        let mut reader = file;

        if layer_size_mb > 1000.0 {
            let estimated_time_min = layer_size_mb / ESTIMATED_SPEED_MBPS / 60.0;
            println!(
                "   ⏱️  Estimated upload time: {:.1}-{:.1} minutes",
                estimated_time_min * 0.5,
                estimated_time_min * 2.0
            );
        }

        let network_start = Instant::now();
        let bytes_sent = Arc::new(AtomicU64::new(0));
        let progress_handle = create_progress_tracker(
            layer_size_mb,
            layer_size_bytes,
            network_start,
            digest,
            Arc::clone(&bytes_sent),
        );

        let upload_result = self
            .client
            .push_blob_stream(
                target_ref,
                auth,
                &mut reader,
                digest,
                self.chunk_size_bytes,
                Some(bytes_sent),
            )
            .await;

        if let Some(handle) = progress_handle {
            handle.abort();
        }

        upload_result.map_err(|e| {
            PusherError::PushError(format!("Failed to upload layer {}: {}", digest, e))
        })?;

        let network_duration = network_start.elapsed();
        let total_duration = upload_start.elapsed();
        let upload_speed = if network_duration.as_secs() > 0 {
            (layer_size_bytes as f64 / (1024.0 * 1024.0)) / network_duration.as_secs_f64()
        } else {
            0.0
        };

        println!(
            "   ⚡ Chunked upload completed! Total: {:.1}s (upload: {:.1}s) @ {:.1} MB/s",
            total_duration.as_secs_f64(),
            network_duration.as_secs_f64(),
            upload_speed
        );

        if layer_size_mb > 1000.0 {
            let gb_transferred = layer_size_mb / 1024.0;
            println!(
                "   🎉 Successfully transferred {:.2} GB in {:.1} minutes",
                gb_transferred,
                network_duration.as_secs_f64() / 60.0
            );
        }

        Ok(())
    }

    /// Uploads small layers by reading them entirely into memory first.
    async fn upload_small_layer(
        &self,
        target_ref: &Reference,
        auth: &RegistryAuth,
        layer_path: &Path,
        digest: &str,
        layer_size_mb: f64,
    ) -> Result<(), PusherError> {
        println!("   📤 Uploading layer directly...");

        let read_start = Instant::now();
        let layer_data = tokio::fs::read(layer_path).await.map_err(|e| {
            PusherError::CacheError(format!("Failed to read extracted layer {}: {}", digest, e))
        })?;

        let read_duration = read_start.elapsed();
        let upload_start = Instant::now();

        self.client
            .push_blob(target_ref, auth, &layer_data, digest)
            .await
            .map_err(|e| {
                PusherError::PushError(format!("Failed to upload layer {}: {}", digest, e))
            })?;

        let upload_duration = upload_start.elapsed();
        let total_duration = read_start.elapsed();
        let speed = if total_duration.as_secs() > 0 {
            layer_size_mb / total_duration.as_secs_f64()
        } else {
            0.0
        };

        println!(
            "   ⚡ Completed in {:.1}s (read: {:.1}ms, upload: {:.1}s) @ {:.1} MB/s",
            total_duration.as_secs_f64(),
            read_duration.as_millis(),
            upload_duration.as_secs_f64(),
            speed
        );

        Ok(())
    }

    /// Appends repository/tag info from the docker-save manifest when missing.
    async fn ensure_target_from_tar_metadata(
        target: String,
        info: &TarRepoInfo,
    ) -> Result<String, PusherError> {
        let reference: Reference = target.parse().map_err(|e| {
            PusherError::PushError(format!("Invalid target image reference: {}", e))
        })?;

        if reference.tag.is_some() || reference.digest.is_some() {
            return Ok(target);
        }

        let sanitized_repo = reference.repository.trim_end_matches('/');
        let new_repository = if sanitized_repo.is_empty() {
            info.repository.clone()
        } else if sanitized_repo
            .split('/')
            .last()
            .map(|segment| segment == info.image_name)
            .unwrap_or(false)
        {
            sanitized_repo.to_string()
        } else {
            format!("{}/{}", sanitized_repo, info.image_name)
        };

        let completed = format!("{}/{}:{}", reference.registry, new_repository, info.tag);

        Self::confirm_or_wait_for_target(
            &completed,
            "Target missing tag; inferred repository/tag from tar metadata.",
        )
        .await?;

        Ok(completed)
    }

    /// Suggests alternate registries using stored credentials when lookup fails.
    async fn attempt_registry_inference(
        info: &TarRepoInfo,
    ) -> Result<Option<(String, (String, String))>, PusherError> {
        let mut stored = state::all_credentials().await?;
        if stored.is_empty() {
            println!("⚠️  No stored credentials found. Unable to infer alternate registry target.");
            return Ok(None);
        }

        let mut ordered: Vec<state::StoredCredential> = Vec::new();
        if let Ok(history) = state::recent_targets().await {
            for entry in history {
                if let Ok(reference) = Reference::parse(&entry) {
                    if let Some(pos) = stored
                        .iter()
                        .position(|cred| cred.registry == reference.registry)
                    {
                        ordered.push(stored.remove(pos));
                    }
                }
            }
        }

        ordered.extend(stored.into_iter());

        for credential in ordered {
            let candidate = format!(
                "{}/{}:{}",
                credential.registry.trim_end_matches('/'),
                info.repository,
                info.tag
            );
            println!(
                "🧭 Suggested alternate target '{}' using stored login {}",
                candidate, credential.registry
            );
            Self::confirm_or_wait_for_target(
                &candidate,
                "Detected missing login. Proposing alternate target based on stored credentials.",
            )
            .await?;
            return Ok(Some((
                candidate,
                (credential.username, credential.password),
            )));
        }

        Ok(None)
    }

    /// Gives users a chance to confirm inferred targets before pushing.
    async fn confirm_or_wait_for_target(candidate: &str, reason: &str) -> Result<(), PusherError> {
        match state::recent_targets().await {
            Ok(history) => {
                if history.iter().any(|entry| entry == candidate) {
                    println!(
                        "💡 {} Previously confirmed '{}'. Continuing in 3 seconds... (Ctrl+C to abort)",
                        reason, candidate
                    );
                    time::sleep(Duration::from_secs(3)).await;
                    return Ok(());
                }
            }
            Err(err) => println!(
                "⚠️  Unable to read push history for confirmation checks: {}",
                err
            ),
        }

        let question = format!("{} Proceed with '{}' ? [Y/n]: ", reason, candidate);
        if Self::prompt_yes_no(&question).await? {
            Ok(())
        } else {
            Err(PusherError::push_error(
                "User declined inferred target suggestion",
            ))
        }
    }

    /// Blocking stdin prompt executed on a background thread to avoid stalling the runtime.
    async fn prompt_yes_no(prompt: &str) -> Result<bool, PusherError> {
        let prompt = prompt.to_string();
        task::spawn_blocking(move || {
            use std::io::{self, Write};

            print!("{}", prompt);
            io::stdout().flush().map_err(PusherError::IoError)?;

            let mut input = String::new();
            io::stdin()
                .read_line(&mut input)
                .map_err(PusherError::IoError)?;

            let answer = input.trim().to_lowercase();
            Ok(answer.is_empty() || answer == "y" || answer == "yes")
        })
        .await
        .map_err(|err| PusherError::push_error(format!("Failed to read confirmation: {}", err)))?
    }

    fn is_missing_credentials_error(err: &PusherError) -> bool {
        matches!(
            err,
            PusherError::PushError(message) if message.contains("Credentials for registry")
        )
    }

    /// Loads stored credentials unless the CLI explicitly overrides them.
    async fn load_credentials_for_registry(
        username: Option<String>,
        password: Option<String>,
        registry: &str,
    ) -> Result<(String, String), PusherError> {
        match (username, password) {
            (Some(user), Some(pass)) => Ok((user, pass)),
            (Some(_), None) | (None, Some(_)) => Err(PusherError::push_error(
                "Both username and password must be provided when using CLI flags",
            )),
            (None, None) => match state::load_credentials(registry).await {
                Ok(Some((stored_user, stored_pass))) => {
                    println!("🔑 Using stored credentials for {}", registry);
                    Ok((stored_user, stored_pass))
                }
                Ok(None) => Err(PusherError::push_error(format!(
                    "Credentials for registry '{}' not found. Run 'login' or pass --username/--password",
                    registry
                ))),
                Err(err) => Err(PusherError::push_error(format!(
                    "Failed to load stored credentials: {}",
                    err
                ))),
            },
        }
    }

    fn upload_layer_task<'b>(
        &'b self,
        layer: ExtractedLayer,
        target_ref: &'b Reference,
        auth: Arc<RegistryAuth>,
    ) -> BoxFuture<'b, Result<LayerUploadOutcome, PusherError>> {
        Box::pin(async move {
            self.upload_single_layer(layer, target_ref, auth.as_ref())
                .await
        })
    }

    async fn upload_single_layer(
        &self,
        layer: ExtractedLayer,
        target_ref: &Reference,
        auth: &RegistryAuth,
    ) -> Result<LayerUploadOutcome, PusherError> {
        let layer_size_mb = layer.size as f64 / (1024.0 * 1024.0);
        println!(
            "📦 Uploading layer {} ({:.1} MB)",
            layer.digest, layer_size_mb
        );

        if self
            .blob_exists_in_registry(target_ref, auth, &layer.digest)
            .await?
        {
            println!(
                "   ✅ Layer already exists in registry, skipping upload: {}",
                layer.digest
            );
            return Ok(LayerUploadOutcome {
                digest: layer.digest,
                skipped: true,
            });
        }

        if layer.size >= LARGE_LAYER_THRESHOLD_BYTES {
            self.upload_large_layer(
                target_ref,
                auth,
                &layer.path,
                &layer.digest,
                layer_size_mb,
                layer.size,
            )
            .await?;
        } else {
            self.upload_small_layer(target_ref, auth, &layer.path, &layer.digest, layer_size_mb)
                .await?;
        }

        println!("   ✅ Successfully uploaded layer {}", layer.digest);

        if layer_size_mb > MEDIUM_LAYER_THRESHOLD_MB {
            time::sleep(Duration::from_millis(RATE_LIMIT_DELAY_MS)).await;
        }

        Ok(LayerUploadOutcome {
            digest: layer.digest,
            skipped: false,
        })
    }
}

struct LayerUploadOutcome {
    digest: String,
    skipped: bool,
}

/// Entry point for the push command. Handles tar imports, target inference, credential
/// resolution, and delegates to the low-level upload helpers.
pub async fn run_push(
    client: &Client,
    input: &str,
    target_image: Option<String>,
    username: Option<String>,
    password: Option<String>,
    registry_override: Option<String>,
    blob_chunk: Option<usize>,
) -> Result<(), PusherError> {
    let chunk_size_bytes = match blob_chunk {
        Some(0) => {
            return Err(PusherError::push_error(
                "--blob-chunk must be greater than 0 MB",
            ));
        }
        Some(mb) => {
            let requested = mb.checked_mul(1024 * 1024).ok_or_else(|| {
                PusherError::push_error("--blob-chunk value is too large for this platform")
            })?;
            if requested > MAX_CHUNKED_LAYER_SIZE_BYTES {
                return Err(PusherError::push_error(format!(
                    "--blob-chunk cannot exceed {} MB",
                    MAX_CHUNKED_LAYER_SIZE_BYTES / (1024 * 1024)
                )));
            }
            requested
        }
        None => CHUNKED_LAYER_SIZE_BYTES,
    };

    PushWorkflow::new(client, chunk_size_bytes, DEFAULT_UPLOAD_CONCURRENCY)
        .run(input, target_image, username, password, registry_override)
        .await
}

/// Formats megabytes into either MB or GB for progress logging.
fn format_size_display(size_mb: f64) -> (f64, &'static str) {
    if size_mb > 1024.0 {
        (size_mb / 1024.0, "GB")
    } else {
        (size_mb, "MB")
    }
}

/// Spawns the periodic progress reporter for large uploads.
fn create_progress_tracker(
    layer_size_mb: f64,
    layer_size_bytes: u64,
    network_start: Instant,
    digest: &str,
    bytes_sent: Arc<AtomicU64>,
) -> Option<task::JoinHandle<()>> {
    if layer_size_mb <= LARGE_LAYER_THRESHOLD_MB {
        return None;
    }

    let layer_size_mb_clone = layer_size_mb;
    let network_start_clone = network_start;
    let bytes_sent_clone = bytes_sent;
    let digest_suffix = digest.chars().skip(digest.len() - 8).collect::<String>();
    let interval_secs = if layer_size_mb > 1000.0 {
        LARGE_LAYER_PROGRESS_INTERVAL_SECS
    } else {
        NORMAL_LAYER_PROGRESS_INTERVAL_SECS
    };

    Some(tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(interval_secs));
        let mut progress_counter = 1;

        loop {
            interval.tick().await;
            let elapsed = network_start_clone.elapsed();

            let elapsed_secs = elapsed.as_secs();

            let sent_bytes = bytes_sent_clone.load(Ordering::Relaxed);
            if sent_bytes == 0 && elapsed_secs < 5 {
                continue;
            }

            let elapsed_min = elapsed.as_secs_f64() / 60.0;
            let sent_mb = sent_bytes as f64 / (1024.0 * 1024.0);
            let total_mb = layer_size_mb_clone;
            let percent = if layer_size_bytes > 0 {
                (sent_bytes as f64 / layer_size_bytes as f64 * 100.0).min(100.0)
            } else {
                0.0
            };

            let speed_mbps = if elapsed_secs > 0 {
                sent_mb / elapsed.as_secs_f64()
            } else {
                0.0
            };
            let remaining_mb = (total_mb - sent_mb).max(0.0);
            let eta_min = if speed_mbps > 0.0 {
                remaining_mb / speed_mbps / 60.0
            } else {
                0.0
            };

            let (transferred_display, unit) = format_size_display(sent_mb);
            let (total_display, _) = format_size_display(total_mb);

            println!(
                "   ⏳ Upload progress #{}: {:.1}% | {:.1}/{:.1} {} | Speed: {:.1} MB/s | ETA: {:.1}min",
                progress_counter,
                percent,
                transferred_display,
                total_display,
                unit,
                speed_mbps,
                eta_min
            );

            if progress_counter % 2 == 0 {
                println!(
                    "   📊 Data transferred: {}/{} bytes | Elapsed: {:.1}min | Layer: ...{}",
                    sent_bytes, layer_size_bytes, elapsed_min, digest_suffix
                );
            }

            if progress_counter % 3 == 0 && layer_size_mb_clone > 1000.0 {
                let gb_size = total_mb / 1024.0;
                let completion_percent = percent;
                println!(
                    "   📈 Network: {:.2} GB total | Avg: {:.1} MB/s | Progress: {:.1}% | Large transfer in progress",
                    gb_size, speed_mbps, completion_percent
                );
            }

            progress_counter += 1;
        }
    }))
}

/// Rebuilds the OCI manifest describing the image extracted from the tarball.
fn build_manifest(extraction: &TarExtraction) -> OciImageManifest {
    let diff_ids = collect_layer_diff_ids(&extraction.config_contents);
    if !diff_ids.is_empty() && diff_ids.len() != extraction.layers.len() {
        println!(
            "⚠️  Config reports {} diff_ids but archive has {} layers",
            diff_ids.len(),
            extraction.layers.len()
        );
    }

    let config_descriptor = OciDescriptor {
        media_type: "application/vnd.docker.container.image.v1+json".to_string(),
        digest: extraction.config_digest.clone(),
        size: extraction.config_contents.len() as i64,
        urls: Vec::new(),
    };

    let mut layer_descriptors = Vec::with_capacity(extraction.layers.len());
    for (index, layer) in extraction.layers.iter().enumerate() {
        if index < diff_ids.len() {
            println!(
                "   🧩 Layer {}/{} maps diff_id {}",
                index + 1,
                extraction.layers.len(),
                diff_ids[index]
            );
        }
        layer_descriptors.push(OciDescriptor {
            media_type: layer.media_type.clone(),
            digest: layer.digest.clone(),
            size: layer.size as i64,
            urls: Vec::new(),
        });
    }

    OciImageManifest {
        schema_version: 2,
        media_type: "application/vnd.docker.distribution.manifest.v2+json".to_string(),
        config: config_descriptor,
        layers: layer_descriptors,
    }
}

/// Parses `rootfs.diff_ids` out of the config JSON for debug logging.
fn collect_layer_diff_ids(config_bytes: &[u8]) -> Vec<String> {
    match serde_json::from_slice::<Value>(config_bytes) {
        Ok(value) => value["rootfs"]["diff_ids"]
            .as_array()
            .map(|diffs| {
                diffs
                    .iter()
                    .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default(),
        Err(err) => {
            println!("⚠️  Unable to inspect config diff_ids: {}", err);
            Vec::new()
        }
    }
}
