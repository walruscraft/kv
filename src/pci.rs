//! PCI device information from /sys/bus/pci/devices.
//!
//! Shows PCI devices with their BDF addresses, vendor/device IDs, class codes,
//! and bound driver information. This is what you want when you SSH into a
//! machine and realize lspci isn't installed.
//!
//! We don't do PCI ID database lookups (that would require external files),
//! so you'll see "0x10de" instead of "NVIDIA Corporation". The hex IDs are
//! actually more useful for scripting anyway.

use crate::cli::GlobalOptions;
use crate::fields::{pci as f, to_text_key};
use crate::filter::{opt_str, Filterable};
use crate::io;
use crate::json::{begin_kv_output, JsonWriter};
use std::path::PathBuf;

const PCI_SYSFS_PATH: &str = "/sys/bus/pci/devices";

/// Information about a PCI device.
#[derive(Debug, Clone)]
pub struct PciDevice {
    /// Bus:Device.Function address (e.g., "0000:01:00.0")
    pub bdf: String,
    /// Vendor ID
    pub vendor_id: u16,
    /// Device ID
    pub device_id: u16,
    /// Class code (3 bytes: class, subclass, prog-if)
    pub class: u32,
    /// Subsystem vendor ID (optional)
    pub subsystem_vendor_id: Option<u16>,
    /// Subsystem device ID (optional)
    pub subsystem_device_id: Option<u16>,
    /// Revision ID
    pub revision: Option<u8>,
    /// Currently bound driver (if any)
    pub driver: Option<String>,
    /// NUMA node (for NUMA systems)
    pub numa_node: Option<i32>,
    /// IOMMU group number
    pub iommu_group: Option<u32>,
    /// Is this a bridge?
    pub is_bridge: bool,
    /// Device enabled?
    pub enabled: Option<bool>,
    /// D-state (power state)
    pub d_state: Option<String>,
}

impl Filterable for PciDevice {
    fn filter_fields(&self) -> Vec<&str> {
        // Note: hex IDs are owned strings, so we return them in the vec
        // This works because Vec<&str> can hold &str from owned Strings in scope
        vec![&self.bdf, opt_str(&self.driver)]
    }

    fn matches_filter(&self, pattern: &str, case_insensitive: bool) -> bool {
        // Override default to include hex IDs which need formatting
        let vendor_hex = io::format_hex_u16(self.vendor_id);
        let device_hex = io::format_hex_u16(self.device_id);
        let fields = [
            self.bdf.as_str(),
            opt_str(&self.driver),
            &vendor_hex,
            &device_hex,
        ];
        crate::filter::matches_any(&fields, pattern, case_insensitive)
    }
}

impl PciDevice {
    /// Read a PCI device from sysfs.
    pub fn read(bdf: &str) -> Option<Self> {
        let base = PathBuf::from(PCI_SYSFS_PATH).join(bdf);

        // Must have at least vendor and device
        let vendor_id = io::read_file_hex::<u16>(base.join("vendor"))?;
        let device_id = io::read_file_hex::<u16>(base.join("device"))?;
        let class = io::read_file_hex::<u32>(base.join("class")).unwrap_or(0);

        let subsystem_vendor_id = io::read_file_hex(base.join("subsystem_vendor"));
        let subsystem_device_id = io::read_file_hex(base.join("subsystem_device"));
        let revision = io::read_file_hex(base.join("revision"));

        // Driver is a symlink - we want just the name
        let driver = io::read_link_name(base.join("driver"));

        // NUMA node might be -1 (no NUMA) or a node number
        let numa_node = io::read_file_parse(base.join("numa_node"));

        // IOMMU group is a symlink, we extract the group number from the path
        let iommu_group = io::read_link_name(base.join("iommu_group"))
            .and_then(|s| s.parse().ok());

        // Bridge detection: class code 0x06xxxx
        let is_bridge = (class >> 16) == 0x06;

        let enabled = io::read_file_parse::<u8>(base.join("enable")).map(|v| v != 0);

        // Power state (D0, D3hot, etc.)
        let d_state = io::read_file_string(base.join("power_state"));

        Some(PciDevice {
            bdf: bdf.to_string(),
            vendor_id,
            device_id,
            class,
            subsystem_vendor_id,
            subsystem_device_id,
            revision,
            driver,
            numa_node,
            iommu_group,
            is_bridge,
            enabled,
            d_state,
        })
    }

    /// Get the class code as a hex string.
    pub fn class_hex(&self) -> String {
        format!("0x{:06x}", self.class)
    }

    /// Output as text.
    pub fn print_text(&self, verbose: bool) {
        let mut parts = Vec::new();

        parts.push(format!("{}={}", to_text_key(f::BDF), self.bdf));
        parts.push(format!("{}={}", to_text_key(f::VENDOR_ID), io::format_hex_u16(self.vendor_id)));
        parts.push(format!("{}={}", to_text_key(f::DEVICE_ID), io::format_hex_u16(self.device_id)));
        parts.push(format!("{}={}", to_text_key(f::CLASS), self.class_hex()));

        if let Some(ref driver) = self.driver {
            parts.push(format!("{}={}", to_text_key(f::DRIVER), driver));
        }

        if verbose {
            if let Some(v) = self.subsystem_vendor_id {
                parts.push(format!("{}={}", to_text_key(f::SUBSYS_VENDOR), io::format_hex_u16(v)));
            }
            if let Some(v) = self.subsystem_device_id {
                parts.push(format!("{}={}", to_text_key(f::SUBSYS_DEVICE), io::format_hex_u16(v)));
            }
            if let Some(v) = self.revision {
                parts.push(format!("{}={}", to_text_key(f::REVISION), io::format_hex_u8(v)));
            }
            if let Some(v) = self.numa_node {
                parts.push(format!("{}={}", to_text_key(f::NUMA_NODE), v));
            }
            if let Some(v) = self.iommu_group {
                parts.push(format!("{}={}", to_text_key(f::IOMMU_GROUP), v));
            }
            if let Some(v) = self.enabled {
                parts.push(format!("{}={}", to_text_key(f::ENABLED), if v { 1 } else { 0 }));
            }
            if let Some(ref state) = self.d_state {
                parts.push(format!("{}={}", to_text_key(f::POWER_STATE), state));
            }
        }

        println!("{}", parts.join(" "));
    }
}

