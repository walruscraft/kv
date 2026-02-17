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
use crate::fields::mem as f;
use crate::io::{self, KbToBytes};
use crate::json::begin_kv_output_streaming;
use crate::print;
use crate::stack::StackString;

/// Path to meminfo. Could be different in containers or chroots,
/// but let's not overthink it for now, will be testing and failing later.
const MEMINFO_PATH: &str = "/proc/meminfo";

/// Memory information structure.
/// All values in KB, because that's what the kernel gives us.
#[derive(Default)]
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
        Self::read_from(MEMINFO_PATH)
    }

    /// Read from a custom path (useful for testing).
    pub fn read_from(path: &str) -> Option<Self> {
        // Use stack-based read - meminfo is typically ~1.5KB
        let contents: StackString<4096> = io::read_file_stack(path)?;
        Some(Self::parse(contents.as_str()))
    }

    /// Parse meminfo content into struct.
    /// Uses direct field matching instead of HashMap to avoid alloc overhead.
    pub fn parse(content: &str) -> Self {
        let mut info = MemInfo::default();

        for line in content.lines() {
            if let Some((key, value)) = parse_meminfo_line(line) {
                match key {
                    "MemTotal" => info.mem_total_kb = Some(value),
                    "MemFree" => info.mem_free_kb = Some(value),
                    "MemAvailable" => info.mem_available_kb = Some(value),
                    "Buffers" => info.buffers_kb = Some(value),
                    "Cached" => info.cached_kb = Some(value),
                    "SwapTotal" => info.swap_total_kb = Some(value),
                    "SwapFree" => info.swap_free_kb = Some(value),
                    "SwapCached" => info.swap_cached_kb = Some(value),
                    "Shmem" => info.shmem_kb = Some(value),
                    "SReclaimable" => info.sreclaimable_kb = Some(value),
                    "SUnreclaim" => info.sunreclaim_kb = Some(value),
                    "Dirty" => info.dirty_kb = Some(value),
                    "Writeback" => info.writeback_kb = Some(value),
                    _ => {} // Ignore fields we don't care about
                }
            }
        }

        info
    }

    /// Output as text (KEY=VALUE format).
    pub fn print_text(&self, verbose: bool, human: bool) {
        let mut w = print::TextWriter::new();

        if human {
            // Human-readable sizes like "16G", "512M"
            w.field_str_opt(f::MEM_TOTAL, self.mem_total_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
            w.field_str_opt(f::MEM_FREE, self.mem_free_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
            w.field_str_opt(f::MEM_AVAILABLE, self.mem_available_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
            w.field_str_opt(f::SWAP_TOTAL, self.swap_total_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
            w.field_str_opt(f::SWAP_FREE, self.swap_free_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));

            if verbose {
                w.field_str_opt(f::BUFFERS, self.buffers_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
                w.field_str_opt(f::CACHED, self.cached_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
                w.field_str_opt(f::SWAP_CACHED, self.swap_cached_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
                w.field_str_opt(f::SHMEM, self.shmem_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
                w.field_str_opt(f::SRECLAIMABLE, self.sreclaimable_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
                w.field_str_opt(f::SUNRECLAIM, self.sunreclaim_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
                w.field_str_opt(f::DIRTY, self.dirty_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
                w.field_str_opt(f::WRITEBACK, self.writeback_kb.map(|v| io::format_human_size(v.kb())).as_ref().map(|s| s.as_str()));
            }
        } else {
            // Raw KB values
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

        w.finish();
    }

    /// Output as JSON (streaming - writes directly to stdout).
    pub fn print_json(&self, pretty: bool, verbose: bool, human: bool) {
        let mut w = begin_kv_output_streaming(pretty, "mem");

        w.field_object("data");

        if human {
            // Human-readable string values
            if let Some(v) = self.mem_total_kb {
                w.field_str(f::MEM_TOTAL, io::format_human_size(v.kb()).as_str());
            }
            if let Some(v) = self.mem_free_kb {
                w.field_str(f::MEM_FREE, io::format_human_size(v.kb()).as_str());
            }
            if let Some(v) = self.mem_available_kb {
                w.field_str(f::MEM_AVAILABLE, io::format_human_size(v.kb()).as_str());
            }
            if let Some(v) = self.swap_total_kb {
                w.field_str(f::SWAP_TOTAL, io::format_human_size(v.kb()).as_str());
            }
            if let Some(v) = self.swap_free_kb {
                w.field_str(f::SWAP_FREE, io::format_human_size(v.kb()).as_str());
            }

            if verbose {
                if let Some(v) = self.buffers_kb {
                    w.field_str(f::BUFFERS, io::format_human_size(v.kb()).as_str());
                }
                if let Some(v) = self.cached_kb {
                    w.field_str(f::CACHED, io::format_human_size(v.kb()).as_str());
                }
                if let Some(v) = self.swap_cached_kb {
                    w.field_str(f::SWAP_CACHED, io::format_human_size(v.kb()).as_str());
                }
                if let Some(v) = self.shmem_kb {
                    w.field_str(f::SHMEM, io::format_human_size(v.kb()).as_str());
                }
                if let Some(v) = self.sreclaimable_kb {
                    w.field_str(f::SRECLAIMABLE, io::format_human_size(v.kb()).as_str());
                }
                if let Some(v) = self.sunreclaim_kb {
                    w.field_str(f::SUNRECLAIM, io::format_human_size(v.kb()).as_str());
                }
                if let Some(v) = self.dirty_kb {
                    w.field_str(f::DIRTY, io::format_human_size(v.kb()).as_str());
                }
                if let Some(v) = self.writeback_kb {
                    w.field_str(f::WRITEBACK, io::format_human_size(v.kb()).as_str());
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
        w.finish();
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
            // Even errors get JSON wrapper for consistency (streaming)
            let mut w = begin_kv_output_streaming(opts.pretty, "mem");
            w.key("data");
            w.value_null();
            w.key("error");
            w.value_string("cannot read /proc/meminfo");
            w.end_object();
            w.finish();
        } else {
            print::print("mem: cannot read ");
            print::println(MEMINFO_PATH);
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
