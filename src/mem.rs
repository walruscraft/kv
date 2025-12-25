//! Memory information from /proc/meminfo.
//!
//! This is about as simple as it gets - parse some key=value pairs from
//! /proc/meminfo and spit them out. The kernel does the heavy lifting,
//! we just read and format.
//!
//! Surprising fact (at least it was to me): /proc/meminfo has been around
//! since Linux 1.0 and the format hasn't changed much. Backwards compatibility
//! is a beautiful and rare thing in this world of change.

use crate::cli::GlobalOptions;
use crate::fields::{mem as f, to_text_key};
use crate::io;
use crate::json::{begin_kv_output, JsonWriter};
use std::collections::HashMap;
use std::path::Path;

/// Path to meminfo. Could be different in containers or chroots,
/// but let's not overthink it for now, will be testing and failing later.
const MEMINFO_PATH: &str = "/proc/meminfo";

/// Memory information structure.
/// All values in KB, because that's what the kernel gives us.
#[derive(Debug, Default)]
pub struct MemInfo {
    pub mem_total_kb: Option<u64>,
    pub mem_free_kb: Option<u64>,
    pub mem_available_kb: Option<u64>,
    pub buffers_kb: Option<u64>,
    pub cached_kb: Option<u64>,
    pub swap_total_kb: Option<u64>,
    pub swap_free_kb: Option<u64>,
    pub swap_cached_kb: Option<u64>,
    pub shmem_kb: Option<u64>,
    pub sreclaimable_kb: Option<u64>,
    pub sunreclaim_kb: Option<u64>,
    pub dirty_kb: Option<u64>,
    pub writeback_kb: Option<u64>,
}

impl MemInfo {
    /// Parse /proc/meminfo into a MemInfo struct.
    ///
    /// Format is simple: "FieldName:        12345 kB"
    /// We strip the "kB" suffix and parse the number.
    pub fn read() -> Option<Self> {
        Self::read_from(Path::new(MEMINFO_PATH))
    }

    /// Read from a custom path (useful for testing).
    pub fn read_from(path: &Path) -> Option<Self> {
        let contents = io::read_file_string(path)?;
        Some(Self::parse(&contents))
    }

    /// Parse meminfo content into struct.
    pub fn parse(content: &str) -> Self {
        let mut map = HashMap::new();

        for line in content.lines() {
            if let Some((key, value)) = parse_meminfo_line(line) {
                map.insert(key, value);
            }
        }

        MemInfo {
            mem_total_kb: map.get("MemTotal").copied(),
            mem_free_kb: map.get("MemFree").copied(),
            mem_available_kb: map.get("MemAvailable").copied(),
            buffers_kb: map.get("Buffers").copied(),
            cached_kb: map.get("Cached").copied(),
            swap_total_kb: map.get("SwapTotal").copied(),
            swap_free_kb: map.get("SwapFree").copied(),
            swap_cached_kb: map.get("SwapCached").copied(),
            shmem_kb: map.get("Shmem").copied(),
            sreclaimable_kb: map.get("SReclaimable").copied(),
            sunreclaim_kb: map.get("SUnreclaim").copied(),
            dirty_kb: map.get("Dirty").copied(),
            writeback_kb: map.get("Writeback").copied(),
        }
    }

