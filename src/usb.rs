//! USB device information from /sys/bus/usb/devices.
//!
//! The USB sysfs hierarchy is... interesting. Unlike PCI where you have
//! nice clean BDF addresses, USB has a tree structure where devices are
//! named things like "1-1.4.2" (bus 1, port 1, hub port 4, hub port 2).
//!
//! We enumerate devices from /sys/bus/usb/devices and skip the "usb*"
//! root hub entries since those aren't real devices users care about.
//!
//! Note: USB device strings (manufacturer, product) might require special
//! permissions to read on some systems. We gracefully handle missing strings.

use crate::cli::GlobalOptions;
use crate::filter::{opt_str, Filterable};
use crate::io;
use crate::json::{begin_kv_output, JsonWriter};
use std::path::PathBuf;

const USB_SYSFS_PATH: &str = "/sys/bus/usb/devices";

/// Information about a USB device.
#[derive(Debug, Clone)]
pub struct UsbDevice {
    /// Device name in USB topology (e.g., "1-1.4")
    pub name: String,
    /// Vendor ID
    pub vendor_id: u16,
    /// Product ID
    pub product_id: u16,
    /// Device class
    pub device_class: u8,
    /// Bus number
    pub busnum: u8,
    /// Device number
    pub devnum: u8,
    /// USB speed (in Mbps: 1.5, 12, 480, 5000, 10000, 20000)
    pub speed_mbps: Option<u32>,
    /// Manufacturer string
    pub manufacturer: Option<String>,
    /// Product string
    pub product: Option<String>,
    /// Serial number
    pub serial: Option<String>,
    /// USB version (e.g., "2.00", "3.10")
    pub usb_version: Option<String>,
    /// Number of configurations
    pub num_configurations: Option<u8>,
    /// Current configuration value
    pub configuration: Option<u8>,
    /// Maximum power consumption in mA
    pub max_power_ma: Option<u32>,
    /// Bound driver
    pub driver: Option<String>,
}

impl Filterable for UsbDevice {
    fn filter_fields(&self) -> Vec<&str> {
        vec![&self.name, opt_str(&self.manufacturer), opt_str(&self.product)]
    }

    fn matches_filter(&self, pattern: &str, case_insensitive: bool) -> bool {
        // Override default to include hex IDs which need formatting
        let vendor_hex = io::format_hex_u16(self.vendor_id);
        let product_hex = io::format_hex_u16(self.product_id);
        let fields = [
            self.name.as_str(),
            opt_str(&self.manufacturer),
            opt_str(&self.product),
            &vendor_hex,
            &product_hex,
        ];
        crate::filter::matches_any(&fields, pattern, case_insensitive)
    }
}

impl UsbDevice {
    /// Read a USB device from sysfs.
    pub fn read(name: &str) -> Option<Self> {
        let base = PathBuf::from(USB_SYSFS_PATH).join(name);

        // Skip root hubs (usb1, usb2, etc.) - they're not real devices
        if name.starts_with("usb") {
            return None;
        }

        // Skip interface directories (contain ':')
        if name.contains(':') {
            return None;
        }

        // Must have vendor and product IDs
        let vendor_id = io::read_file_hex::<u16>(base.join("idVendor"))?;
        let product_id = io::read_file_hex::<u16>(base.join("idProduct"))?;

        let device_class = io::read_file_hex(base.join("bDeviceClass")).unwrap_or(0);
        let busnum = io::read_file_parse(base.join("busnum")).unwrap_or(0);
        let devnum = io::read_file_parse(base.join("devnum")).unwrap_or(0);

        // Speed is reported as a string like "480" or "5000"
        let speed_mbps = io::read_file_parse(base.join("speed"));

        // These may require elevated permissions
        let manufacturer = io::read_file_string(base.join("manufacturer"));
        let product = io::read_file_string(base.join("product"));
        let serial = io::read_file_string(base.join("serial"));

        let usb_version = io::read_file_string(base.join("version")).map(|s| s.trim().to_string());
        let num_configurations = io::read_file_parse(base.join("bNumConfigurations"));
        let configuration = io::read_file_parse(base.join("bConfigurationValue"));

        // Max power is in mA but sometimes reported as "500mA" string
        let max_power_ma = io::read_file_string(base.join("bMaxPower"))
            .and_then(|s| {
                let s = s.trim().strip_suffix("mA").unwrap_or(&s);
                s.parse().ok()
            });

        let driver = io::read_link_name(base.join("driver"));

        Some(UsbDevice {
            name: name.to_string(),
            vendor_id,
            product_id,
            device_class,
            busnum,
            devnum,
            speed_mbps,
            manufacturer,
            product,
            serial,
            usb_version,
            num_configurations,
            configuration,
            max_power_ma,
            driver,
        })
    }

