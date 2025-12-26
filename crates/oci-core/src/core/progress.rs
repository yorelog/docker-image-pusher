use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct LayerProgressInit {
    pub digest: String,
    pub total_bytes: u64,
    pub chunk_size_bytes: usize,
    pub estimated_chunks: u64,
}

#[derive(Debug, Clone)]
pub struct LayerProgressUpdate {
    pub digest: String,
    pub sent_bytes: u64,
    pub total_bytes: u64,
    pub elapsed: Duration,
    pub speed_mbps: f64,
    pub eta_seconds: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct LayerProgressComplete {
    pub digest: String,
    pub total_bytes: u64,
    pub elapsed: Duration,
    pub average_mbps: f64,
}

#[derive(Debug, Clone)]
pub struct ChunkTransferEvent {
    pub digest: String,
    pub chunk_index: u64,
    pub chunk_bytes: usize,
    pub total_transferred: u64,
    pub total_bytes: Option<u64>,
}

pub trait ProgressReporter: Send + Sync + 'static {
    fn on_layer_start(&self, _info: LayerProgressInit) {}
    fn on_layer_progress(&self, _update: LayerProgressUpdate) {}
    fn on_layer_complete(&self, _summary: LayerProgressComplete) {}
    fn on_chunk_transferred(&self, _chunk: ChunkTransferEvent) {}
}

pub type ProgressReporterHandle = Arc<dyn ProgressReporter>;
