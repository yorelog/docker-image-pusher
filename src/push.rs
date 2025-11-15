use std::path::Path;
use std::time::Instant;

use serde_json;
use tokio::{
    io::BufReader,
    task,
    time::{self, Duration},
};

use crate::{
    CACHE_DIR, CHUNKED_LAYER_SIZE_BYTES, ESTIMATED_SPEED_MBPS, LARGE_LAYER_PROGRESS_INTERVAL_SECS,
    LARGE_LAYER_THRESHOLD_BYTES, LARGE_LAYER_THRESHOLD_MB, MEDIUM_LAYER_THRESHOLD_MB,
    NORMAL_LAYER_PROGRESS_INTERVAL_SECS, PusherError, RATE_LIMIT_DELAY_MS, cache, image,
    oci::{auth::RegistryAuth, client::Client, manifest::OciImageManifest, reference::Reference},
    state,
    tar_import::{
        TarRepoInfo, build_target_from_tar, import_tar_file, infer_target_from_history,
        tar_repo_info_from_path,
    },
};

/// Entry point for the push command. Handles tar imports, target inference, credential
/// resolution, and delegates to the low-level upload helpers.
pub async fn run_push(
    client: &Client,
    input: &str,
    target_image: Option<String>,
    username: Option<String>,
    password: Option<String>,
    registry_override: Option<String>,
) -> Result<(), PusherError> {
    let input_path = Path::new(input);
    let is_tar_source = input_path.is_file();
    let mut inferred_target = target_image;
    let mut tar_info: Option<TarRepoInfo> = None;

    if is_tar_source {
        println!("📦 Preparing to push Docker archive: {}", input);
        let info = tar_repo_info_from_path(input)?;
        tar_info = Some(info);
        let info = tar_info.as_ref().unwrap();
        if inferred_target.is_none() {
            if let Some(history_target) = infer_target_from_history(info).await? {
                inferred_target = Some(history_target);
            } else {
                let fallback = build_target_from_tar(info, registry_override.as_deref());
                println!(
                    "💡 No recent history matched. Using target derived from tar metadata: {}",
                    fallback
                );
                inferred_target = Some(fallback);
            }
        }
    } else {
        println!("📤 Pushing cached image: {}", input);
    }

    if inferred_target.is_none() {
        return Err(PusherError::PushError(
            "Unable to determine target image. Provide --target or ensure the tar archive contains RepoTags.".to_string(),
        ));
    }

    let mut final_target = inferred_target.unwrap();
    if let Some(info) = tar_info.as_ref() {
        final_target = ensure_target_from_tar_metadata(final_target, info).await?;
    }
    println!("🎯 Target image resolved as: {}", final_target);

    let target_ref: Reference = final_target
        .parse()
        .map_err(|e| PusherError::PushError(format!("Invalid target image reference: {}", e)))?;

    let explicit_credentials_requested = username.is_some() || password.is_some();
    let (resolved_username, resolved_password) =
        match resolve_credentials(username, password, target_ref.registry_host()).await {
            Ok(creds) => creds,
            Err(err) => {
                if !explicit_credentials_requested
                    && is_missing_credentials_error(&err)
                    && tar_info.is_some()
                {
                    if let Some((replacement_target, creds)) =
                        attempt_registry_inference(tar_info.as_ref().unwrap()).await?
                    {
                        if replacement_target != final_target {
                            println!("🔁 Switching to inferred target: {}", replacement_target);
                        }
                        final_target = replacement_target;
                        creds
                    } else {
                        return Err(err);
                    }
                } else {
                    return Err(err);
                }
            }
        };

    let source_cache_key = if is_tar_source {
        let cache_name = image::sanitize_image_name(&final_target);
        println!("🧩 Importing tar archive into cache entry: {}", cache_name);
        import_tar_file(input, &cache_name).await?;
        cache_name
    } else {
        if !cache::has_cached_image(input).await? {
            println!("⚠️  Image not found in cache, pulling first...");
            cache::cache_image(client, input).await?;
        }
        input.to_string()
    };

    push_cached_image(
        client,
        &source_cache_key,
        &final_target,
        &resolved_username,
        &resolved_password,
    )
    .await?;
    state::record_push_target(&final_target).await?;
    println!("✅ Successfully pushed image: {}", final_target);
    Ok(())
}

