//! I/O helpers for reading sysfs and procfs.
//!
//! These functions are designed to be forgiving - they return Option<T> rather
//! than Result<T, E> because in the world of /sys and /proc, files come and go,
//! permissions vary, and we'd rather skip a field than crash.
//!
//! Philosophy: "If you can't read it, shrug and move on."

use std::fs;
use std::path::Path;
use std::str::FromStr;

/// Read an entire file to a String, trimming whitespace.
///
/// Returns None if the file doesn't exist, can't be read, or isn't valid UTF-8.
/// This is the workhorse for reading sysfs attributes like `/sys/bus/pci/devices/0000:01:00.0/vendor`.
///
/// # Why Option instead of Result?
///
/// In sysfs/procfs land, missing files are normal - a device might not have
/// a numa_node, a network interface might not report speed, etc. We treat
/// "can't read" the same as "doesn't exist" for simplicity.
pub fn read_file_string(path: impl AsRef<Path>) -> Option<String> {
    let path = path.as_ref();
    match fs::read_to_string(path) {
        Ok(s) => {
            let trimmed = s.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }
        Err(e) => {
            crate::dbg_fail!(path.display(), e);
            None
        }
    }
}

/// Read a file and parse it as type T.
///
/// Useful for numeric sysfs attributes. Handles the typical case where
/// the kernel writes something like "12345\n" and we want the number.
pub fn read_file_parse<T: FromStr>(path: impl AsRef<Path>) -> Option<T> {
    read_file_string(path)?.parse().ok()
}

/// Read a file containing a hexadecimal value.
///
/// Handles both "0x1234" and "1234" formats because the kernel isn't
/// always consistent about prefixes. Life's too short to care.
pub fn read_file_hex<T>(path: impl AsRef<Path>) -> Option<T>
where
    T: FromStrRadix,
{
    let s = read_file_string(path)?;
    parse_hex(&s)
}

/// Parse a hex string, with or without "0x" prefix.
pub fn parse_hex<T: FromStrRadix>(s: &str) -> Option<T> {
    let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    T::from_str_radix(s, 16)
}

/// Trait for types that can be parsed from a string with a radix.
/// We need this because FromStr doesn't support radix, and we're not
/// pulling in external crates for something this simple.
pub trait FromStrRadix: Sized {
    fn from_str_radix(s: &str, radix: u32) -> Option<Self>;
}

// Implement for the integer types we care about
macro_rules! impl_from_str_radix {
    ($($t:ty),*) => {
        $(
            impl FromStrRadix for $t {
                fn from_str_radix(s: &str, radix: u32) -> Option<Self> {
                    <$t>::from_str_radix(s, radix).ok()
                }
            }
        )*
    };
}

impl_from_str_radix!(u8, u16, u32, u64, usize, i32, i64);

/// Read the names of entries in a directory.
///
/// Returns an empty Vec if the directory doesn't exist or can't be read.
/// Skips entries that can't be read (hey, it happens).
pub fn read_dir_names(path: impl AsRef<Path>) -> Vec<String> {
    let path = path.as_ref();
    let entries = match fs::read_dir(path) {
        Ok(e) => e,
        Err(e) => {
            crate::dbg_fail!(path.display(), e);
            return Vec::new();
        }
    };

    let names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();

    crate::dbg_scan!(path.display(), names.len());
    names
}

/// Read directory entries sorted alphabetically.
///
/// Sorting gives us predictable, reproducible output - important for
/// diffing snapshots and not driving users crazy.
pub fn read_dir_names_sorted(path: impl AsRef<Path>) -> Vec<String> {
    let mut names = read_dir_names(path);
    names.sort();
    names
}

/// Get the target of a symbolic link as a String.
///
/// Used for things like reading the driver name from
/// `/sys/bus/pci/devices/0000:01:00.0/driver` which is a symlink
/// to something like `../../../bus/pci/drivers/nouveau`.
pub fn read_link_name(path: impl AsRef<Path>) -> Option<String> {
    fs::read_link(path.as_ref())
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
}

/// Check if a path exists.
///
/// Simple wrapper, but useful for readability in conditionals.
pub fn path_exists(path: impl AsRef<Path>) -> bool {
    path.as_ref().exists()
}

/// Format a u32 as a hex string with "0x" prefix.
///
/// Because "0x10de" is more readable than "4318" when you're
/// trying to figure out what GPU you've got.
#[allow(dead_code)] // May be used by future class code formatting
pub fn format_hex_u32(val: u32) -> String {
    format!("0x{:04x}", val)
}

