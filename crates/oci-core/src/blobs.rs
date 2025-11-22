use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use futures::{future::BoxFuture, stream::FuturesUnordered, StreamExt};
use tokio::task;
use tokio::fs::File;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};

const MAX_STREAM_RETRIES: usize = 2;
const MAX_AUTO_CHUNK_BYTES: usize = 256 * 1024 * 1024;

use crate::auth::RegistryAuth;
use crate::client::Client;
use crate::errors::OciError;
use crate::progress::{LayerProgressComplete, LayerProgressInit, ProgressReporterHandle};
use crate::reference::Reference;

/// Metadata describing a locally extracted layer on disk.
#[derive(Debug, Clone)]
pub struct LocalLayer {
	pub digest: String,
	pub media_type: String,
	pub size: u64,
	pub path: PathBuf,
}

impl LocalLayer {
	pub fn size_mb(&self) -> f64 {
		self.size as f64 / (1024.0 * 1024.0)
	}
}

/// Controls how uploads are scheduled and how progress is reported.
pub struct LayerUploadOptions {
	pub chunk_size_bytes: usize,
	pub large_layer_threshold_bytes: u64,
	pub medium_layer_threshold_mb: f64,
	pub rate_limit_delay_ms: u64,
	pub concurrency: usize,
	pub progress: ProgressOptions,
	pub progress_reporter: Option<ProgressReporterHandle>,
}

impl Clone for LayerUploadOptions {
	fn clone(&self) -> Self {
		Self {
			chunk_size_bytes: self.chunk_size_bytes,
			large_layer_threshold_bytes: self.large_layer_threshold_bytes,
			medium_layer_threshold_mb: self.medium_layer_threshold_mb,
			rate_limit_delay_ms: self.rate_limit_delay_ms,
			concurrency: self.concurrency,
			progress: self.progress.clone(),
			progress_reporter: self.progress_reporter.clone(),
		}
	}
}

impl fmt::Debug for LayerUploadOptions {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("LayerUploadOptions")
			.field("chunk_size_bytes", &self.chunk_size_bytes)
			.field("large_layer_threshold_bytes", &self.large_layer_threshold_bytes)
			.field("medium_layer_threshold_mb", &self.medium_layer_threshold_mb)
			.field("rate_limit_delay_ms", &self.rate_limit_delay_ms)
			.field("concurrency", &self.concurrency)
			.field("progress", &self.progress)
			.field("progress_reporter", &self.progress_reporter.is_some())
			.finish()
	}
}

impl LayerUploadOptions {
	pub fn normalized(mut self) -> Self {
		if self.chunk_size_bytes == 0 {
			self.chunk_size_bytes = 1;
		}
		if self.concurrency == 0 {
			self.concurrency = 1;
		}
		self
	}
}

/// Fine-grained switches for progress telemetry.
#[derive(Debug, Clone)]
pub struct ProgressOptions {
	pub large_layer_threshold_mb: f64,
	pub large_interval_secs: u64,
	pub normal_interval_secs: u64,
	pub estimated_speed_mbps: f64,
}

/// Aggregate outcome once all layer uploads finish.
#[derive(Debug, Default)]
pub struct UploadSummary {
	pub uploaded: Vec<String>,
	pub skipped: usize,
}

struct LayerUploadOutcome {
	digest: String,
	skipped: bool,
}

/// Coordinates per-layer uploads with bounded concurrency.
pub struct LayerUploadPool {
	client: Client,
	reference: Reference,
	auth: Arc<RegistryAuth>,
	options: LayerUploadOptions,
}

impl LayerUploadPool {
	pub fn new(
		client: &Client,
		reference: &Reference,
		auth: Arc<RegistryAuth>,
		options: LayerUploadOptions,
	) -> Self {
		Self {
			client: client.clone(),
			reference: reference.clone(),
			auth,
			options: options.normalized(),
		}
	}

	pub async fn upload_stream(
		&self,
		mut layers: mpsc::Receiver<LocalLayer>,
	) -> Result<UploadSummary, OciError> {
		let mut in_flight: FuturesUnordered<_> = FuturesUnordered::new();
		let mut summary = UploadSummary::default();
		let mut closed = false;

		loop {
			while in_flight.len() < self.options.concurrency && !closed {
				match layers.recv().await {
					Some(layer) => in_flight.push(self.upload_layer_task(layer)),
					None => closed = true,
				}
			}

			match in_flight.next().await {
				Some(result) => {
					let outcome = result?;
					if outcome.skipped {
						summary.skipped += 1;
					} else {
						summary.uploaded.push(outcome.digest);
					}
				}
				None => {
					if closed {
						break;
					}
				}
			}
		}

		Ok(summary)
	}

	fn upload_layer_task(&self, layer: LocalLayer) -> BoxFuture<'static, Result<LayerUploadOutcome, OciError>> {
		let client = self.client.clone();
		let reference = self.reference.clone();
		let auth = Arc::clone(&self.auth);
		let options = self.options.clone();
		Box::pin(async move { upload_single_layer(client, reference, auth, options, layer).await })
	}
}

