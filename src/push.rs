use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use tokio::{
    sync::mpsc,
    task,
    time::{self, Duration},
};

use crate::{
    CHUNKED_LAYER_SIZE_BYTES, ESTIMATED_SPEED_MBPS, LARGE_LAYER_PROGRESS_INTERVAL_SECS,
    LARGE_LAYER_THRESHOLD_BYTES, LARGE_LAYER_THRESHOLD_MB, MAX_CHUNKED_LAYER_SIZE_BYTES,
    NORMAL_LAYER_PROGRESS_INTERVAL_SECS, PusherError, RATE_LIMIT_DELAY_MS,
    progress_display::docker_like_progress_reporter,
    state,
    tar_import::{
        TarExtraction, TarRepoInfo, build_target_from_tar, extract_tar_archive_with_sender,
        infer_target_from_history, tar_repo_info_from_path,
    },
};

const DEFAULT_UPLOAD_CONCURRENCY: usize = 3;
const MEDIUM_LAYER_THRESHOLD_MB: f64 = 250.0;
use oci_core::{
    auth::RegistryAuth,
    blobs::{LayerUploadOptions, LayerUploadPool, LocalLayer, ProgressOptions},
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

        let (layer_tx, layer_rx) = mpsc::channel::<LocalLayer>(self.upload_parallelism * 2);
        let tar_path_string = tar_path.to_string();
        let extraction_handle = task::spawn_blocking(move || {
            extract_tar_archive_with_sender(&tar_path_string, Some(layer_tx))
        });

        let upload_options = LayerUploadOptions {
            chunk_size_bytes: self.chunk_size_bytes,
            large_layer_threshold_bytes: LARGE_LAYER_THRESHOLD_BYTES,
            medium_layer_threshold_mb: MEDIUM_LAYER_THRESHOLD_MB,
            rate_limit_delay_ms: RATE_LIMIT_DELAY_MS,
            concurrency: self.upload_parallelism,
            progress: ProgressOptions {
                large_layer_threshold_mb: LARGE_LAYER_THRESHOLD_MB,
                large_interval_secs: LARGE_LAYER_PROGRESS_INTERVAL_SECS,
                normal_interval_secs: NORMAL_LAYER_PROGRESS_INTERVAL_SECS,
                estimated_speed_mbps: ESTIMATED_SPEED_MBPS,
            },
            progress_reporter: Some(docker_like_progress_reporter()),
        };

        let uploader =
            LayerUploadPool::new(self.client, target_ref, Arc::clone(&auth), upload_options);
        let upload_summary = uploader
            .upload_stream(layer_rx)
            .await
            .map_err(|err| PusherError::PushError(format!("Failed to upload layers: {}", err)))?;

        let extraction = extraction_handle.await.map_err(|err| {
            PusherError::push_error(format!("Tar extraction task failed: {}", err))
        })??;

        if upload_summary.skipped > 0 {
            println!(
                "💡 Skipped {} layer(s) that already existed in the registry",
                upload_summary.skipped
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
            upload_summary.uploaded.len(),
            manifest_url
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
