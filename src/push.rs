use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde_json;
use tokio::{
    task,
    time::{self, Duration},
};

use crate::{
    CACHE_DIR, CHUNKED_LAYER_SIZE_BYTES, ESTIMATED_SPEED_MBPS, LARGE_LAYER_PROGRESS_INTERVAL_SECS,
    LARGE_LAYER_THRESHOLD_BYTES, LARGE_LAYER_THRESHOLD_MB, MAX_CHUNKED_LAYER_SIZE_BYTES,
    MEDIUM_LAYER_THRESHOLD_MB, NORMAL_LAYER_PROGRESS_INTERVAL_SECS, PusherError,
    RATE_LIMIT_DELAY_MS, cache, image, state,
    tar_import::{
        TarRepoInfo, build_target_from_tar, import_tar_file, infer_target_from_history,
        tar_repo_info_from_path,
    },
};
use oci_core::{
    auth::RegistryAuth, client::Client, manifest::OciImageManifest, reference::Reference,
};

/// Captures everything needed to execute a push once analysis is finished.
#[derive(Debug)]
struct PushPlan {
    cache_key: String,
    target: String,
    target_ref: Reference,
    username: String,
    password: String,
}

/// Describes the source material for a push invocation.
#[derive(Debug)]
struct InputContext {
    is_tar_source: bool,
    tar_info: Option<TarRepoInfo>,
}

/// Coordinates the push workflow so helper functions remain grouped.
pub struct PushWorkflow<'a> {
    client: &'a Client,
    chunk_size_bytes: usize,
}

impl<'a> PushWorkflow<'a> {
    pub fn new(client: &'a Client, chunk_size_bytes: usize) -> Self {
        Self {
            client,
            chunk_size_bytes,
        }
    }

    pub async fn run(
        &self,
        input: &str,
        target_image: Option<String>,
        username: Option<String>,
        password: Option<String>,
        registry_override: Option<String>,
    ) -> Result<(), PusherError> {
        let plan = self
            .prepare_push_plan(input, target_image, username, password, registry_override)
            .await?;

        self.execute_push_plan(&plan).await?;
        state::record_push_target(&plan.target).await?;
        println!("✅ Successfully pushed image: {}", plan.target);
        Ok(())
    }

    async fn prepare_push_plan(
        &self,
        input: &str,
        target_image: Option<String>,
        username: Option<String>,
        password: Option<String>,
        registry_override: Option<String>,
    ) -> Result<PushPlan, PusherError> {
        let context = self.analyze_input_source(input)?;
        let mut target = self
            .determine_target(target_image, context.tar_info.as_ref(), registry_override)
            .await?;

        if let Some(info) = context.tar_info.as_ref() {
            target = Self::ensure_target_from_tar_metadata(target, info).await?;
        }
        println!("🎯 Target image resolved as: {}", target);

        let explicit_credentials_requested = username.is_some() || password.is_some();
        let (final_target, target_ref, resolved_username, resolved_password) =
            Self::resolve_target_credentials(
                target,
                context.tar_info.as_ref(),
                username,
                password,
                explicit_credentials_requested,
            )
            .await?;

        let cache_key = self
            .prepare_source_cache(input, &final_target, context.is_tar_source)
            .await?;

        Ok(PushPlan {
            cache_key,
            target: final_target,
            target_ref,
            username: resolved_username,
            password: resolved_password,
        })
    }

    async fn execute_push_plan(&self, plan: &PushPlan) -> Result<(), PusherError> {
        self.push_cached_image(
            &plan.cache_key,
            &plan.target,
            &plan.target_ref,
            &plan.username,
            &plan.password,
        )
        .await
    }

    fn analyze_input_source(&self, input: &str) -> Result<InputContext, PusherError> {
        let input_path = Path::new(input);
        if input_path.is_file() {
            println!("📦 Preparing to push Docker archive: {}", input);
            let info = tar_repo_info_from_path(input)?;
            Ok(InputContext {
                is_tar_source: true,
                tar_info: Some(info),
            })
        } else {
            println!("📤 Pushing cached image: {}", input);
            Ok(InputContext {
                is_tar_source: false,
                tar_info: None,
            })
        }
    }

    async fn determine_target(
        &self,
        provided_target: Option<String>,
        tar_info: Option<&TarRepoInfo>,
        registry_override: Option<String>,
    ) -> Result<String, PusherError> {
        if let Some(target) = provided_target {
            return Ok(target);
        }

        if let Some(info) = tar_info {
            if let Some(history_target) = infer_target_from_history(info).await? {
                return Ok(history_target);
            }

            let fallback = build_target_from_tar(info, registry_override.as_deref());
            println!(
                "💡 No recent history matched. Using target derived from tar metadata: {}",
                fallback
            );
            return Ok(fallback);
        }

        Err(PusherError::PushError(
            "Unable to determine target image. Provide --target or ensure the tar archive contains RepoTags.".to_string(),
        ))
    }