async fn upload_single_layer(
	client: Client,
	reference: Reference,
	auth: Arc<RegistryAuth>,
	options: LayerUploadOptions,
	layer: LocalLayer,
) -> Result<LayerUploadOutcome, OciError> {
	let layer_size_mb = layer.size_mb();
	println!("📦 Uploading layer {} ({:.1} MB)", layer.digest, layer_size_mb);

	if blob_exists(&client, &reference, auth.as_ref(), &layer).await? {
		println!(
			"   ✅ Layer already exists in registry, skipping upload: {}",
			layer.digest
		);
		return Ok(LayerUploadOutcome {
			digest: layer.digest,
			skipped: true,
		});
	}

	if layer.size >= options.large_layer_threshold_bytes {
		upload_large_layer(&client, &reference, auth.as_ref(), &layer, &options).await?;
	} else {
		upload_small_layer(&client, &reference, auth.as_ref(), &layer).await?;
	}

	println!("   ✅ Successfully uploaded layer {}", layer.digest);

	if layer_size_mb > options.medium_layer_threshold_mb && options.rate_limit_delay_ms > 0 {
		task::yield_now().await;
	}

	Ok(LayerUploadOutcome {
		digest: layer.digest,
		skipped: false,
	})
}

async fn blob_exists(
	client: &Client,
	reference: &Reference,
	auth: &RegistryAuth,
	layer: &LocalLayer,
) -> Result<bool, OciError> {
	match client.blob_exists(reference, &layer.digest, auth).await {
		Ok(exists) => Ok(exists),
		Err(err) => {
			println!(
				"   ⚠️  Unable to check blob {} presence in registry (continuing with upload): {}",
				layer.digest, err
			);
			Ok(false)
		}
	}
}

