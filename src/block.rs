//! Block device information from /sys/block.
//!
//! Provides an lsblk-like view of block devices and their partitions.
//! Reads from /sys/block for device info and cross-references with
//! /proc/self/mounts to show mount points.
//!
//! We handle the somewhat odd sysfs layout where partitions can appear
//! either as subdirectories of /sys/block/<disk>/ or as separate entries.

use crate::cli::GlobalOptions;
use crate::filter::{opt_str, Filterable};
use crate::io;
use crate::json::{begin_kv_output, JsonWriter};
use std::collections::HashMap;
use std::path::PathBuf;

const BLOCK_SYSFS_PATH: &str = "/sys/block";
const MOUNTS_PATH: &str = "/proc/self/mounts";

/// Type of block device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Other reserved for future device types
pub enum BlockType {
    Disk,
    Part,
    Loop,
    Ram,
    Other,
}

impl BlockType {
    fn as_str(&self) -> &'static str {
        match self {
            BlockType::Disk => "disk",
            BlockType::Part => "part",
            BlockType::Loop => "loop",
            BlockType::Ram => "ram",
            BlockType::Other => "other",
        }
    }
}

/// Information about a block device or partition.
#[derive(Debug, Clone)]
pub struct BlockDevice {
    /// Device name (e.g., "sda", "sda1", "nvme0n1p1")
    pub name: String,
    /// Device type
    pub dev_type: BlockType,
    /// Major device number
    pub major: u32,
    /// Minor device number
    pub minor: u32,
    /// Size in sectors
    pub size_sectors: u64,
    /// Sector size in bytes (usually 512)
    pub sector_size: u32,
    /// Is the device removable?
    pub removable: bool,
    /// Is the device read-only?
    pub ro: bool,
    /// Parent device name (for partitions)
    pub parent: Option<String>,
    /// Mount point (if mounted)
    pub mountpoint: Option<String>,
    /// Device model (for disks, if available)
    pub model: Option<String>,
    /// Rotational device (HDD) or not (SSD)?
    pub rotational: Option<bool>,
    /// Scheduler in use
    pub scheduler: Option<String>,
}

impl Filterable for BlockDevice {
    fn filter_fields(&self) -> Vec<&str> {
        vec![
            &self.name,
            opt_str(&self.model),
            opt_str(&self.mountpoint),
            self.dev_type.as_str(),
        ]
    }
}

impl BlockDevice {
    /// Read a block device from sysfs.
    ///
    /// For partitions, most attributes (removable, queue/*, model) don't exist -
    /// they inherit physical characteristics from the parent disk. We skip reading
    /// them to avoid noisy debug output.
    pub fn read(name: &str, parent: Option<&str>, mountpoints: &HashMap<String, String>) -> Option<Self> {
        let base = if let Some(p) = parent {
            PathBuf::from(BLOCK_SYSFS_PATH).join(p).join(name)
        } else {
            PathBuf::from(BLOCK_SYSFS_PATH).join(name)
        };

        // Must have at least size and dev (major:minor)
        if !base.join("size").exists() {
            return None;
        }

        let size_sectors: u64 = io::read_file_parse(base.join("size")).unwrap_or(0);
        let dev_str = io::read_file_string(base.join("dev"))?;
        let (major, minor) = parse_dev(&dev_str)?;

        // Determine device type
        let is_partition = parent.is_some();
        let dev_type = if is_partition {
            BlockType::Part
        } else if name.starts_with("loop") {
            BlockType::Loop
        } else if name.starts_with("ram") {
            BlockType::Ram
        } else {
            BlockType::Disk
        };

        // For partitions, skip reading disk-level attributes that don't exist
        // (removable, queue/*, device/model). They inherit from parent.
        let (removable, sector_size, model, rotational, scheduler) = if is_partition {
            (false, 512, None, None, None)
        } else {
            let removable = io::read_file_parse::<u8>(base.join("removable"))
                .map(|v| v != 0)
                .unwrap_or(false);

            // Sector size - try hw_sector_size first, fall back to logical
            let sector_size = io::read_file_parse(base.join("queue/hw_sector_size"))
                .or_else(|| io::read_file_parse(base.join("queue/logical_block_size")))
                .unwrap_or(512);

            // Model - try device/model first (SCSI/NVMe), then device/name (MMC/SD)
            let model = io::read_file_string(base.join("device/model"))
                .or_else(|| io::read_file_string(base.join("device/name")));

            // Rotational flag
            let rotational = io::read_file_parse::<u8>(base.join("queue/rotational"))
                .map(|v| v != 0);

            // Scheduler (e.g., "[mq-deadline] none" - extract the active one)
            let scheduler = io::read_file_string(base.join("queue/scheduler"))
                .and_then(|s| extract_active_scheduler(&s));

            (removable, sector_size, model, rotational, scheduler)
        };

        // ro is valid for both disks and partitions
        let ro = io::read_file_parse::<u8>(base.join("ro"))
            .map(|v| v != 0)
            .unwrap_or(false);

        // Look up mount point by device path
        let dev_path = format!("/dev/{}", name);
        let mountpoint = mountpoints.get(&dev_path).cloned();

        Some(BlockDevice {
            name: name.to_string(),
            dev_type,
            major,
            minor,
            size_sectors,
            sector_size,
            removable,
            ro,
            parent: parent.map(|s| s.to_string()),
            mountpoint,
            model,
            rotational,
            scheduler,
        })
    }