    async fn resolve_target_credentials(
        target: String,
        tar_info: Option<&TarRepoInfo>,
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
                    if !explicit_credentials_requested
                        && Self::is_missing_credentials_error(&err)
                        && tar_info.is_some()
                    {
                        if let Some((replacement_target, creds)) =
                            Self::attempt_registry_inference(tar_info.unwrap()).await?
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

    async fn prepare_source_cache(
        &self,
        input: &str,
        target: &str,
        is_tar_source: bool,
    ) -> Result<String, PusherError> {
        if is_tar_source {
            let cache_name = image::sanitize_image_name(target);
            println!("🧩 Importing tar archive into cache entry: {}", cache_name);
            import_tar_file(input, &cache_name).await?;
            Ok(cache_name)
        } else {
            if !cache::has_cached_image(input).await? {
                println!("⚠️  Image not found in cache, pulling first...");
                cache::cache_image(self.client, input).await?;
            }
            Ok(input.to_string())
        }
    }

    async fn push_cached_image(
        &self,
        source_image: &str,
        target: &str,
        target_ref: &Reference,
        username: &str,
        password: &str,
    ) -> Result<(), PusherError> {
        let cache_dir = Path::new(CACHE_DIR);
        let image_cache_dir = cache_dir.join(image::sanitize_image_name(source_image));
        let auth = RegistryAuth::basic(username, password);

        println!(
            "🔐 Preparing registry session with {}",
            target_ref.registry_host()
        );

        let index_path = image_cache_dir.join("index.json");
        let index_content = tokio::fs::read_to_string(&index_path)
            .await
            .map_err(|_| PusherError::CacheNotFound)?;
        let index: serde_json::Value = serde_json::from_str(&index_content)?;

        let manifest_path = image_cache_dir.join("manifest.json");
        let manifest_content = tokio::fs::read_to_string(&manifest_path)
            .await
            .map_err(|e| {
                PusherError::CacheError(format!("Failed to read cached manifest: {}", e))
            })?;
        let manifest: OciImageManifest = serde_json::from_str(&manifest_content)?;

        let layer_digests: Vec<String> = index["layers"]
            .as_array()
            .ok_or(PusherError::CacheError(
                "Invalid layers format in index".to_string(),
            ))?
            .iter()
            .map(|v| v.as_str().unwrap_or("").to_string())
            .collect();
        println!(
            "📤 Uploading {} cached layers sequentially with memory optimization...",
            layer_digests.len()
        );

        let mut uploaded_layers = Vec::new();
        let mut skipped_uploads = 0;

        for (i, digest) in layer_digests.iter().enumerate() {
            let layer_path = image_cache_dir.join(digest.replace(":", "_"));
            let layer_metadata = tokio::fs::metadata(&layer_path).await.map_err(|e| {
                PusherError::CacheError(format!("Failed to get layer metadata {}: {}", digest, e))
            })?;
            let layer_size_bytes = layer_metadata.len();
            let layer_size_mb = layer_size_bytes as f64 / (1024.0 * 1024.0);

            println!(
                "📦 Uploading layer {}/{}: {} ({:.1} MB)",
                i + 1,
                layer_digests.len(),
                digest,
                layer_size_mb
            );

            if self
                .blob_exists_in_registry(target_ref, &auth, digest)
                .await?
            {
                println!(
                    "   ✅ Layer already exists in registry, skipping upload: {}",
                    digest
                );
                uploaded_layers.push(digest.clone());
                skipped_uploads += 1;
                continue;
            }

            if layer_size_bytes >= LARGE_LAYER_THRESHOLD_BYTES {
                self.upload_large_layer(
                    target_ref,
                    &auth,
                    &layer_path,
                    digest,
                    layer_size_mb,
                    layer_size_bytes,
                )
                .await?;
            } else {
                self.upload_small_layer(target_ref, &auth, &layer_path, digest, layer_size_mb)
                    .await?;
            }

            println!("   ✅ Successfully uploaded layer {}", digest);

            if layer_size_mb > MEDIUM_LAYER_THRESHOLD_MB {
                time::sleep(Duration::from_millis(RATE_LIMIT_DELAY_MS)).await;
            }
            uploaded_layers.push(digest.clone());
        }

        println!(
            "🚀 Sequential upload completed for {} layers",
            uploaded_layers.len()
        );
        if skipped_uploads > 0 {
            println!(
                "💡 Skipped {} layers that already existed in registry",
                skipped_uploads
            );
        }

        let config_digest = index["config"]
            .as_str()
            .ok_or(PusherError::CacheError("Invalid index format".to_string()))?;
        let config_path =
            image_cache_dir.join(format!("config_{}.json", config_digest.replace(":", "_")));

        println!("⚙️  Uploading config: {}", config_digest);
        let config_data = tokio::fs::read(&config_path)
            .await
            .map_err(|e| PusherError::CacheError(format!("Failed to read cached config: {}", e)))?;

        self.client
            .push_blob(&target_ref, &auth, &config_data, config_digest)
            .await
            .map_err(|e| PusherError::PushError(format!("Failed to upload config: {}", e)))?;

        println!("📋 Pushing manifest to registry: {}", target);
        let manifest_url = self
            .client
            .push_manifest(&target_ref, &manifest, &auth)
            .await
            .map_err(|e| PusherError::PushError(format!("Failed to push manifest: {}", e)))?;

        println!(
            "🎉 Successfully pushed {} layers to {}",
            uploaded_layers.len(),
            manifest_url
        );
        Ok(())
    }

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
            PusherError::CacheError(format!("Failed to open cached layer {}: {}", digest, e))
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
            PusherError::CacheError(format!("Failed to read cached layer {}: {}", digest, e))
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

    PushWorkflow::new(client, chunk_size_bytes)
        .run(input, target_image, username, password, registry_override)
        .await
}

fn format_size_display(size_mb: f64) -> (f64, &'static str) {
    if size_mb > 1024.0 {
        (size_mb / 1024.0, "GB")
    } else {
        (size_mb, "MB")
    }
}

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
