//! Mount point information from /proc/self/mounts.
//!
//! Shows what's mounted where, which filesystem type, and mount options.
//! Useful when you need to know if that NFS share actually mounted or
//! why your rootfs is mysteriously read-only.

use crate::cli::GlobalOptions;
use crate::filter::Filterable;
use crate::io;
use crate::json::{begin_kv_output, JsonWriter};

const MOUNTS_PATH: &str = "/proc/self/mounts";

/// A single mount point entry.
#[derive(Debug, Clone)]
pub struct MountEntry {
    /// Source device or pseudo-filesystem (e.g., "/dev/sda1" or "tmpfs")
    pub source: String,
    /// Target mount point (e.g., "/" or "/home")
    pub target: String,
    /// Filesystem type (e.g., "ext4", "tmpfs", "nfs")
    pub fstype: String,
    /// Mount options as a single string
    pub options: String,
    /// Dump frequency (from fstab, usually 0)
    pub dump_freq: u32,
    /// fsck pass number (usually 0)
    pub pass_num: u32,
}

impl MountEntry {
    /// Parse a line from /proc/mounts.
    ///
    /// Format: device mountpoint fstype options dump pass
    /// Example: /dev/sda1 / ext4 rw,relatime 0 0
    ///
    /// Note: Spaces in paths are escaped as \040 (octal), and we decode them.
    pub fn parse(line: &str) -> Option<Self> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            return None;
        }

        Some(MountEntry {
            source: decode_mount_escapes(parts[0]),
            target: decode_mount_escapes(parts[1]),
            fstype: parts[2].to_string(),
            options: parts[3].to_string(),
            dump_freq: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
            pass_num: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
        })
    }

    /// Output as text (single line, KEY=VALUE format).
    pub fn print_text(&self) {
        // Quote source and target since they might have spaces
        println!(
            "SOURCE=\"{}\" TARGET=\"{}\" FSTYPE={} OPTIONS=\"{}\"",
            self.source, self.target, self.fstype, self.options
        );
    }

}

impl Filterable for MountEntry {
    fn filter_fields(&self) -> Vec<&str> {
        vec![&self.source, &self.target, &self.fstype]
    }
}

/// Decode mount escape sequences.
///
/// The kernel escapes special characters in mount paths using octal:
/// - Space: \040
/// - Tab: \011
/// - Newline: \012
/// - Backslash: \134
///
/// This is important for paths like "/mnt/My Documents" which would
/// otherwise confuse the space-separated format.
fn decode_mount_escapes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            // Try to parse octal escape (3 digits)
            let mut octal = String::new();
            for _ in 0..3 {
                if let Some(&next) = chars.peek() {
                    if next.is_ascii_digit() {
                        octal.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
            }

            if octal.len() == 3 {
                // Parse as octal and convert to char
                if let Ok(byte) = u8::from_str_radix(&octal, 8) {
                    result.push(byte as char);
                    continue;
                }
            }
            // Not a valid escape, keep the backslash and any digits we consumed
            result.push('\\');
            result.push_str(&octal);
        } else {
            result.push(c);
        }
    }

    result
}

/// Read all mount entries.
pub fn read_mounts() -> Vec<MountEntry> {
    let Some(contents) = io::read_file_string(MOUNTS_PATH) else {
        return Vec::new();
    };

    contents.lines().filter_map(MountEntry::parse).collect()
}

/// Entry point for `kv mounts` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    let mounts = read_mounts();

    // Apply filter if specified
    let mounts: Vec<_> = if let Some(ref pattern) = opts.filter {
        mounts
            .into_iter()
            .filter(|m| m.matches_filter(pattern, opts.filter_case_insensitive))
            .collect()
    } else {
        mounts
    };

    if mounts.is_empty() {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "mounts");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else if opts.filter.is_some() {
            println!("mounts: no matching mounts");
        } else {
            println!("mounts: no mounts found (is /proc mounted?)");
        }
        return 0;
    }

    if opts.json {
        print_json(&mounts, opts.pretty, opts.verbose);
    } else {
        for mount in &mounts {
            mount.print_text();
        }
    }

    0
}

/// Print mounts as JSON.
fn print_json(mounts: &[MountEntry], pretty: bool, verbose: bool) {
    let mut w = begin_kv_output(pretty, "mounts");

    w.field_array("data");
    for mount in mounts {
        w.array_object_begin();
        w.field_str("source", &mount.source);
        w.field_str("target", &mount.target);
        w.field_str("fstype", &mount.fstype);
        w.field_str("options", &mount.options);
        if verbose {
            w.field_u64("dump_freq", mount.dump_freq as u64);
            w.field_u64("pass_num", mount.pass_num as u64);
        }
        w.array_object_end();
    }
    w.end_field_array();

    w.end_object();
    println!("{}", w.finish());
}

/// Collect mounts for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect() -> Vec<MountEntry> {
    read_mounts()
}

/// Write mounts to JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json(w: &mut JsonWriter, mounts: &[MountEntry], verbose: bool) {
    w.field_array("mounts");
    for mount in mounts {
        w.array_object_begin();
        w.field_str("source", &mount.source);
        w.field_str("target", &mount.target);
        w.field_str("fstype", &mount.fstype);
        w.field_str("options", &mount.options);
        if verbose {
            w.field_u64("dump_freq", mount.dump_freq as u64);
            w.field_u64("pass_num", mount.pass_num as u64);
        }
        w.array_object_end();
    }
    w.end_field_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_mount() {
        let line = "/dev/sda1 / ext4 rw,relatime 0 0";
        let entry = MountEntry::parse(line).unwrap();

        assert_eq!(entry.source, "/dev/sda1");
        assert_eq!(entry.target, "/");
        assert_eq!(entry.fstype, "ext4");
        assert_eq!(entry.options, "rw,relatime");
    }

    #[test]
    fn parse_tmpfs() {
        let line = "tmpfs /tmp tmpfs rw,nosuid,nodev 0 0";
        let entry = MountEntry::parse(line).unwrap();

        assert_eq!(entry.source, "tmpfs");
        assert_eq!(entry.target, "/tmp");
        assert_eq!(entry.fstype, "tmpfs");
    }

    #[test]
    fn decode_space_in_path() {
        let decoded = decode_mount_escapes("/mnt/My\\040Documents");
        assert_eq!(decoded, "/mnt/My Documents");
    }

    #[test]
    fn decode_multiple_escapes() {
        let decoded = decode_mount_escapes("/mnt/path\\040with\\040spaces\\040here");
        assert_eq!(decoded, "/mnt/path with spaces here");
    }

    #[test]
    fn decode_tab_escape() {
        let decoded = decode_mount_escapes("/mnt/with\\011tab");
        assert_eq!(decoded, "/mnt/with\ttab");
    }

    #[test]
    fn decode_backslash_escape() {
        let decoded = decode_mount_escapes("/mnt/back\\134slash");
        assert_eq!(decoded, "/mnt/back\\slash");
    }

    #[test]
    fn decode_no_escapes() {
        let decoded = decode_mount_escapes("/simple/path");
        assert_eq!(decoded, "/simple/path");
    }

    #[test]
    fn parse_malformed_line() {
        assert!(MountEntry::parse("not enough fields").is_none());
        assert!(MountEntry::parse("").is_none());
    }
}