async fn push_cached_image(
    client: &Client,
    source_image: &str,
    target_image: &str,
    username: &str,
    password: &str,
) -> Result<(), PusherError> {
    let cache_dir = Path::new(CACHE_DIR);
    let image_cache_dir = cache_dir.join(image::sanitize_image_name(source_image));
    let auth = RegistryAuth::basic(username, password);
    let target_ref: Reference = target_image
        .parse()
        .map_err(|e| PusherError::PushError(format!("Invalid target image reference: {}", e)))?;

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
        .map_err(|e| PusherError::CacheError(format!("Failed to read cached manifest: {}", e)))?;
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

        if blob_exists_in_registry(client, &target_ref, &auth, digest).await? {
            println!(
                "   ✅ Layer already exists in registry, skipping upload: {}",
                digest
            );
            uploaded_layers.push(digest.clone());
            skipped_uploads += 1;
            continue;
        }

        if layer_size_bytes >= LARGE_LAYER_THRESHOLD_BYTES {
            upload_large_layer(
                client,
                &target_ref,
                &auth,
                &layer_path,
                digest,
                layer_size_mb,
                layer_size_bytes,
            )
            .await?;
        } else {
            upload_small_layer(
                client,
                &target_ref,
                &auth,
                &layer_path,
                digest,
                layer_size_mb,
            )
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

    client
        .push_blob(&target_ref, &auth, &config_data, config_digest)
        .await
        .map_err(|e| PusherError::PushError(format!("Failed to upload config: {}", e)))?;

    println!("📋 Pushing manifest to registry: {}", target_image);
    let manifest_url = client
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
    client: &Client,
    target_ref: &Reference,
    auth: &RegistryAuth,
    digest: &str,
) -> Result<bool, PusherError> {
    match client.blob_exists(target_ref, digest, auth).await {
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

fn format_size_display(size_mb: f64) -> (f64, &'static str) {
    if size_mb > 1024.0 {
        (size_mb / 1024.0, "GB")
    } else {
        (size_mb, "MB")
    }
}

fn calculate_upload_progress(elapsed_secs: u64, layer_size_mb: f64) -> f64 {
    if elapsed_secs > 10 {
        let time_factor = elapsed_secs as f64 / (layer_size_mb / 8.0);
        ((time_factor / (1.0 + time_factor)) * 100.0).min(95.0)
    } else {
        10.0
    }
}

fn create_progress_tracker(
    layer_size_mb: f64,
    layer_size_bytes: u64,
    network_start: Instant,
    digest: &str,
) -> Option<task::JoinHandle<()>> {
    if layer_size_mb <= LARGE_LAYER_THRESHOLD_MB {
        return None;
    }

    let layer_size_mb_clone = layer_size_mb;
    let network_start_clone = network_start;
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

            if elapsed.as_secs() > 0 {
                let elapsed_min = elapsed.as_secs_f64() / 60.0;
                let estimated_progress_percent =
                    calculate_upload_progress(elapsed.as_secs(), layer_size_mb_clone);

                let estimated_transferred_mb =
                    (estimated_progress_percent / 100.0) * layer_size_mb_clone;
                let estimated_remaining_mb = layer_size_mb_clone - estimated_transferred_mb;
                let estimated_transferred_bytes =
                    (estimated_progress_percent / 100.0) * layer_size_bytes as f64;

                let current_speed_mbps = if elapsed.as_secs() > 5 {
                    estimated_transferred_mb / elapsed.as_secs_f64()
                } else {
                    ESTIMATED_SPEED_MBPS
                };

                let remaining_time_min = if current_speed_mbps > 0.0 {
                    estimated_remaining_mb / current_speed_mbps / 60.0
                } else {
                    0.0
                };

                let (transferred_display, unit) = format_size_display(estimated_transferred_mb);
                let (total_display, _) = format_size_display(layer_size_mb_clone);

                println!(
                    "   ⏳ Upload progress #{}: {:.1}% | {:.1}/{:.1} {} | Speed: ~{:.1} MB/s | ETA: {:.1}min",
                    progress_counter,
                    estimated_progress_percent,
                    transferred_display,
                    total_display,
                    unit,
                    current_speed_mbps,
                    remaining_time_min
                );

                if progress_counter % 2 == 0 {
                    println!(
                        "   📊 Data transferred: {:.0}/{} bytes | Elapsed: {:.1}min | Layer: ...{}",
                        estimated_transferred_bytes, layer_size_bytes, elapsed_min, digest_suffix
                    );
                }

                if progress_counter % 3 == 0 && layer_size_mb_clone > 1000.0 {
                    let gb_size = layer_size_mb_clone / 1024.0;
                    let avg_speed = estimated_transferred_mb / elapsed.as_secs_f64();
                    let completion_percent =
                        ((estimated_transferred_mb / layer_size_mb_clone) * 100.0).min(95.0);

                    println!(
                        "   📈 Network: {:.2} GB total | Avg: {:.1} MB/s | Progress: {:.1}% | Large transfer in progress",
                        gb_size, avg_speed, completion_percent
                    );
                }

                progress_counter += 1;
            }
        }
    }))
}

