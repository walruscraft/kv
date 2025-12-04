//! Power supply information from /sys/class/power_supply/.
//!
//! Linux exposes batteries, AC adapters, and USB power sources via sysfs.
//! Each power_supply directory contains attributes like type, status,
//! capacity (for batteries), and voltage/current readings.
//!
//! Common types:
//! - Battery: laptop/device batteries with capacity, status, etc.
//! - Mains: AC adapter (online/offline)
//! - USB: USB power delivery sources
//! - UPS: Uninterruptible power supplies

use crate::cli::GlobalOptions;
use crate::filter::{opt_str, Filterable};
use crate::io::{read_dir_names_sorted, read_file_string};
use crate::json::{begin_kv_output, JsonWriter};
use std::path::Path;

const POWER_SUPPLY_PATH: &str = "/sys/class/power_supply";

/// Information about a single power supply.
#[derive(Debug, Clone)]
pub struct PowerSupply {
    /// Supply name (e.g., "BAT0", "AC", "ucsi-source-psy-...")
    pub name: String,
    /// Type: Battery, Mains, USB, UPS, etc.
    pub supply_type: Option<String>,
    /// Status: Charging, Discharging, Full, Not charging, Unknown
    pub status: Option<String>,
    /// Online status (1 = connected, 0 = disconnected) - for Mains/USB
    pub online: Option<u8>,
    /// Battery capacity percentage (0-100)
    pub capacity: Option<u8>,
    /// Voltage now in microvolts
    pub voltage_uv: Option<i64>,
    /// Current now in microamps (positive = charging, negative = discharging)
    pub current_ua: Option<i64>,
    /// Power now in microwatts
    pub power_uw: Option<i64>,
    /// USB type (for USB power supplies): C, PD, PD_PPS, etc.
    pub usb_type: Option<String>,
    /// Energy now in microwatt-hours (battery)
    pub energy_now_uwh: Option<i64>,
    /// Energy full in microwatt-hours (battery design capacity)
    pub energy_full_uwh: Option<i64>,
    /// Charge now in microamp-hours (battery)
    pub charge_now_uah: Option<i64>,
    /// Charge full in microamp-hours (battery)
    pub charge_full_uah: Option<i64>,
    /// Battery cycle count
    pub cycle_count: Option<i32>,
    /// Battery technology (Li-ion, Li-poly, NiMH, etc.)
    pub technology: Option<String>,
    /// Manufacturer name
    pub manufacturer: Option<String>,
    /// Model name
    pub model_name: Option<String>,
    /// Maximum current in microamps (USB PD negotiated)
    pub current_max_ua: Option<i64>,
    /// Maximum voltage in microvolts (USB PD negotiated)
    pub voltage_max_uv: Option<i64>,
}

