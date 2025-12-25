//! Thermal sensor information from /sys/class/thermal/ and /sys/class/hwmon/.
//!
//! Linux exposes thermal data via two subsystems:
//!
//! 1. thermal_zone - High-level thermal management with named zones
//!    (like "cpu-thermal", "gpu-thermal"). Common on ARM/embedded.
//!
//! 2. hwmon - Hardware monitoring with raw sensor data. Common on x86
//!    (coretemp, k10temp, it87, etc.) and provides more detail.
//!
//! We also expose cooling devices (fans, CPU frequency scaling, throttle
//! alerts) and trip points (temperature thresholds that trigger actions).
//!
//! Temperature is reported in millidegrees Celsius - divide by 1000
//! for the human-readable value. We keep it in millidegrees for precision.

use crate::cli::GlobalOptions;
use crate::fields::{thermal as f, to_text_key};
use crate::filter::{opt_str, Filterable};
use crate::io::{read_dir_names_sorted, read_file_string};
use crate::json::{begin_kv_output, JsonWriter};
use std::path::Path;

const THERMAL_PATH: &str = "/sys/class/thermal";
const HWMON_PATH: &str = "/sys/class/hwmon";

/// A temperature trip point - threshold that triggers thermal action.
#[derive(Debug, Clone)]
pub struct TripPoint {
    /// Trip point index (0, 1, 2, ...)
    pub index: u32,
    /// Temperature threshold in millidegrees Celsius
    pub temp_millicelsius: i64,
    /// Trip type: "critical", "hot", "passive", "active"
    pub trip_type: String,
}

impl TripPoint {
    /// Temperature in degrees Celsius.
    pub fn temp_celsius(&self) -> f64 {
        self.temp_millicelsius as f64 / 1000.0
    }
}

/// A cooling device - fan, CPU frequency scaling, throttle alert, etc.
#[derive(Debug, Clone)]
pub struct CoolingDevice {
    /// Device name (e.g., "cooling_device0")
    pub name: String,
    /// Device type (e.g., "pwm-fan", "cpufreq-cpu0", "gpu-throttle-alert")
    pub device_type: String,
    /// Current cooling state (0 = off/max freq, higher = more cooling/throttling)
    pub cur_state: u32,
    /// Maximum cooling state
    pub max_state: u32,
}

