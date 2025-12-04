//! Debug output utilities for kv.
//!
//! When debug mode is enabled (via -D flag or KV_DEBUG=1 environment variable),
//! these functions print diagnostic information to stderr. This is useful for
//! troubleshooting issues on new/unusual hardware.

use std::sync::atomic::{AtomicBool, Ordering};

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
/// Format: [DEBUG] message
#[macro_export]
macro_rules! dbg_print {
    ($($arg:tt)*) => {
        if $crate::debug::is_enabled() {
            eprintln!("[DEBUG] {}", format!($($arg)*));
        }
    };
}

/// Print a debug message about file access.
/// Format: [DEBUG] READ path
#[macro_export]
macro_rules! dbg_read {
    ($path:expr) => {
        if $crate::debug::is_enabled() {
            eprintln!("[DEBUG] READ {}", $path);
        }
    };
}

/// Print a debug message about file read failure.
/// Format: [DEBUG] FAIL path: error
#[macro_export]
macro_rules! dbg_fail {
    ($path:expr, $err:expr) => {
        if $crate::debug::is_enabled() {
            eprintln!("[DEBUG] FAIL {}: {}", $path, $err);
        }
    };
}

/// Print a debug message about parse errors.
/// Format: [DEBUG] PARSE context: error
#[macro_export]
macro_rules! dbg_parse {
    ($context:expr, $err:expr) => {
        if $crate::debug::is_enabled() {
            eprintln!("[DEBUG] PARSE {}: {}", $context, $err);
        }
    };
}

/// Print a debug message about directory scan.
/// Format: [DEBUG] SCAN path (count entries)
#[macro_export]
macro_rules! dbg_scan {
    ($path:expr, $count:expr) => {
        if $crate::debug::is_enabled() {
            eprintln!("[DEBUG] SCAN {} ({} entries)", $path, $count);
        }
    };
}