    /// Output as text (KEY=VALUE format).
    pub fn print_text(&self, verbose: bool, human: bool) {
        let mut parts = Vec::new();

        if human {
            // Human-readable sizes like "16G", "512M"
            if let Some(v) = self.mem_total_kb {
                parts.push(format!("{}={}", to_text_key(f::MEM_TOTAL), io::format_kb_human(v)));
            }
            if let Some(v) = self.mem_free_kb {
                parts.push(format!("{}={}", to_text_key(f::MEM_FREE), io::format_kb_human(v)));
            }
            if let Some(v) = self.mem_available_kb {
                parts.push(format!("{}={}", to_text_key(f::MEM_AVAILABLE), io::format_kb_human(v)));
            }
            if let Some(v) = self.swap_total_kb {
                parts.push(format!("{}={}", to_text_key(f::SWAP_TOTAL), io::format_kb_human(v)));
            }
            if let Some(v) = self.swap_free_kb {
                parts.push(format!("{}={}", to_text_key(f::SWAP_FREE), io::format_kb_human(v)));
            }

            if verbose {
                if let Some(v) = self.buffers_kb {
                    parts.push(format!("{}={}", to_text_key(f::BUFFERS), io::format_kb_human(v)));
                }
                if let Some(v) = self.cached_kb {
                    parts.push(format!("{}={}", to_text_key(f::CACHED), io::format_kb_human(v)));
                }
                if let Some(v) = self.swap_cached_kb {
                    parts.push(format!("{}={}", to_text_key(f::SWAP_CACHED), io::format_kb_human(v)));
                }
                if let Some(v) = self.shmem_kb {
                    parts.push(format!("{}={}", to_text_key(f::SHMEM), io::format_kb_human(v)));
                }
                if let Some(v) = self.sreclaimable_kb {
                    parts.push(format!("{}={}", to_text_key(f::SRECLAIMABLE), io::format_kb_human(v)));
                }
                if let Some(v) = self.sunreclaim_kb {
                    parts.push(format!("{}={}", to_text_key(f::SUNRECLAIM), io::format_kb_human(v)));
                }
                if let Some(v) = self.dirty_kb {
                    parts.push(format!("{}={}", to_text_key(f::DIRTY), io::format_kb_human(v)));
                }
                if let Some(v) = self.writeback_kb {
                    parts.push(format!("{}={}", to_text_key(f::WRITEBACK), io::format_kb_human(v)));
                }
            }
        } else {
            // Raw KB values
            if let Some(v) = self.mem_total_kb {
                parts.push(format!("{}={}", to_text_key(f::MEM_TOTAL_KB), v));
            }
            if let Some(v) = self.mem_free_kb {
                parts.push(format!("{}={}", to_text_key(f::MEM_FREE_KB), v));
            }
            if let Some(v) = self.mem_available_kb {
                parts.push(format!("{}={}", to_text_key(f::MEM_AVAILABLE_KB), v));
            }
            if let Some(v) = self.swap_total_kb {
                parts.push(format!("{}={}", to_text_key(f::SWAP_TOTAL_KB), v));
            }
            if let Some(v) = self.swap_free_kb {
                parts.push(format!("{}={}", to_text_key(f::SWAP_FREE_KB), v));
            }

            if verbose {
                if let Some(v) = self.buffers_kb {
                    parts.push(format!("{}={}", to_text_key(f::BUFFERS_KB), v));
                }
                if let Some(v) = self.cached_kb {
                    parts.push(format!("{}={}", to_text_key(f::CACHED_KB), v));
                }
                if let Some(v) = self.swap_cached_kb {
                    parts.push(format!("{}={}", to_text_key(f::SWAP_CACHED_KB), v));
                }
                if let Some(v) = self.shmem_kb {
                    parts.push(format!("{}={}", to_text_key(f::SHMEM_KB), v));
                }
                if let Some(v) = self.sreclaimable_kb {
                    parts.push(format!("{}={}", to_text_key(f::SRECLAIMABLE_KB), v));
                }
                if let Some(v) = self.sunreclaim_kb {
                    parts.push(format!("{}={}", to_text_key(f::SUNRECLAIM_KB), v));
                }
                if let Some(v) = self.dirty_kb {
                    parts.push(format!("{}={}", to_text_key(f::DIRTY_KB), v));
                }
                if let Some(v) = self.writeback_kb {
                    parts.push(format!("{}={}", to_text_key(f::WRITEBACK_KB), v));
                }
            }
        }

        println!("{}", parts.join(" "));
    }