impl CoolingDevice {
    /// Read cooling device info from /sys/class/thermal.
    fn read(base: &Path, name: &str) -> Option<Self> {
        let path = base.join(name);
        if !path.is_dir() || !name.starts_with("cooling_device") {
            return None;
        }

        let device_type = read_file_string(path.join("type"))?;
        let cur_state = read_file_string(path.join("cur_state"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let max_state = read_file_string(path.join("max_state"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        Some(CoolingDevice {
            name: name.to_string(),
            device_type,
            cur_state,
            max_state,
        })
    }

}

impl Filterable for CoolingDevice {
    fn filter_fields(&self) -> Vec<&str> {
        vec![&self.name, &self.device_type]
    }
}

/// Source of thermal data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThermalSource {
    /// From /sys/class/thermal/thermal_zone*
    ThermalZone,
    /// From /sys/class/hwmon/hwmon*/temp*_input
    Hwmon,
}

/// Information about a single thermal sensor.
#[derive(Debug, Clone)]
pub struct ThermalZone {
    /// Zone/sensor name (e.g., "thermal_zone0", "hwmon0")
    pub name: String,
    /// Type/label (e.g., "cpu-thermal", "coretemp")
    pub zone_type: Option<String>,
    /// Sensor label (for hwmon, e.g., "Core 0", "Package id 0")
    pub label: Option<String>,
    /// Current temperature in millidegrees Celsius
    pub temp_millicelsius: Option<i64>,
    /// Policy in use (e.g., "step_wise") - thermal zones only
    pub policy: Option<String>,
    /// Critical temperature threshold in millidegrees
    pub temp_crit: Option<i64>,
    /// Trip points (temperature thresholds)
    pub trip_points: Vec<TripPoint>,
    /// Source of this reading
    pub source: ThermalSource,
}

impl ThermalZone {
    /// Read thermal zone info from /sys/class/thermal.
    fn read_thermal_zone(base: &Path, name: &str) -> Option<Self> {
        let path = base.join(name);
        if !path.is_dir() {
            return None;
        }

        // Only process thermal_zone* entries, skip cooling_device*
        if !name.starts_with("thermal_zone") {
            return None;
        }

        let zone_type = read_file_string(path.join("type"));
        let temp_str = read_file_string(path.join("temp"));
        let policy = read_file_string(path.join("policy"));

        // Parse temperature - some zones may not have a readable temp
        let temp_millicelsius = temp_str.and_then(|s| s.parse::<i64>().ok());

        // Read trip points
        let trip_points = Self::read_trip_points(&path);

        // Extract critical temp from trip points if available
        let temp_crit = trip_points
            .iter()
            .find(|tp| tp.trip_type == "critical")
            .map(|tp| tp.temp_millicelsius);

        Some(ThermalZone {
            name: name.to_string(),
            zone_type,
            label: None,
            temp_millicelsius,
            policy,
            temp_crit,
            trip_points,
            source: ThermalSource::ThermalZone,
        })
    }

    /// Read trip points from a thermal zone directory.
    ///
    /// Trip points are numbered starting from 0. We stop scanning after
    /// 2 consecutive missing trip points to reduce debug noise on systems
    /// with only a few trip points (like RPi4 with just trip_point_0).
    fn read_trip_points(zone_path: &Path) -> Vec<TripPoint> {
        let mut trip_points = Vec::new();
        let mut consecutive_misses = 0;

        for i in 0..16 {
            let temp_file = zone_path.join(format!("trip_point_{}_temp", i));
            let type_file = zone_path.join(format!("trip_point_{}_type", i));

            if let (Some(temp_str), Some(trip_type)) =
                (read_file_string(&temp_file), read_file_string(&type_file))
            {
                if let Ok(temp) = temp_str.parse::<i64>() {
                    trip_points.push(TripPoint {
                        index: i,
                        temp_millicelsius: temp,
                        trip_type,
                    });
                    consecutive_misses = 0;
                    continue;
                }
            }

            // Trip point not found - track consecutive misses
            consecutive_misses += 1;
            if consecutive_misses >= 2 {
                // Stop scanning after 2 consecutive missing trip points
                break;
            }
        }

        trip_points
    }

    /// Temperature in degrees Celsius (for display).
    pub fn temp_celsius(&self) -> Option<f64> {
        self.temp_millicelsius.map(|t| t as f64 / 1000.0)
    }

    /// Critical temperature in degrees Celsius.
    pub fn temp_crit_celsius(&self) -> Option<f64> {
        self.temp_crit.map(|t| t as f64 / 1000.0)
    }

}

impl Filterable for ThermalZone {
    fn filter_fields(&self) -> Vec<&str> {
        vec![&self.name, opt_str(&self.zone_type), opt_str(&self.label)]
    }
}

/// Read temperature sensors from a hwmon device.
/// Returns multiple sensors if the device has multiple temp inputs.
fn read_hwmon_temps(base: &Path, name: &str) -> Vec<ThermalZone> {
    let path = base.join(name);
    if !path.is_dir() {
        return Vec::new();
    }

    let hwmon_name = read_file_string(path.join("name"));

    // Look for temp*_input files (temp1_input, temp2_input, etc.)
    let mut sensors = Vec::new();

    // Check up to 16 possible temperature inputs
    for i in 1..=16 {
        let temp_file = path.join(format!("temp{}_input", i));
        if let Some(temp_str) = read_file_string(&temp_file) {
            if let Ok(temp) = temp_str.parse::<i64>() {
                // Read optional label (e.g., "Core 0", "Package id 0")
                let label = read_file_string(path.join(format!("temp{}_label", i)));

                // Read optional critical temperature
                let temp_crit = read_file_string(path.join(format!("temp{}_crit", i)))
                    .and_then(|s| s.parse::<i64>().ok());

                // Create a descriptive name
                let sensor_name = if i == 1 {
                    name.to_string()
                } else {
                    format!("{}:{}", name, i)
                };

                sensors.push(ThermalZone {
                    name: sensor_name,
                    zone_type: hwmon_name.clone(),
                    label,
                    temp_millicelsius: Some(temp),
                    policy: None,
                    temp_crit,
                    trip_points: Vec::new(), // hwmon doesn't have trip points
                    source: ThermalSource::Hwmon,
                });
            }
        }
    }

    sensors
}

/// Read all thermal sensors from the system.
/// Prefers thermal_zone subsystem, falls back to hwmon if no zones found.
pub fn read_thermal_zones() -> Vec<ThermalZone> {
    // First try thermal zones
    let zones = read_thermal_zones_from(Path::new(THERMAL_PATH));
    if !zones.is_empty() {
        return zones;
    }

    // Fall back to hwmon
    read_hwmon_sensors_from(Path::new(HWMON_PATH))
}

/// Read thermal zones from /sys/class/thermal.
pub fn read_thermal_zones_from(base: &Path) -> Vec<ThermalZone> {
    let names = read_dir_names_sorted(base);
    names
        .iter()
        .filter_map(|name| ThermalZone::read_thermal_zone(base, name))
        .collect()
}

/// Read temperature sensors from /sys/class/hwmon.
pub fn read_hwmon_sensors_from(base: &Path) -> Vec<ThermalZone> {
    let names = read_dir_names_sorted(base);
    names
        .iter()
        .flat_map(|name| read_hwmon_temps(base, name))
        .collect()
}

/// Read all cooling devices from the system.
pub fn read_cooling_devices() -> Vec<CoolingDevice> {
    read_cooling_devices_from(Path::new(THERMAL_PATH))
}

/// Read cooling devices from /sys/class/thermal.
pub fn read_cooling_devices_from(base: &Path) -> Vec<CoolingDevice> {
    let names = read_dir_names_sorted(base);
    names
        .iter()
        .filter_map(|name| CoolingDevice::read(base, name))
        .collect()
}

/// Format temperature for display.
fn format_temp(temp: f64, human: bool) -> String {
    if human {
        format!("{:.1}°C", temp)
    } else {
        format!("{:.1}", temp)
    }
}

/// Print thermal zones as text.
fn print_text(zones: &[ThermalZone], cooling: &[CoolingDevice], verbose: bool, human: bool) {
    // Print temperature sensors
    for zone in zones {
        let mut parts = Vec::new();

        // Use type as the primary identifier, fallback to zone name
        let type_str = zone.zone_type.as_deref().unwrap_or(&zone.name);
        parts.push(format!("{}={}", to_text_key(f::SENSOR), type_str));

        // For hwmon with labels, show the label (e.g., "Core 0")
        if let Some(ref label) = zone.label {
            parts.push(format!("{}=\"{}\"", to_text_key(f::LABEL), label));
        }

        if let Some(temp) = zone.temp_celsius() {
            parts.push(format!("{}={}", to_text_key(f::TEMP), format_temp(temp, human)));
        }

        if verbose {
            if let Some(crit) = zone.temp_crit_celsius() {
                parts.push(format!("{}={}", to_text_key(f::CRIT), format_temp(crit, human)));
            }
            // Show trip points in verbose mode
            if !zone.trip_points.is_empty() {
                let trips: Vec<String> = zone
                    .trip_points
                    .iter()
                    .map(|tp| format!("{}:{}", tp.trip_type, format_temp(tp.temp_celsius(), human)))
                    .collect();
                parts.push(format!("{}={}", to_text_key(f::TRIPS), trips.join(",")));
            }
            if let Some(ref policy) = zone.policy {
                parts.push(format!("{}={}", to_text_key(f::POLICY), policy));
            }
            // Show source in verbose mode
            let source = match zone.source {
                ThermalSource::ThermalZone => "thermal",
                ThermalSource::Hwmon => "hwmon",
            };
            parts.push(format!("{}={}", to_text_key(f::SOURCE), source));
        }

        println!("{}", parts.join(" "));
    }

    // Print cooling devices in verbose mode
    if verbose && !cooling.is_empty() {
        for dev in cooling {
            let state_info = if dev.max_state > 0 {
                format!("{}={}/{}", to_text_key(f::STATE), dev.cur_state, dev.max_state)
            } else {
                format!("{}={}", to_text_key(f::STATE), dev.cur_state)
            };
            println!("{}={} {}", to_text_key(f::COOLING), dev.device_type, state_info);
        }
    }
}

/// Print thermal zones as JSON.
fn print_json(zones: &[ThermalZone], cooling: &[CoolingDevice], pretty: bool, verbose: bool) {
    let mut w = begin_kv_output(pretty, "thermal");

    w.field_array("sensors");

    for zone in zones {
        w.array_object_begin();

        if let Some(ref t) = zone.zone_type {
            w.field_str(f::SENSOR, t);
        } else {
            w.field_str(f::SENSOR, &zone.name);
        }

        if let Some(ref label) = zone.label {
            w.field_str(f::LABEL, label);
        }

        if let Some(temp) = zone.temp_millicelsius {
            w.field_i64(f::TEMP_MILLICELSIUS, temp);
        }

        if verbose {
            w.field_str(f::NAME, &zone.name);
            if let Some(crit) = zone.temp_crit {
                w.field_i64(f::TEMP_CRIT_MILLICELSIUS, crit);
            }
            // Include trip points in verbose JSON
            if !zone.trip_points.is_empty() {
                w.field_array(f::TRIP_POINTS);
                for tp in &zone.trip_points {
                    w.array_object_begin();
                    w.field_u64(f::INDEX, tp.index as u64);
                    w.field_str(f::TYPE, &tp.trip_type);
                    w.field_i64(f::TEMP_MILLICELSIUS, tp.temp_millicelsius);
                    w.array_object_end();
                }
                w.end_field_array();
            }
            if let Some(ref policy) = zone.policy {
                w.field_str(f::POLICY, policy);
            }
            let source = match zone.source {
                ThermalSource::ThermalZone => "thermal",
                ThermalSource::Hwmon => "hwmon",
            };
            w.field_str(f::SOURCE, source);
        }

        w.array_object_end();
    }

    w.end_field_array();

    // Include cooling devices in verbose mode
    if verbose && !cooling.is_empty() {
        w.field_array(f::COOLING);
        for dev in cooling {
            w.array_object_begin();
            w.field_str(f::TYPE, &dev.device_type);
            w.field_u64(f::CUR_STATE, dev.cur_state as u64);
            w.field_u64(f::MAX_STATE, dev.max_state as u64);
            w.field_str(f::NAME, &dev.name);
            w.array_object_end();
        }
        w.end_field_array();
    }

    w.end_object();

    println!("{}", w.finish());
}

/// Entry point for `kv thermal` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    let zones = read_thermal_zones();
    let cooling = if opts.verbose {
        read_cooling_devices()
    } else {
        Vec::new()
    };

    // Apply filter if specified
    let zones: Vec<_> = if let Some(ref pattern) = opts.filter {
        zones
            .into_iter()
            .filter(|z| z.matches_filter(pattern, opts.filter_case_insensitive))
            .collect()
    } else {
        zones
    };

    let cooling: Vec<_> = if let Some(ref pattern) = opts.filter {
        cooling
            .into_iter()
            .filter(|c| c.matches_filter(pattern, opts.filter_case_insensitive))
            .collect()
    } else {
        cooling
    };

    if zones.is_empty() && cooling.is_empty() {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "thermal");
            w.field_array("sensors");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else if opts.filter.is_some() {
            println!("thermal: no matching sensors");
        } else {
            println!("thermal: no temperature sensors found");
        }
        return 0;
    }

    if opts.json {
        print_json(&zones, &cooling, opts.pretty, opts.verbose);
    } else {
        print_text(&zones, &cooling, opts.verbose, opts.human);
    }

    0
}

/// Collect thermal info for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect(_verbose: bool) -> Vec<ThermalZone> {
    read_thermal_zones()
}