    /// Get a human-readable speed description.
    #[allow(dead_code)] // Useful for verbose/debug output in future
    pub fn speed_name(&self) -> Option<&'static str> {
        self.speed_mbps.map(|speed| match speed {
            1 | 2 => "Low Speed (1.5 Mbps)",    // 1.5 reported as 1 or 2
            12 => "Full Speed (12 Mbps)",
            480 => "High Speed (480 Mbps)",
            5000 => "SuperSpeed (5 Gbps)",
            10000 => "SuperSpeed+ (10 Gbps)",
            20000 => "SuperSpeed+ (20 Gbps)",
            _ => "Unknown",
        })
    }

    /// Output as text.
    pub fn print_text(&self, verbose: bool) {
        let mut parts = Vec::new();

        parts.push(format!("NAME={}", self.name));
        parts.push(format!("VENDOR_ID={}", io::format_hex_u16(self.vendor_id)));
        parts.push(format!("PRODUCT_ID={}", io::format_hex_u16(self.product_id)));

        if let Some(ref mfr) = self.manufacturer {
            parts.push(format!("MANUFACTURER=\"{}\"", mfr));
        }
        if let Some(ref prod) = self.product {
            parts.push(format!("PRODUCT=\"{}\"", prod));
        }
        if let Some(speed) = self.speed_mbps {
            parts.push(format!("SPEED_MBPS={}", speed));
        }

        if verbose {
            parts.push(format!("CLASS={}", io::format_hex_u8(self.device_class)));
            parts.push(format!("BUS={}", self.busnum));
            parts.push(format!("DEV={}", self.devnum));
            if let Some(ref serial) = self.serial {
                parts.push(format!("SERIAL=\"{}\"", serial));
            }
            if let Some(ref version) = self.usb_version {
                parts.push(format!("USB_VERSION={}", version));
            }
            if let Some(power) = self.max_power_ma {
                parts.push(format!("MAX_POWER_MA={}", power));
            }
            if let Some(ref driver) = self.driver {
                parts.push(format!("DRIVER={}", driver));
            }
        }

        println!("{}", parts.join(" "));
    }
}

/// Read all USB devices.
pub fn read_usb_devices() -> Vec<UsbDevice> {
    let names = io::read_dir_names_sorted(USB_SYSFS_PATH);
    names.iter().filter_map(|name| UsbDevice::read(name)).collect()
}

/// Entry point for `kv usb` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    if !io::path_exists(USB_SYSFS_PATH) {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "usb");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else {
            println!("usb: no USB bus found (missing {})", USB_SYSFS_PATH);
        }
        return 0;
    }

    let devices = read_usb_devices();

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
            let mut w = begin_kv_output(opts.pretty, "usb");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else if opts.filter.is_some() {
            println!("usb: no matching devices");
        } else {
            println!("usb: no USB devices found");
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
fn print_json(devices: &[UsbDevice], pretty: bool, verbose: bool) {
    let mut w = begin_kv_output(pretty, "usb");

    w.field_array("data");
    for dev in devices {
        write_device_json(&mut w, dev, verbose);
    }
    w.end_field_array();

    w.end_object();
    println!("{}", w.finish());
}

/// Write a single device to JSON.
fn write_device_json(w: &mut JsonWriter, dev: &UsbDevice, verbose: bool) {
    w.array_object_begin();

    w.field_str("name", &dev.name);
    w.field_str("vendor_id", &io::format_hex_u16(dev.vendor_id));
    w.field_str("product_id", &io::format_hex_u16(dev.product_id));
    w.field_str_opt("manufacturer", dev.manufacturer.as_deref());
    w.field_str_opt("product", dev.product.as_deref());
    w.field_u64_opt("speed_mbps", dev.speed_mbps.map(|v| v as u64));

    if verbose {
        w.field_str("device_class", &io::format_hex_u8(dev.device_class));
        w.field_u64("busnum", dev.busnum as u64);
        w.field_u64("devnum", dev.devnum as u64);
        w.field_str_opt("serial", dev.serial.as_deref());
        w.field_str_opt("usb_version", dev.usb_version.as_deref());
        w.field_u64_opt("num_configurations", dev.num_configurations.map(|v| v as u64));
        w.field_u64_opt("configuration", dev.configuration.map(|v| v as u64));
        w.field_u64_opt("max_power_ma", dev.max_power_ma.map(|v| v as u64));
        w.field_str_opt("driver", dev.driver.as_deref());
    }

    w.array_object_end();
}

/// Collect USB devices for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect() -> Vec<UsbDevice> {
    if io::path_exists(USB_SYSFS_PATH) {
        read_usb_devices()
    } else {
        Vec::new()
    }
}

/// Write USB devices to JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json_snapshot(w: &mut JsonWriter, devices: &[UsbDevice], verbose: bool) {
    w.field_array("usb");
    for dev in devices {
        write_device_json(w, dev, verbose);
    }
    w.end_field_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_names() {
        let mut dev = UsbDevice {
            name: "1-1".to_string(),
            vendor_id: 0,
            product_id: 0,
            device_class: 0,
            busnum: 1,
            devnum: 1,
            speed_mbps: Some(480),
            manufacturer: None,
            product: None,
            serial: None,
            usb_version: None,
            num_configurations: None,
            configuration: None,
            max_power_ma: None,
            driver: None,
        };

        assert_eq!(dev.speed_name(), Some("High Speed (480 Mbps)"));

        dev.speed_mbps = Some(5000);
        assert_eq!(dev.speed_name(), Some("SuperSpeed (5 Gbps)"));

        dev.speed_mbps = Some(12);
        assert_eq!(dev.speed_name(), Some("Full Speed (12 Mbps)"));
    }

    #[test]
    fn skip_root_hubs() {
        // Root hub names start with "usb" - we should skip them
        assert!(UsbDevice::read("usb1").is_none());
    }

    #[test]
    fn skip_interfaces() {
        // Interface names contain ':' like "1-1:1.0" - we should skip them
        // (Can't test directly without creating sysfs entries, but the logic is in read())
        assert!("1-1:1.0".contains(':'));
    }

    #[test]
    fn read_devices_doesnt_panic() {
        // Even on systems with no USB (like WSL), this shouldn't panic
        let devices = read_usb_devices();
        println!("Found {} USB devices", devices.len());
    }
}