async fn upload_small_layer(
	client: &Client,
	reference: &Reference,
	auth: &RegistryAuth,
	layer: &LocalLayer,
) -> Result<(), OciError> {
	println!("   📤 Uploading layer directly...");

	let read_start = Instant::now();
	let layer_data = tokio::fs::read(&layer.path).await?;
	let read_duration = read_start.elapsed();
	let upload_start = Instant::now();

	client
		.push_blob(reference, auth, &layer_data, &layer.digest)
		.await?;

	let upload_duration = upload_start.elapsed();
	let total_duration = read_start.elapsed();
	let speed = if total_duration.as_secs() > 0 {
		layer.size_mb() / total_duration.as_secs_f64()
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

async fn upload_large_layer(
	client: &Client,
	reference: &Reference,
	auth: &RegistryAuth,
	layer: &LocalLayer,
	options: &LayerUploadOptions,
) -> Result<(), OciError> {
	let layer_size_mb = layer.size_mb();
	let chunk_size_mb = options.chunk_size_bytes as f64 / (1024.0 * 1024.0);
	println!(
		"   🔄 Chunk-streaming large layer ({:.1} MB) in {:.0} MB chunks (auto-adjusting)...",
		layer_size_mb, chunk_size_mb
	);
	let estimated_chunks = ((layer.size as f64) / options.chunk_size_bytes as f64)
		.ceil()
		.max(1.0) as u64;
	if let Some(reporter) = options.progress_reporter.as_ref() {
		reporter.on_layer_start(LayerProgressInit {
			digest: layer.digest.clone(),
			total_bytes: layer.size,
			chunk_size_bytes: options.chunk_size_bytes,
			estimated_chunks,
		});
	} else {
		println!(
			"   🧮 Planning roughly {} chunk(s) (~{:.1} MB each)",
			estimated_chunks,
			chunk_size_mb
		);
	}

	let upload_start = Instant::now();

	if layer_size_mb > 1000.0 {
		let estimated_time_min = layer_size_mb / options.progress.estimated_speed_mbps / 60.0;
		println!(
			"   ⏱️  Estimated upload time: {:.1}-{:.1} minutes",
			estimated_time_min * 0.5,
			estimated_time_min * 2.0
		);
	}

	let network_start = Instant::now();
	let bytes_sent = Arc::new(AtomicU64::new(0));
	let reporter_active = options.progress_reporter.is_some();
	let progress_handle = create_progress_tracker(
		layer_size_mb,
		layer.size,
		network_start,
		layer.digest.clone(),
		Arc::clone(&bytes_sent),
		&options.progress,
		reporter_active,
	);
	let progress_counter = if reporter_active {
		None
	} else {
		Some(Arc::clone(&bytes_sent))
	};

	let mut adaptive_chunk_bytes = options.chunk_size_bytes;
	let mut attempt = 0;
	loop {
		if let Some(counter) = progress_counter.as_ref() {
			counter.store(0, Ordering::Relaxed);
		}
		let mut file = File::open(&layer.path).await?;
		if attempt > 0 {
			println!(
				"   🔁 Upload session reset by registry; restarting stream (attempt {}/{})",
				attempt + 1,
				MAX_STREAM_RETRIES + 1
			);
		}
		match client
			.push_blob_stream(
				reference,
				auth,
				&mut file,
				&layer.digest,
				adaptive_chunk_bytes,
				Some(layer.size),
				options.progress_reporter.clone(),
				progress_counter.as_ref().map(Arc::clone),
			)
			.await
		{
			Ok(_) => break,
			Err(OciError::UploadReset(reason)) => {
				if attempt >= MAX_STREAM_RETRIES {
					if let Some(handle) = &progress_handle {
						handle.abort();
					}
					return Err(OciError::UploadReset(format!(
						"{} (after {} retries)",
						reason,
						MAX_STREAM_RETRIES + 1
					)));
				}
				attempt += 1;
				let trimmed = reason.chars().take(160).collect::<String>();
				println!(
					"   ⚠️  Registry invalidated upload session: {}",
					trimmed
				);
				let new_size = bump_chunk_size(adaptive_chunk_bytes);
				if new_size != adaptive_chunk_bytes {
					adaptive_chunk_bytes = new_size;
					println!(
						"   📏 Increasing chunk size to {:.0} MB to reduce PATCH count",
						adaptive_chunk_bytes as f64 / (1024.0 * 1024.0)
					);
				}
				continue;
			}
			Err(err) => {
				if let Some(handle) = &progress_handle {
					handle.abort();
				}
				return Err(err);
			}
		}
	}

	if let Some(handle) = &progress_handle {
		handle.abort();
	}

	let network_duration = network_start.elapsed();
	let total_duration = upload_start.elapsed();
	let upload_speed = if network_duration.as_secs() > 0 {
		(layer.size as f64 / (1024.0 * 1024.0)) / network_duration.as_secs_f64()
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

	if let Some(reporter) = options.progress_reporter.as_ref() {
		reporter.on_layer_complete(LayerProgressComplete {
			digest: layer.digest.clone(),
			total_bytes: layer.size,
			elapsed: total_duration,
			average_mbps: upload_speed,
		});
	}

	Ok(())
}

fn create_progress_tracker(
	layer_size_mb: f64,
	layer_size_bytes: u64,
	network_start: Instant,
	digest: String,
	bytes_sent: Arc<AtomicU64>,
	progress: &ProgressOptions,
	reporter_active: bool,
) -> Option<task::JoinHandle<()>> {
	if reporter_active || layer_size_mb <= progress.large_layer_threshold_mb {
		return None;
	}

	let interval_secs = if layer_size_mb > 1000.0 {
		progress.large_interval_secs
	} else {
		progress.normal_interval_secs
	};
	let label = short_digest(&digest);

	Some(tokio::spawn(async move {
		let mut interval = time::interval(Duration::from_secs(interval_secs));
		loop {
			interval.tick().await;
			let elapsed = network_start.elapsed();
			let sent_bytes = bytes_sent.load(Ordering::Relaxed);
			if sent_bytes == 0 && elapsed.as_secs() < 5 {
				continue;
			}

			let sent_mb = sent_bytes as f64 / (1024.0 * 1024.0);
			let percent = if layer_size_bytes > 0 {
				(sent_bytes as f64 / layer_size_bytes as f64 * 100.0).min(100.0)
			} else {
				0.0
			};
			let speed_mbps = if elapsed.as_secs() > 0 {
				sent_mb / elapsed.as_secs_f64()
			} else {
				0.0
			};
			let remaining_mb = (layer_size_mb - sent_mb).max(0.0);
			let eta_display = if speed_mbps > 0.0 {
				format_eta(remaining_mb / speed_mbps)
			} else {
				"--".to_string()
			};

			println!(
				"   {} uploading {:.1}% ({:.1}/{:.1} MB) @ {:.1} MB/s ETA {}",
				label,
				percent,
				sent_mb,
				layer_size_mb,
				speed_mbps,
				eta_display
			);
		}
	}))
}

fn bump_chunk_size(current: usize) -> usize {
	if current >= MAX_AUTO_CHUNK_BYTES {
		current
	} else {
		current
			.saturating_mul(2)
			.min(MAX_AUTO_CHUNK_BYTES)
	}
}

fn short_digest(digest: &str) -> String {
	let short = digest.split(':').last().unwrap_or(digest);
	let label: String = short.chars().take(12).collect();
	if label.is_empty() {
		digest.to_string()
	} else {
		label
	}
}

fn format_eta(seconds: f64) -> String {
	if !seconds.is_finite() {
		return "--".to_string();
	}
	let total = seconds.max(0.0).round() as u64;
	let minutes = total / 60;
	let secs = total % 60;
	if minutes == 0 {
		format!("{}s", secs)
	} else {
		format!("{}m{:02}s", minutes, secs)
	}
}