/// Read all PCI devices.
pub fn read_pci_devices() -> Vec<PciDevice> {
    let bdf_names = io::read_dir_names_sorted(PCI_SYSFS_PATH);
    bdf_names.iter().filter_map(|bdf| PciDevice::read(bdf)).collect()
}

/// Entry point for `kv pci` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    if !io::path_exists(PCI_SYSFS_PATH) {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "pci");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else {
            println!("pci: no PCI bus found (missing {})", PCI_SYSFS_PATH);
        }
        return 0;
    }

    let devices = read_pci_devices();

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
            let mut w = begin_kv_output(opts.pretty, "pci");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else if opts.filter.is_some() {
            println!("pci: no matching devices");
        } else {
            println!("pci: no PCI devices found");
        }
        return 0;
    }

    if opts.json {
        print_json(&devices, opts.pretty, opts.verbose);
    } else {
        for dev in &devices {
            dev.print_text(opts.verbose);
        }
    }

    0
}

/// Print devices as JSON.
fn print_json(devices: &[PciDevice], pretty: bool, verbose: bool) {
    let mut w = begin_kv_output(pretty, "pci");

    w.field_array("data");
    for dev in devices {
        write_device_json(&mut w, dev, verbose);
    }
    w.end_field_array();

    w.end_object();
    println!("{}", w.finish());
}

/// Write a single device to JSON.
fn write_device_json(w: &mut JsonWriter, dev: &PciDevice, verbose: bool) {
    w.array_object_begin();

    w.field_str(f::BDF, &dev.bdf);
    w.field_str(f::VENDOR_ID, &io::format_hex_u16(dev.vendor_id));
    w.field_str(f::DEVICE_ID, &io::format_hex_u16(dev.device_id));
    w.field_str(f::CLASS, &dev.class_hex());
    w.field_str_opt(f::DRIVER, dev.driver.as_deref());

    if verbose {
        if let Some(v) = dev.subsystem_vendor_id {
            w.field_str(f::SUBSYS_VENDOR, &io::format_hex_u16(v));
        }
        if let Some(v) = dev.subsystem_device_id {
            w.field_str(f::SUBSYS_DEVICE, &io::format_hex_u16(v));
        }
        if let Some(v) = dev.revision {
            w.field_str(f::REVISION, &io::format_hex_u8(v));
        }
        w.field_u64_opt(f::NUMA_NODE, dev.numa_node.map(|v| v as u64));
        w.field_u64_opt(f::IOMMU_GROUP, dev.iommu_group.map(|v| v as u64));
        if let Some(v) = dev.enabled {
            w.field_bool(f::ENABLED, v);
        }
        w.field_str_opt(f::POWER_STATE, dev.d_state.as_deref());
        w.field_bool(f::IS_BRIDGE, dev.is_bridge);
    }

    w.array_object_end();
}

/// Collect PCI devices for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect() -> Vec<PciDevice> {
    if io::path_exists(PCI_SYSFS_PATH) {
        read_pci_devices()
    } else {
        Vec::new()
    }
}

/// Write PCI devices to JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json_snapshot(w: &mut JsonWriter, devices: &[PciDevice], verbose: bool) {
    w.field_array("pci");
    for dev in devices {
        write_device_json(w, dev, verbose);
    }
    w.end_field_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_hex_formatting() {
        let mut dev = PciDevice {
            bdf: "0000:00:00.0".to_string(),
            vendor_id: 0x8086,
            device_id: 0x1234,
            class: 0x060000, // Host bridge
            subsystem_vendor_id: None,
            subsystem_device_id: None,
            revision: None,
            driver: None,
            numa_node: None,
            iommu_group: None,
            is_bridge: true,
            enabled: None,
            d_state: None,
        };

        assert_eq!(dev.class_hex(), "0x060000");

        dev.class = 0x030000; // VGA controller
        assert_eq!(dev.class_hex(), "0x030000");
    }

    #[test]
    fn read_pci_if_available() {
        // This test will do different things depending on the system
        let devices = read_pci_devices();
        println!("Found {} PCI devices", devices.len());
        // If we have devices, they should have valid BDFs
        for dev in &devices {
            assert!(dev.bdf.contains(':'), "BDF should contain colons");
        }
    }

    #[test]
    fn bridge_detection() {
        // Class 0x06xxxx should be bridges
        let class_bridge = 0x060000u32;
        let class_vga = 0x030000u32;

        assert_eq!((class_bridge >> 16), 0x06);
        assert_ne!((class_vga >> 16), 0x06);
    }
}
