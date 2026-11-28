//! Helpers shared by the integration tests.

#![allow(dead_code)] // Each test file uses a different subset.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use tempfile::TempDir;
use trama_core::storage::{Clock, Library, Timestamp};

pub const MINUTE: i64 = 60 * 1000;
pub const DAY: i64 = 24 * 60 * MINUTE;

/// A clock the test moves by hand.
#[derive(Clone)]
pub struct ManualClock(Arc<AtomicI64>);

impl ManualClock {
    pub const START: Timestamp = 1_800_000_000_000;

    pub fn new() -> Self {
        Self(Arc::new(AtomicI64::new(Self::START)))
    }

    pub fn advance(&self, ms: i64) {
        self.0.fetch_add(ms, Ordering::SeqCst);
    }

    pub fn now(&self) -> Timestamp {
        self.0.load(Ordering::SeqCst)
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Timestamp {
        ManualClock::now(self)
    }
}

/// A new library in a temporary folder, with a manual clock.
pub fn library() -> (TempDir, ManualClock, Library) {
    let dir = TempDir::new().unwrap();
    let clock = ManualClock::new();
    let library = Library::open_with_clock(dir.path(), Box::new(clock.clone())).unwrap();
    (dir, clock, library)
}

pub mod http;