async fn upload_large_layer(
    client: &Client,
    target_ref: &Reference,
    auth: &RegistryAuth,
    layer_path: &Path,
    digest: &str,
    layer_size_mb: f64,
    layer_size_bytes: u64,
) -> Result<(), PusherError> {
    println!(
        "   🔄 Chunk-streaming large layer ({:.1} MB) in {:.0} MB chunks...",
        layer_size_mb,
        CHUNKED_LAYER_SIZE_BYTES as f64 / (1024.0 * 1024.0)
    );

    let upload_start = Instant::now();
    let file = tokio::fs::File::open(layer_path).await.map_err(|e| {
        PusherError::CacheError(format!("Failed to open cached layer {}: {}", digest, e))
    })?;
    let mut reader = BufReader::with_capacity(CHUNKED_LAYER_SIZE_BYTES, file);

    if layer_size_mb > 1000.0 {
        let estimated_time_min = layer_size_mb / ESTIMATED_SPEED_MBPS / 60.0;
        println!(
            "   ⏱️  Estimated upload time: {:.1}-{:.1} minutes",
            estimated_time_min * 0.5,
            estimated_time_min * 2.0
        );
    }

    let network_start = Instant::now();
    let progress_handle =
        create_progress_tracker(layer_size_mb, layer_size_bytes, network_start, digest);

    let upload_result = client
        .push_blob_stream(
            target_ref,
            auth,
            &mut reader,
            digest,
            CHUNKED_LAYER_SIZE_BYTES,
        )
        .await;

    if let Some(handle) = progress_handle {
        handle.abort();
    }

    upload_result
        .map_err(|e| PusherError::PushError(format!("Failed to upload layer {}: {}", digest, e)))?;

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
    client: &Client,
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

    client
        .push_blob(target_ref, auth, &layer_data, digest)
        .await
        .map_err(|e| PusherError::PushError(format!("Failed to upload layer {}: {}", digest, e)))?;

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
    let reference: Reference = target
        .parse()
        .map_err(|e| PusherError::PushError(format!("Invalid target image reference: {}", e)))?;

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

    confirm_or_wait_for_target(
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
        confirm_or_wait_for_target(
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
    if prompt_yes_no(&question).await? {
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

async fn resolve_credentials(
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
