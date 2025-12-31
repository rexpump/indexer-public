//! Synchronization state management

/// Tracks indexing progress and manages batch sizes
#[derive(Debug)]
pub struct SyncState {
    pub start_block: u64,
    pub end_block: u64,
    pub current_block: u64,
    pub batch_size: u64,
    min_batch_size: u64,
}

impl SyncState {
    pub fn new(start_block: u64, end_block: u64, batch_size: u64) -> Self {
        Self {
            start_block,
            end_block,
            current_block: start_block,
            batch_size,
            min_batch_size: 10,
        }
    }

    /// Get the next batch range to process
    pub fn next_batch(&self) -> (u64, u64) {
        let from = self.current_block;
        let to = std::cmp::min(self.current_block + self.batch_size - 1, self.end_block);
        (from, to)
    }

    /// Update progress after successful batch processing
    pub fn update_progress(&mut self, processed_to: u64) {
        self.current_block = processed_to + 1;
    }

    /// Check if sync is complete
    pub fn is_complete(&self) -> bool {
        self.current_block > self.end_block
    }

    /// Calculate progress percentage
    pub fn progress_percent(&self) -> f64 {
        if self.end_block == self.start_block {
            return 100.0;
        }
        let processed = self.current_block.saturating_sub(self.start_block) as f64;
        let total = (self.end_block - self.start_block) as f64;
        (processed / total) * 100.0
    }

    /// Reduce batch size (for handling errors)
    pub fn reduce_batch_size(&mut self) {
        self.batch_size = std::cmp::max(self.batch_size / 2, self.min_batch_size);
    }

    /// Increase batch size (for optimization)
    pub fn increase_batch_size(&mut self, max: u64) {
        self.batch_size = std::cmp::min(self.batch_size * 2, max);
    }

    /// Blocks remaining to sync
    pub fn blocks_remaining(&self) -> u64 {
        self.end_block.saturating_sub(self.current_block)
    }
}