impl PowerSupply {
    /// Read power supply info from a sysfs path.
    fn read_from(base: &Path, name: &str) -> Option<Self> {
        let path = base.join(name);
        if !path.is_dir() {
            return None;
        }

        let supply_type = read_file_string(path.join("type"));
        let status = read_file_string(path.join("status"));
        let online = read_file_string(path.join("online"))
            .and_then(|s| s.parse::<u8>().ok());
        let capacity = read_file_string(path.join("capacity"))
            .and_then(|s| s.parse::<u8>().ok());

        // Voltage - try voltage_now first
        let voltage_uv = read_file_string(path.join("voltage_now"))
            .and_then(|s| s.parse::<i64>().ok());

        // Current - try current_now (some systems use current_avg)
        let current_ua = read_file_string(path.join("current_now"))
            .and_then(|s| s.parse::<i64>().ok());

        // Power - try power_now
        let power_uw = read_file_string(path.join("power_now"))
            .and_then(|s| s.parse::<i64>().ok());

        // USB type for USB power supplies (shows [active] type)
        let usb_type = read_file_string(path.join("usb_type"))
            .map(|s| parse_usb_type(&s));

        // Energy (battery) - in microwatt-hours
        let energy_now_uwh = read_file_string(path.join("energy_now"))
            .and_then(|s| s.parse::<i64>().ok());
        let energy_full_uwh = read_file_string(path.join("energy_full"))
            .and_then(|s| s.parse::<i64>().ok());

        // Charge (battery) - in microamp-hours
        let charge_now_uah = read_file_string(path.join("charge_now"))
            .and_then(|s| s.parse::<i64>().ok());
        let charge_full_uah = read_file_string(path.join("charge_full"))
            .and_then(|s| s.parse::<i64>().ok());

        // Battery metadata
        let cycle_count = read_file_string(path.join("cycle_count"))
            .and_then(|s| s.parse::<i32>().ok())
            .filter(|&c| c >= 0); // -1 means unavailable
        let technology = read_file_string(path.join("technology"))
            .filter(|s| s != "Unknown");
        let manufacturer = read_file_string(path.join("manufacturer"))
            .filter(|s| !s.is_empty());
        let model_name = read_file_string(path.join("model_name"))
            .filter(|s| !s.is_empty());

        // USB PD negotiated limits
        let current_max_ua = read_file_string(path.join("current_max"))
            .and_then(|s| s.parse::<i64>().ok());
        let voltage_max_uv = read_file_string(path.join("voltage_max"))
            .and_then(|s| s.parse::<i64>().ok());

        Some(PowerSupply {
            name: name.to_string(),
            supply_type,
            status,
            online,
            capacity,
            voltage_uv,
            current_ua,
            power_uw,
            usb_type,
            energy_now_uwh,
            energy_full_uwh,
            charge_now_uah,
            charge_full_uah,
            cycle_count,
            technology,
            manufacturer,
            model_name,
            current_max_ua,
            voltage_max_uv,
        })
    }

    /// Voltage in volts (for display).
    pub fn voltage_v(&self) -> Option<f64> {
        self.voltage_uv.map(|v| v as f64 / 1_000_000.0)
    }

    /// Current in amps (for display).
    pub fn current_a(&self) -> Option<f64> {
        self.current_ua.map(|c| c as f64 / 1_000_000.0)
    }

    /// Power in watts (for display).
    pub fn power_w(&self) -> Option<f64> {
        self.power_uw.map(|p| p as f64 / 1_000_000.0)
    }

    /// Energy now in watt-hours.
    pub fn energy_now_wh(&self) -> Option<f64> {
        self.energy_now_uwh.map(|e| e as f64 / 1_000_000.0)
    }

    /// Energy full in watt-hours.
    pub fn energy_full_wh(&self) -> Option<f64> {
        self.energy_full_uwh.map(|e| e as f64 / 1_000_000.0)
    }

    /// Charge now in milliamp-hours.
    pub fn charge_now_mah(&self) -> Option<f64> {
        self.charge_now_uah.map(|c| c as f64 / 1_000.0)
    }

    /// Charge full in milliamp-hours.
    pub fn charge_full_mah(&self) -> Option<f64> {
        self.charge_full_uah.map(|c| c as f64 / 1_000.0)
    }

    /// Maximum current in amps (USB PD).
    pub fn current_max_a(&self) -> Option<f64> {
        self.current_max_ua.map(|c| c as f64 / 1_000_000.0)
    }

    /// Maximum voltage in volts (USB PD).
    pub fn voltage_max_v(&self) -> Option<f64> {
        self.voltage_max_uv.map(|v| v as f64 / 1_000_000.0)
    }

}

impl Filterable for PowerSupply {
    fn filter_fields(&self) -> Vec<&str> {
        vec![
            &self.name,
            opt_str(&self.supply_type),
            opt_str(&self.status),
            opt_str(&self.usb_type),
        ]
    }
}

/// Format voltage for display.
fn format_voltage(v: f64, human: bool) -> String {
    if human {
        format!("{:.1}V", v)
    } else {
        format!("{:.2}", v)
    }
}

/// Format current for display.
fn format_current(c: f64, human: bool) -> String {
    if human {
        if c.abs() >= 1.0 {
            format!("{:.1}A", c)
        } else {
            format!("{:.0}mA", c * 1000.0)
        }
    } else {
        format!("{:.3}", c)
    }
}