    /// Calculate size in bytes.
    #[allow(dead_code)] // Useful utility for human-readable output
    pub fn size_bytes(&self) -> u64 {
        self.size_sectors * self.sector_size as u64
    }

    /// Output as text.
    pub fn print_text(&self, verbose: bool, human: bool) {
        let mut parts = Vec::new();

        parts.push(format!("NAME={}", self.name));
        parts.push(format!("TYPE={}", self.dev_type.as_str()));
        parts.push(format!("MAJMIN={}:{}", self.major, self.minor));

        if human {
            let size = io::format_sectors_human(self.size_sectors, self.sector_size);
            parts.push(format!("SIZE={}", size));
        } else {
            parts.push(format!("SIZE_SECTORS={}", self.size_sectors));
        }

        if let Some(ref parent) = self.parent {
            parts.push(format!("PARENT={}", parent));
        }

        if let Some(ref mp) = self.mountpoint {
            parts.push(format!("MOUNTPOINT=\"{}\"", mp));
        }

        if verbose {
            if !human {
                parts.push(format!("SECTOR_SIZE={}", self.sector_size));
            }
            parts.push(format!("REMOVABLE={}", if self.removable { 1 } else { 0 }));
            parts.push(format!("RO={}", if self.ro { 1 } else { 0 }));

            if let Some(ref model) = self.model {
                parts.push(format!("MODEL=\"{}\"", model.trim()));
            }
            if let Some(rot) = self.rotational {
                parts.push(format!("ROTATIONAL={}", if rot { 1 } else { 0 }));
            }
            if let Some(ref sched) = self.scheduler {
                parts.push(format!("SCHEDULER={}", sched));
            }
        }

        println!("{}", parts.join(" "));
    }
}

/// Parse major:minor string.
fn parse_dev(s: &str) -> Option<(u32, u32)> {
    let (maj, min) = s.split_once(':')?;
    Some((maj.trim().parse().ok()?, min.trim().parse().ok()?))
}

/// Extract active scheduler from scheduler file content.
/// Format: "mq-deadline [none]" -> "none"
fn extract_active_scheduler(s: &str) -> Option<String> {
    // Active scheduler is in brackets
    let start = s.find('[')?;
    let end = s.find(']')?;
    if start < end {
        Some(s[start + 1..end].to_string())
    } else {
        None
    }
}

/// Read mount points from /proc/self/mounts.
/// Returns map of device path -> mount point.
fn read_mountpoints() -> HashMap<String, String> {
    let mut map = HashMap::new();

    let Some(contents) = io::read_file_string(MOUNTS_PATH) else {
        return map;
    };

    for line in contents.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            map.insert(parts[0].to_string(), parts[1].to_string());
        }
    }

    map
}

