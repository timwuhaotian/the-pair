//! Test-only helpers for serializing process-wide environment mutations.
//!
//! `cargo test` runs every test in a single process across many threads, so any
//! test that pokes `std::env` (HOME, PATH, APPDATA, ...) is mutating shared
//! state. Tests in different modules must therefore share *one* lock — a
//! per-module lock only serializes its own module and still races everyone
//! else.
//!
//! The guard is poison-tolerant on purpose: when one env test fails its
//! assertion the mutex would otherwise stay poisoned and turn a single genuine
//! failure into a cascade of unrelated `PoisonError` failures.

use std::sync::{Mutex, MutexGuard};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Acquire exclusive access to the process environment for the current test.
///
/// Hold the returned guard for as long as the test relies on the env vars it
/// set, and restore the originals before asserting so a failure never leaks
/// state into the next test.
pub(crate) fn lock_env() -> MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