/// Write thermal info to a JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json(w: &mut JsonWriter, zones: &[ThermalZone], verbose: bool) {
    if zones.is_empty() {
        return; // Omit empty sections in snapshot
    }

    w.field_array("thermal");

    for zone in zones {
        w.array_object_begin();

        if let Some(ref t) = zone.zone_type {
            w.field_str(f::SENSOR, t);
        } else {
            w.field_str(f::SENSOR, &zone.name);
        }

        if let Some(ref label) = zone.label {
            w.field_str(f::LABEL, label);
        }

        if let Some(temp) = zone.temp_millicelsius {
            w.field_i64(f::TEMP_MILLICELSIUS, temp);
        }

        if verbose {
            w.field_str(f::NAME, &zone.name);
            if let Some(crit) = zone.temp_crit {
                w.field_i64(f::TEMP_CRIT_MILLICELSIUS, crit);
            }
            if let Some(ref policy) = zone.policy {
                w.field_str(f::POLICY, policy);
            }
        }

        w.array_object_end();
    }

    w.end_field_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_conversion() {
        let zone = ThermalZone {
            name: "thermal_zone0".to_string(),
            zone_type: Some("cpu-thermal".to_string()),
            label: None,
            temp_millicelsius: Some(44500),
            policy: None,
            temp_crit: Some(105000),
            trip_points: vec![
                TripPoint {
                    index: 0,
                    temp_millicelsius: 99000,
                    trip_type: "passive".to_string(),
                },
                TripPoint {
                    index: 1,
                    temp_millicelsius: 105000,
                    trip_type: "critical".to_string(),
                },
            ],
            source: ThermalSource::ThermalZone,
        };

        assert_eq!(zone.temp_celsius(), Some(44.5));
        assert_eq!(zone.temp_crit_celsius(), Some(105.0));
        assert_eq!(zone.trip_points.len(), 2);
        assert_eq!(zone.trip_points[0].temp_celsius(), 99.0);
    }

    #[test]
    fn temp_conversion_none() {
        let zone = ThermalZone {
            name: "thermal_zone0".to_string(),
            zone_type: Some("cpu-thermal".to_string()),
            label: None,
            temp_millicelsius: None,
            policy: None,
            temp_crit: None,
            trip_points: Vec::new(),
            source: ThermalSource::ThermalZone,
        };

        assert_eq!(zone.temp_celsius(), None);
        assert_eq!(zone.temp_crit_celsius(), None);
    }

    #[test]
    fn hwmon_sensor() {
        let zone = ThermalZone {
            name: "hwmon0".to_string(),
            zone_type: Some("coretemp".to_string()),
            label: Some("Core 0".to_string()),
            temp_millicelsius: Some(52000),
            policy: None,
            temp_crit: Some(100000),
            trip_points: Vec::new(),
            source: ThermalSource::Hwmon,
        };

        assert_eq!(zone.temp_celsius(), Some(52.0));
        assert_eq!(zone.label.as_deref(), Some("Core 0"));
        assert_eq!(zone.source, ThermalSource::Hwmon);
    }

    #[test]
    fn cooling_device() {
        let dev = CoolingDevice {
            name: "cooling_device0".to_string(),
            device_type: "cpufreq-cpu0".to_string(),
            cur_state: 0,
            max_state: 28,
        };

        assert_eq!(dev.device_type, "cpufreq-cpu0");
        assert_eq!(dev.cur_state, 0);
        assert_eq!(dev.max_state, 28);
    }

    #[test]
    fn cooling_device_filter() {
        let dev = CoolingDevice {
            name: "cooling_device3".to_string(),
            device_type: "pwm-fan".to_string(),
            cur_state: 1,
            max_state: 2,
        };

        assert!(dev.matches_filter("fan", false));
        // Pattern must be lowercased for case-insensitive mode (CLI does this)
        assert!(dev.matches_filter("fan", true));
        assert!(!dev.matches_filter("cpu", false));
    }
}