/// Read all block devices with their partitions.
pub fn read_block_devices() -> Vec<BlockDevice> {
    let mountpoints = read_mountpoints();
    let mut devices = Vec::new();

    let disk_names = io::read_dir_names_sorted(BLOCK_SYSFS_PATH);

    for disk_name in &disk_names {
        // Read the disk itself
        if let Some(disk) = BlockDevice::read(disk_name, None, &mountpoints) {
            // Skip loop devices with size 0 (unbound) unless verbose
            if disk.dev_type == BlockType::Loop && disk.size_sectors == 0 {
                continue;
            }

            let parent_name = disk.name.clone();
            devices.push(disk);

            // Look for partitions as subdirectories
            let disk_path = PathBuf::from(BLOCK_SYSFS_PATH).join(&parent_name);
            for entry_name in io::read_dir_names_sorted(&disk_path) {
                // Partition directories start with the disk name
                if entry_name.starts_with(&parent_name) {
                    if let Some(part) = BlockDevice::read(&entry_name, Some(&parent_name), &mountpoints) {
                        devices.push(part);
                    }
                }
            }
        }
    }

    devices
}

/// Entry point for `kv block` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    let devices = read_block_devices();

    // Apply filter if specified
    let devices: Vec<_> = if let Some(ref pattern) = opts.filter {
        devices
            .into_iter()
            .filter(|d| d.matches_filter(pattern, opts.filter_case_insensitive))
            .collect()
    } else {
        devices
    };

    if devices.is_empty() {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "block");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else if opts.filter.is_some() {
            println!("block: no matching devices");
        } else {
            println!("block: no block devices found");
        }
        return 0;
    }

    if opts.json {
        print_json(&devices, opts.pretty, opts.verbose, opts.human);
    } else {
        for dev in &devices {
            dev.print_text(opts.verbose, opts.human);
        }
    }

    0
}

/// Print devices as JSON.
fn print_json(devices: &[BlockDevice], pretty: bool, verbose: bool, human: bool) {
    let mut w = begin_kv_output(pretty, "block");

    w.field_array("data");
    for dev in devices {
        write_device_json(&mut w, dev, verbose, human);
    }
    w.end_field_array();

    w.end_object();
    println!("{}", w.finish());
}

/// Write a single device to JSON.
fn write_device_json(w: &mut JsonWriter, dev: &BlockDevice, verbose: bool, human: bool) {
    w.array_object_begin();

    w.field_str("name", &dev.name);
    w.field_str("type", dev.dev_type.as_str());
    w.field_u64("major", dev.major as u64);
    w.field_u64("minor", dev.minor as u64);

    if human {
        let size = io::format_sectors_human(dev.size_sectors, dev.sector_size);
        w.field_str("size", &size);
    } else {
        w.field_u64("size_sectors", dev.size_sectors);
    }

    w.field_str_opt("parent", dev.parent.as_deref());
    w.field_str_opt("mountpoint", dev.mountpoint.as_deref());

    if verbose {
        if !human {
            w.field_u64("sector_size", dev.sector_size as u64);
        }
        w.field_bool("removable", dev.removable);
        w.field_bool("ro", dev.ro);
        w.field_str_opt("model", dev.model.as_deref());
        if let Some(rot) = dev.rotational {
            w.field_bool("rotational", rot);
        }
        w.field_str_opt("scheduler", dev.scheduler.as_deref());
    }

    w.array_object_end();
}

/// Collect block devices for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect() -> Vec<BlockDevice> {
    read_block_devices()
}

/// Write block devices to JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json_snapshot(w: &mut JsonWriter, devices: &[BlockDevice], verbose: bool) {
    w.field_array("block");
    for dev in devices {
        write_device_json(w, dev, verbose, false); // snapshot uses raw values
    }
    w.end_field_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dev_string() {
        assert_eq!(parse_dev("8:0"), Some((8, 0)));
        assert_eq!(parse_dev("259:1"), Some((259, 1)));
        assert_eq!(parse_dev("invalid"), None);
    }

    #[test]
    fn extract_scheduler() {
        assert_eq!(
            extract_active_scheduler("mq-deadline kyber [none]"),
            Some("none".to_string())
        );
        assert_eq!(
            extract_active_scheduler("[mq-deadline] kyber none"),
            Some("mq-deadline".to_string())
        );
    }

    #[test]
    fn read_some_devices() {
        // On any Linux system, we should have at least some block devices
        let devices = read_block_devices();
        // This might be empty in some containers, but usually there's at least something
        // Let's just make sure it doesn't panic
        println!("Found {} block devices", devices.len());
    }

    #[test]
    fn block_type_strings() {
        assert_eq!(BlockType::Disk.as_str(), "disk");
        assert_eq!(BlockType::Part.as_str(), "part");
        assert_eq!(BlockType::Loop.as_str(), "loop");
    }
}
