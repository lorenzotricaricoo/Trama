//! Memory use of a transcription. The fp32 model peaks at about 2.8 GB (spike #10), so
//! every transcription measures and reports its peak.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// Samples the resident memory of the process on a background thread and keeps the
/// highest value seen, until stopped.
#[derive(Debug)]
pub struct MemoryMonitor {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl MemoryMonitor {
    /// Starts sampling every `interval`.
    pub fn start(interval: Duration) -> Self {
        let peak = Arc::new(AtomicU64::new(resident_bytes().unwrap_or(0)));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (peak, stop) = (Arc::clone(&peak), Arc::clone(&stop));
            std::thread::Builder::new()
                .name("trama-memory".into())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        if let Some(now) = resident_bytes() {
                            peak.fetch_max(now, Ordering::Relaxed);
                        }
                        std::thread::park_timeout(interval);
                    }
                })
                .ok()
        };
        Self { peak, stop, worker }
    }

    /// Highest resident memory seen so far, in bytes; `None` where it can't be measured.
    pub fn peak_bytes(&self) -> Option<u64> {
        if let Some(now) = resident_bytes() {
            self.peak.fetch_max(now, Ordering::Relaxed);
        }
        Some(self.peak.load(Ordering::Relaxed)).filter(|&peak| peak > 0)
    }

    /// Stops sampling and returns the peak.
    pub fn finish(mut self) -> Option<u64> {
        let peak = self.peak_bytes();
        self.shutdown();
        peak
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

impl Drop for MemoryMonitor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Resident memory of this process, in bytes.
pub fn resident_bytes() -> Option<u64> {
    memory_stats::memory_stats().map(|stats| stats.physical_mem as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_peak_includes_memory_used_meanwhile() {
        let monitor = MemoryMonitor::start(Duration::from_millis(5));
        assert!(
            monitor.peak_bytes().is_some(),
            "measurable on this platform"
        );
        let block = vec![1u8; 64 * 1024 * 1024];
        std::thread::sleep(Duration::from_millis(50));
        let peak = monitor.finish().unwrap();
        assert!(block.iter().all(|&b| b == 1));
        assert!(peak >= block.len() as u64, "peak {peak}");
    }
}