/// Format power for display.
fn format_power(p: f64, human: bool) -> String {
    if human {
        format!("{:.1}W", p)
    } else {
        format!("{:.2}", p)
    }
}

/// Format energy for display.
fn format_energy(e: f64, human: bool) -> String {
    if human {
        format!("{:.1}Wh", e)
    } else {
        format!("{:.2}", e)
    }
}

/// Format charge for display.
fn format_charge(c: f64, human: bool) -> String {
    if human {
        format!("{:.0}mAh", c)
    } else {
        format!("{:.0}", c)
    }
}

/// Parse USB type string - extract the active type marked with [brackets].
fn parse_usb_type(s: &str) -> String {
    // Format: "C [PD] PD_PPS" - extract what's in brackets
    if let Some(start) = s.find('[') {
        if let Some(end) = s.find(']') {
            if start < end {
                return s[start + 1..end].to_string();
            }
        }
    }
    // No brackets - return as-is (trimmed)
    s.trim().to_string()
}

/// Read all power supplies from the system.
pub fn read_power_supplies() -> Vec<PowerSupply> {
    read_power_supplies_from(Path::new(POWER_SUPPLY_PATH))
}

/// Read power supplies from a custom path (for testing).
pub fn read_power_supplies_from(base: &Path) -> Vec<PowerSupply> {
    let names = read_dir_names_sorted(base);
    names
        .iter()
        .filter_map(|name| PowerSupply::read_from(base, name))
        .collect()
}

/// Print power supplies as text.
fn print_text(supplies: &[PowerSupply], verbose: bool, human: bool) {
    for supply in supplies {
        let mut parts = Vec::new();

        parts.push(format!("NAME={}", supply.name));

        if let Some(ref t) = supply.supply_type {
            parts.push(format!("TYPE={}", t));
        }

        // For batteries, show status and capacity
        if supply.supply_type.as_deref() == Some("Battery") {
            if let Some(ref status) = supply.status {
                parts.push(format!("STATUS={}", status));
            }
            if let Some(cap) = supply.capacity {
                parts.push(format!("CAPACITY={}%", cap));
            }
        } else {
            // For Mains/USB, show online status
            if let Some(online) = supply.online {
                parts.push(format!("ONLINE={}", if online == 1 { "yes" } else { "no" }));
            }
        }

        // USB type for USB supplies
        if let Some(ref usb_type) = supply.usb_type {
            parts.push(format!("USB_TYPE={}", usb_type));
        }

        // Verbose mode: show electrical details
        if verbose {
            // Energy (batteries)
            if let (Some(now), Some(full)) = (supply.energy_now_wh(), supply.energy_full_wh()) {
                if human {
                    parts.push(format!("ENERGY={}/{}", format_energy(now, true), format_energy(full, true)));
                } else {
                    parts.push(format!("ENERGY_WH={:.1}/{:.1}", now, full));
                }
            } else if let (Some(now), Some(full)) = (supply.charge_now_mah(), supply.charge_full_mah()) {
                // Some batteries report charge instead of energy
                if human {
                    parts.push(format!("CHARGE={}/{}", format_charge(now, true), format_charge(full, true)));
                } else {
                    parts.push(format!("CHARGE_MAH={:.0}/{:.0}", now, full));
                }
            }

            // Voltage
            if let Some(v) = supply.voltage_v() {
                if human {
                    parts.push(format!("VOLTAGE={}", format_voltage(v, true)));
                } else {
                    parts.push(format!("VOLTAGE_V={:.2}", v));
                }
            }

            // Current
            if let Some(c) = supply.current_a() {
                if human {
                    parts.push(format!("CURRENT={}", format_current(c, true)));
                } else {
                    parts.push(format!("CURRENT_A={:.3}", c));
                }
            }

            // Power
            if let Some(p) = supply.power_w() {
                if human {
                    parts.push(format!("POWER={}", format_power(p, true)));
                } else {
                    parts.push(format!("POWER_W={:.2}", p));
                }
            }

            // USB PD negotiated limits
            if let Some(v) = supply.voltage_max_v() {
                if human {
                    parts.push(format!("VOLTAGE_MAX={}", format_voltage(v, true)));
                } else {
                    parts.push(format!("VOLTAGE_MAX_V={:.2}", v));
                }
            }
            if let Some(c) = supply.current_max_a() {
                if human {
                    parts.push(format!("CURRENT_MAX={}", format_current(c, true)));
                } else {
                    parts.push(format!("CURRENT_MAX_A={:.2}", c));
                }
            }

            // Battery metadata
            if let Some(cycles) = supply.cycle_count {
                parts.push(format!("CYCLES={}", cycles));
            }
            if let Some(ref tech) = supply.technology {
                parts.push(format!("TECHNOLOGY={}", tech));
            }
            if let Some(ref model) = supply.model_name {
                parts.push(format!("MODEL=\"{}\"", model));
            }
            if let Some(ref mfr) = supply.manufacturer {
                parts.push(format!("MANUFACTURER=\"{}\"", mfr));
            }
        }

        println!("{}", parts.join(" "));
    }
}