    /// Output as JSON.
    pub fn print_json(&self, pretty: bool, verbose: bool, human: bool) {
        let mut w = begin_kv_output(pretty, "mem");

        w.field_object("data");

        if human {
            // Human-readable string values
            if let Some(v) = self.mem_total_kb {
                w.field_str(f::MEM_TOTAL, &io::format_kb_human(v));
            }
            if let Some(v) = self.mem_free_kb {
                w.field_str(f::MEM_FREE, &io::format_kb_human(v));
            }
            if let Some(v) = self.mem_available_kb {
                w.field_str(f::MEM_AVAILABLE, &io::format_kb_human(v));
            }
            if let Some(v) = self.swap_total_kb {
                w.field_str(f::SWAP_TOTAL, &io::format_kb_human(v));
            }
            if let Some(v) = self.swap_free_kb {
                w.field_str(f::SWAP_FREE, &io::format_kb_human(v));
            }

            if verbose {
                if let Some(v) = self.buffers_kb {
                    w.field_str(f::BUFFERS, &io::format_kb_human(v));
                }
                if let Some(v) = self.cached_kb {
                    w.field_str(f::CACHED, &io::format_kb_human(v));
                }
                if let Some(v) = self.swap_cached_kb {
                    w.field_str(f::SWAP_CACHED, &io::format_kb_human(v));
                }
                if let Some(v) = self.shmem_kb {
                    w.field_str(f::SHMEM, &io::format_kb_human(v));
                }
                if let Some(v) = self.sreclaimable_kb {
                    w.field_str(f::SRECLAIMABLE, &io::format_kb_human(v));
                }
                if let Some(v) = self.sunreclaim_kb {
                    w.field_str(f::SUNRECLAIM, &io::format_kb_human(v));
                }
                if let Some(v) = self.dirty_kb {
                    w.field_str(f::DIRTY, &io::format_kb_human(v));
                }
                if let Some(v) = self.writeback_kb {
                    w.field_str(f::WRITEBACK, &io::format_kb_human(v));
                }
            }
        } else {
            // Raw KB numeric values
            w.field_u64_opt(f::MEM_TOTAL_KB, self.mem_total_kb);
            w.field_u64_opt(f::MEM_FREE_KB, self.mem_free_kb);
            w.field_u64_opt(f::MEM_AVAILABLE_KB, self.mem_available_kb);
            w.field_u64_opt(f::SWAP_TOTAL_KB, self.swap_total_kb);
            w.field_u64_opt(f::SWAP_FREE_KB, self.swap_free_kb);

            if verbose {
                w.field_u64_opt(f::BUFFERS_KB, self.buffers_kb);
                w.field_u64_opt(f::CACHED_KB, self.cached_kb);
                w.field_u64_opt(f::SWAP_CACHED_KB, self.swap_cached_kb);
                w.field_u64_opt(f::SHMEM_KB, self.shmem_kb);
                w.field_u64_opt(f::SRECLAIMABLE_KB, self.sreclaimable_kb);
                w.field_u64_opt(f::SUNRECLAIM_KB, self.sunreclaim_kb);
                w.field_u64_opt(f::DIRTY_KB, self.dirty_kb);
                w.field_u64_opt(f::WRITEBACK_KB, self.writeback_kb);
            }
        }

        w.end_field_object();
        w.end_object();

        println!("{}", w.finish());
    }
}

/// Parse a single line from /proc/meminfo.
///
/// Format: "FieldName:        12345 kB"
/// Returns: Some(("FieldName", 12345))
fn parse_meminfo_line(line: &str) -> Option<(&str, u64)> {
    let (key, rest) = line.split_once(':')?;
    let key = key.trim();

    // The value part looks like "        12345 kB" or just "12345"
    let value_str = rest.trim();

    // Strip " kB" suffix if present (some fields don't have it)
    let value_str = value_str.strip_suffix(" kB").unwrap_or(value_str);
    let value_str = value_str.trim();

    let value: u64 = value_str.parse().ok()?;
    Some((key, value))
}

/// Entry point for `kv mem` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    let Some(info) = MemInfo::read() else {
        // Can't read /proc/meminfo - this is unusual but not fatal
        if opts.json {
            // Even errors get JSON wrapper for consistency
            let mut w = begin_kv_output(opts.pretty, "mem");
            w.key("data");
            w.value_null();
            w.key("error");
            w.value_string("cannot read /proc/meminfo");
            w.end_object();
            println!("{}", w.finish());
        } else {
            println!("mem: cannot read {}", MEMINFO_PATH);
        }
        return 0; // Graceful degradation - missing data isn't an error
    };

    if opts.json {
        info.print_json(opts.pretty, opts.verbose, opts.human);
    } else {
        info.print_text(opts.verbose, opts.human);
    }

    0
}