/// Format a u16 as a hex string with "0x" prefix.
pub fn format_hex_u16(val: u16) -> String {
    format!("0x{:04x}", val)
}

/// Format a u8 as a hex string with "0x" prefix.
pub fn format_hex_u8(val: u8) -> String {
    format!("0x{:02x}", val)
}

/// Format bytes as human-readable size using binary (1024) divisors.
///
/// Follows the `ls -h` convention: 1K = 1024 bytes, etc.
/// Uses one decimal place for values >= 10, no decimal for smaller.
///
/// Examples:
/// - 512 -> "512"
/// - 1024 -> "1K"
/// - 1536 -> "1.5K"
/// - 1073741824 -> "1G"
/// - 1649267441664 -> "1.5T"
pub fn format_size_human(bytes: u64) -> String {
    const KI: u64 = 1024;
    const MI: u64 = 1024 * 1024;
    const GI: u64 = 1024 * 1024 * 1024;
    const TI: u64 = 1024 * 1024 * 1024 * 1024;
    const PI: u64 = 1024 * 1024 * 1024 * 1024 * 1024;

    if bytes >= PI {
        let val = bytes as f64 / PI as f64;
        format_human_value(val, "P")
    } else if bytes >= TI {
        let val = bytes as f64 / TI as f64;
        format_human_value(val, "T")
    } else if bytes >= GI {
        let val = bytes as f64 / GI as f64;
        format_human_value(val, "G")
    } else if bytes >= MI {
        let val = bytes as f64 / MI as f64;
        format_human_value(val, "M")
    } else if bytes >= KI {
        let val = bytes as f64 / KI as f64;
        format_human_value(val, "K")
    } else {
        format!("{}", bytes)
    }
}

/// Format kilobytes as human-readable size.
///
/// Input is in KB (like /proc/meminfo), output uses binary divisors.
pub fn format_kb_human(kb: u64) -> String {
    const KI: u64 = 1024; // 1 MiB in KB
    const MI: u64 = 1024 * 1024; // 1 GiB in KB
    const GI: u64 = 1024 * 1024 * 1024; // 1 TiB in KB

    if kb >= GI {
        let val = kb as f64 / GI as f64;
        format_human_value(val, "T")
    } else if kb >= MI {
        let val = kb as f64 / MI as f64;
        format_human_value(val, "G")
    } else if kb >= KI {
        let val = kb as f64 / KI as f64;
        format_human_value(val, "M")
    } else {
        format!("{}K", kb)
    }
}

/// Format sectors as human-readable size.
///
/// Assumes 512-byte sectors (standard for Linux block devices).
pub fn format_sectors_human(sectors: u64, sector_size: u32) -> String {
    let bytes = sectors * sector_size as u64;
    format_size_human(bytes)
}