/// Print power supplies as JSON.
fn print_json(supplies: &[PowerSupply], pretty: bool, verbose: bool) {
    let mut w = begin_kv_output(pretty, "power");

    w.field_array("data");

    for supply in supplies {
        w.array_object_begin();

        w.field_str("name", &supply.name);

        if let Some(ref t) = supply.supply_type {
            w.field_str("type", t);
        }

        if let Some(ref status) = supply.status {
            w.field_str("status", status);
        }

        if let Some(online) = supply.online {
            w.field_bool("online", online == 1);
        }

        if let Some(cap) = supply.capacity {
            w.field_u64("capacity_percent", cap as u64);
        }

        if let Some(ref usb_type) = supply.usb_type {
            w.field_str("usb_type", usb_type);
        }

        if verbose {
            // Electrical measurements
            if let Some(v) = supply.voltage_uv {
                w.field_i64("voltage_uv", v);
            }
            if let Some(c) = supply.current_ua {
                w.field_i64("current_ua", c);
            }
            if let Some(p) = supply.power_uw {
                w.field_i64("power_uw", p);
            }

            // Energy/charge
            if let Some(e) = supply.energy_now_uwh {
                w.field_i64("energy_now_uwh", e);
            }
            if let Some(e) = supply.energy_full_uwh {
                w.field_i64("energy_full_uwh", e);
            }
            if let Some(c) = supply.charge_now_uah {
                w.field_i64("charge_now_uah", c);
            }
            if let Some(c) = supply.charge_full_uah {
                w.field_i64("charge_full_uah", c);
            }

            // USB PD limits
            if let Some(v) = supply.voltage_max_uv {
                w.field_i64("voltage_max_uv", v);
            }
            if let Some(c) = supply.current_max_ua {
                w.field_i64("current_max_ua", c);
            }

            // Battery metadata
            if let Some(cycles) = supply.cycle_count {
                w.field_i64("cycle_count", cycles as i64);
            }
            if let Some(ref tech) = supply.technology {
                w.field_str("technology", tech);
            }
            if let Some(ref model) = supply.model_name {
                w.field_str("model_name", model);
            }
            if let Some(ref mfr) = supply.manufacturer {
                w.field_str("manufacturer", mfr);
            }
        }

        w.array_object_end();
    }

    w.end_field_array();
    w.end_object();

    println!("{}", w.finish());
}

/// Entry point for `kv power` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    let supplies = read_power_supplies();

    // Apply filter if specified
    let supplies: Vec<_> = if let Some(ref pattern) = opts.filter {
        supplies
            .into_iter()
            .filter(|s| s.matches_filter(pattern, opts.filter_case_insensitive))
            .collect()
    } else {
        supplies
    };

    if supplies.is_empty() {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "power");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else if opts.filter.is_some() {
            println!("power: no matching power supplies");
        } else {
            println!("power: no power supplies found in {}", POWER_SUPPLY_PATH);
        }
        return 0;
    }

    if opts.json {
        print_json(&supplies, opts.pretty, opts.verbose);
    } else {
        print_text(&supplies, opts.verbose, opts.human);
    }

    0
}