/// Collect memory info for snapshot (returns the data, doesn't print).
#[cfg(feature = "snapshot")]
pub fn collect(_verbose: bool) -> Option<MemInfo> {
    MemInfo::read()
}

/// Write memory info to a JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json(w: &mut JsonWriter, info: &MemInfo, verbose: bool) {
    w.field_object("mem");

    w.field_u64_opt(f::MEM_TOTAL_KB, info.mem_total_kb);
    w.field_u64_opt(f::MEM_FREE_KB, info.mem_free_kb);
    w.field_u64_opt(f::MEM_AVAILABLE_KB, info.mem_available_kb);
    w.field_u64_opt(f::SWAP_TOTAL_KB, info.swap_total_kb);
    w.field_u64_opt(f::SWAP_FREE_KB, info.swap_free_kb);

    if verbose {
        w.field_u64_opt(f::BUFFERS_KB, info.buffers_kb);
        w.field_u64_opt(f::CACHED_KB, info.cached_kb);
        w.field_u64_opt(f::SWAP_CACHED_KB, info.swap_cached_kb);
        w.field_u64_opt(f::SHMEM_KB, info.shmem_kb);
        w.field_u64_opt(f::SRECLAIMABLE_KB, info.sreclaimable_kb);
        w.field_u64_opt(f::SUNRECLAIM_KB, info.sunreclaim_kb);
        w.field_u64_opt(f::DIRTY_KB, info.dirty_kb);
        w.field_u64_opt(f::WRITEBACK_KB, info.writeback_kb);
    }

    w.end_field_object();
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_MEMINFO: &str = r#"MemTotal:       16324656 kB
MemFree:          123456 kB
MemAvailable:   12345678 kB
Buffers:          234567 kB
Cached:          3456789 kB
SwapCached:            0 kB
Active:          4567890 kB
Inactive:        2345678 kB
Active(anon):    1234567 kB
Inactive(anon):   123456 kB
Active(file):    3333333 kB
Inactive(file):  2222222 kB
Unevictable:           0 kB
Mlocked:               0 kB
SwapTotal:       2097148 kB
SwapFree:        2097148 kB
Dirty:               123 kB
Writeback:             0 kB
AnonPages:       1111111 kB
Mapped:           222222 kB
Shmem:            333333 kB
KReclaimable:     444444 kB
Slab:             555555 kB
SReclaimable:     444444 kB
SUnreclaim:       111111 kB
"#;

    #[test]
    fn parse_meminfo() {
        let info = MemInfo::parse(SAMPLE_MEMINFO);

        assert_eq!(info.mem_total_kb, Some(16324656));
        assert_eq!(info.mem_free_kb, Some(123456));
        assert_eq!(info.mem_available_kb, Some(12345678));
        assert_eq!(info.buffers_kb, Some(234567));
        assert_eq!(info.cached_kb, Some(3456789));
        assert_eq!(info.swap_total_kb, Some(2097148));
        assert_eq!(info.swap_free_kb, Some(2097148));
        assert_eq!(info.shmem_kb, Some(333333));
        assert_eq!(info.sreclaimable_kb, Some(444444));
        assert_eq!(info.sunreclaim_kb, Some(111111));
        assert_eq!(info.dirty_kb, Some(123));
        assert_eq!(info.writeback_kb, Some(0));
    }

    #[test]
    fn parse_single_line() {
        let result = parse_meminfo_line("MemTotal:       16324656 kB");
        assert_eq!(result, Some(("MemTotal", 16324656)));
    }

    #[test]
    fn parse_line_without_kb_suffix() {
        // Some fields might not have kB suffix
        let result = parse_meminfo_line("HugePages_Total:       0");
        assert_eq!(result, Some(("HugePages_Total", 0)));
    }

    #[test]
    fn parse_malformed_line() {
        assert!(parse_meminfo_line("not a valid line").is_none());
        assert!(parse_meminfo_line("").is_none());
    }
}