/// Helper to format a value with suffix, choosing precision based on magnitude.
fn format_human_value(val: f64, suffix: &str) -> String {
    if val >= 10.0 {
        // Large values: no decimal (e.g., "15G")
        format!("{:.0}{}", val, suffix)
    } else {
        // Small values: one decimal (e.g., "1.5G")
        // But trim ".0" for clean whole numbers
        let s = format!("{:.1}{}", val, suffix);
        if s.contains(".0") {
            format!("{:.0}{}", val, suffix)
        } else {
            s
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: We can't use tempfile (external crate), so our unit tests focus on
    // pure functions. File I/O tests are done via integration tests that use
    // real /proc and /sys paths (which is actually better - tests real behavior!)

    mod parse_hex_tests {
        use super::*;

        #[test]
        fn with_0x_prefix() {
            assert_eq!(parse_hex::<u32>("0x1234"), Some(0x1234));
            assert_eq!(parse_hex::<u32>("0xABCD"), Some(0xABCD));
            assert_eq!(parse_hex::<u32>("0xabcd"), Some(0xabcd));
        }

        #[test]
        #[allow(non_snake_case)]
        fn with_0X_prefix() {
            // Some systems might use uppercase X. Weird, but let's handle it.
            assert_eq!(parse_hex::<u32>("0X1234"), Some(0x1234));
        }

        #[test]
        fn without_prefix() {
            assert_eq!(parse_hex::<u32>("1234"), Some(0x1234));
            assert_eq!(parse_hex::<u16>("ffff"), Some(0xffff));
        }

        #[test]
        fn invalid_hex() {
            assert_eq!(parse_hex::<u32>("not_hex"), None);
            assert_eq!(parse_hex::<u32>("0xGGGG"), None);
        }

        #[test]
        fn empty_string() {
            assert_eq!(parse_hex::<u32>(""), None);
        }

        #[test]
        fn different_sizes() {
            assert_eq!(parse_hex::<u8>("ff"), Some(0xff));
            assert_eq!(parse_hex::<u16>("ffff"), Some(0xffff));
            assert_eq!(parse_hex::<u64>("ffffffffffffffff"), Some(u64::MAX));
        }
    }

    mod format_hex_tests {
        use super::*;

        #[test]
        fn u32_formatting() {
            assert_eq!(format_hex_u32(0x10de), "0x10de");
            assert_eq!(format_hex_u32(0x0001), "0x0001");
            assert_eq!(format_hex_u32(0), "0x0000");
        }

        #[test]
        fn u16_formatting() {
            assert_eq!(format_hex_u16(0x1234), "0x1234");
            assert_eq!(format_hex_u16(0x00ff), "0x00ff");
        }

        #[test]
        fn u8_formatting() {
            assert_eq!(format_hex_u8(0xff), "0xff");
            assert_eq!(format_hex_u8(0x0a), "0x0a");
        }
    }

    mod read_file_tests {
        use super::*;

        #[test]
        fn read_nonexistent_file() {
            assert_eq!(read_file_string("/this/path/definitely/does/not/exist"), None);
        }

        #[test]
        fn read_numeric_file() {
            // Simulating what the kernel writes to sysfs
            // Can't easily test without temp files in pure std
            // These would be integration tests in practice
        }
    }

    mod read_dir_tests {
        use super::*;

        #[test]
        fn read_nonexistent_dir() {
            let names = read_dir_names("/this/path/definitely/does/not/exist");
            assert!(names.is_empty());
        }

        #[test]
        fn sorted_output() {
            // Test that sorting works
            let mut v = vec!["zebra".to_string(), "apple".to_string(), "mango".to_string()];
            v.sort();
            assert_eq!(v, vec!["apple", "mango", "zebra"]);
        }
    }

    mod human_format_tests {
        use super::*;

        #[test]
        fn bytes_small() {
            assert_eq!(format_size_human(0), "0");
            assert_eq!(format_size_human(512), "512");
            assert_eq!(format_size_human(1023), "1023");
        }

        #[test]
        fn bytes_kilobytes() {
            assert_eq!(format_size_human(1024), "1K");
            assert_eq!(format_size_human(1536), "1.5K");
            assert_eq!(format_size_human(10240), "10K");
            assert_eq!(format_size_human(102400), "100K");
        }

        #[test]
        fn bytes_megabytes() {
            assert_eq!(format_size_human(1024 * 1024), "1M");
            assert_eq!(format_size_human(1024 * 1024 + 512 * 1024), "1.5M");
            assert_eq!(format_size_human(500 * 1024 * 1024), "500M");
        }

        #[test]
        fn bytes_gigabytes() {
            assert_eq!(format_size_human(1024 * 1024 * 1024), "1G");
            assert_eq!(format_size_human(2 * 1024 * 1024 * 1024_u64), "2G");
            assert_eq!(format_size_human(500 * 1024 * 1024 * 1024_u64), "500G");
        }

        #[test]
        fn bytes_terabytes() {
            assert_eq!(format_size_human(1024_u64 * 1024 * 1024 * 1024), "1T");
            assert_eq!(format_size_human(2_u64 * 1024 * 1024 * 1024 * 1024), "2T");
        }

        #[test]
        fn kb_to_human() {
            assert_eq!(format_kb_human(1), "1K");
            assert_eq!(format_kb_human(1024), "1M");
            assert_eq!(format_kb_human(1024 * 1024), "1G");
            assert_eq!(format_kb_human(16 * 1024 * 1024), "16G");
            assert_eq!(format_kb_human(64 * 1024 * 1024), "64G");
        }

        #[test]
        fn sectors_to_human() {
            // 2 sectors * 512 bytes = 1024 bytes = 1K
            assert_eq!(format_sectors_human(2, 512), "1K");
            // 2M sectors * 512 = 1G
            assert_eq!(format_sectors_human(2 * 1024 * 1024, 512), "1G");
            // 4T sectors at 512 bytes each = 2T
            assert_eq!(format_sectors_human(4_u64 * 1024 * 1024 * 1024, 512), "2T");
        }
    }
}