/// Collect power info for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect(_verbose: bool) -> Vec<PowerSupply> {
    read_power_supplies()
}

/// Write power info to a JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json(w: &mut JsonWriter, supplies: &[PowerSupply], verbose: bool) {
    if supplies.is_empty() {
        return; // Omit empty sections in snapshot
    }

    w.field_array("power");

    for supply in supplies {
        w.array_object_begin();

        w.field_str("name", &supply.name);

        if let Some(ref t) = supply.supply_type {
            w.field_str("type", t);
        }

        if let Some(ref status) = supply.status {
            w.field_str("status", status);
        }

        if let Some(online) = supply.online {
            w.field_bool("online", online == 1);
        }

        if let Some(cap) = supply.capacity {
            w.field_u64("capacity_percent", cap as u64);
        }

        if let Some(ref usb_type) = supply.usb_type {
            w.field_str("usb_type", usb_type);
        }

        if verbose {
            if let Some(v) = supply.voltage_uv {
                w.field_i64("voltage_uv", v);
            }
            if let Some(c) = supply.current_ua {
                w.field_i64("current_ua", c);
            }
            if let Some(p) = supply.power_uw {
                w.field_i64("power_uw", p);
            }
            if let Some(e) = supply.energy_now_uwh {
                w.field_i64("energy_now_uwh", e);
            }
            if let Some(e) = supply.energy_full_uwh {
                w.field_i64("energy_full_uwh", e);
            }
            if let Some(cycles) = supply.cycle_count {
                w.field_i64("cycle_count", cycles as i64);
            }
            if let Some(ref tech) = supply.technology {
                w.field_str("technology", tech);
            }
            if let Some(ref model) = supply.model_name {
                w.field_str("model_name", model);
            }
        }

        w.array_object_end();
    }

    w.end_field_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_supply() -> PowerSupply {
        PowerSupply {
            name: "BAT0".to_string(),
            supply_type: Some("Battery".to_string()),
            status: Some("Discharging".to_string()),
            online: None,
            capacity: Some(85),
            voltage_uv: Some(12_500_000),
            current_ua: Some(-1_500_000),
            power_uw: Some(18_750_000),
            usb_type: None,
            energy_now_uwh: Some(45_000_000),
            energy_full_uwh: Some(50_000_000),
            charge_now_uah: None,
            charge_full_uah: None,
            cycle_count: Some(150),
            technology: Some("Li-ion".to_string()),
            manufacturer: Some("LG".to_string()),
            model_name: Some("DELL ABC123".to_string()),
            current_max_ua: None,
            voltage_max_uv: None,
        }
    }

    #[test]
    fn parse_usb_type_bracketed() {
        assert_eq!(parse_usb_type("[C] PD PD_PPS"), "C");
        assert_eq!(parse_usb_type("C [PD] PD_PPS"), "PD");
        assert_eq!(parse_usb_type("C PD [PD_PPS]"), "PD_PPS");
    }

    #[test]
    fn parse_usb_type_no_brackets() {
        assert_eq!(parse_usb_type("Unknown"), "Unknown");
        assert_eq!(parse_usb_type("  SDP  "), "SDP");
    }

    #[test]
    fn voltage_conversion() {
        let supply = make_test_supply();
        assert_eq!(supply.voltage_v(), Some(12.5));
        assert_eq!(supply.current_a(), Some(-1.5));
        assert_eq!(supply.power_w(), Some(18.75));
    }

    #[test]
    fn energy_conversion() {
        let supply = make_test_supply();
        assert_eq!(supply.energy_now_wh(), Some(45.0));
        assert_eq!(supply.energy_full_wh(), Some(50.0));
    }

    #[test]
    fn format_helpers() {
        assert_eq!(format_voltage(12.5, true), "12.5V");
        assert_eq!(format_voltage(12.5, false), "12.50");
        assert_eq!(format_current(1.5, true), "1.5A");
        assert_eq!(format_current(0.5, true), "500mA");
        assert_eq!(format_power(18.75, true), "18.8W");
        assert_eq!(format_energy(45.0, true), "45.0Wh");
    }
}
