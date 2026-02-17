//! Debug output utilities for kv.
//!
//! When debug mode is enabled (via -D flag or KV_DEBUG=1 environment variable),
//! these functions print diagnostic information to stderr. This is useful for
//! troubleshooting issues on new/unusual hardware.
//!
//! Note: In no_std builds, debug output is simplified to avoid format! overhead.

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};

/// Global debug mode flag, set once at startup.
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Enable debug mode globally. Called once from main after parsing args.
pub fn set_enabled(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Check if debug mode is enabled.
#[inline]
pub fn is_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Print a debug message to stderr if debug mode is enabled.
/// In no_std, this is simplified to avoid format! overhead.
#[macro_export]
macro_rules! dbg_print {
    ($($arg:tt)*) => {
        // Disabled in no_std to avoid format! overhead
        // Could be re-enabled with manual string building if needed
    };
}

/// Print a debug message about file access.
#[macro_export]
macro_rules! dbg_read {
    ($path:expr) => {
        // Disabled in no_std
    };
}

/// Print a debug message about file read failure.
#[macro_export]
macro_rules! dbg_fail {
    ($path:expr, $err:expr) => {
        // Disabled in no_std
    };
}

/// Print a debug message about parse errors.
#[macro_export]
macro_rules! dbg_parse {
    ($context:expr, $err:expr) => {
        // Disabled in no_std
    };
}

/// Print a debug message about directory scan.
#[macro_export]
macro_rules! dbg_scan {
    ($path:expr, $count:expr) => {
        // Disabled in no_std
    };
}
