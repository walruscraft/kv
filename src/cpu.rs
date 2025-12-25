//! CPU information from /proc/cpuinfo and /sys/devices/system/cpu.
//!
//! Parsing /proc/cpuinfo is a bit of an adventure because the format varies
//! between architectures:
//!   - x86: "model name", "vendor_id", "flags" (huge list), "cpu MHz"
//!   - ARM: "CPU implementer", "CPU part", "Features", "Hardware"
//!   - RISC-V: "isa" (e.g., rv64imafdvcsu), "mmu" (e.g., sv39)
//!
//! We do our best to provide useful information regardless of architecture,
//! but some fields may be missing on some platforms. That's life in embedded.

use crate::cli::GlobalOptions;
use crate::fields::{cpu as f, to_text_key};
use crate::io;
use crate::json::{begin_kv_output, JsonWriter};
use std::collections::HashSet;

const CPUINFO_PATH: &str = "/proc/cpuinfo";
// Future: could use /sys/devices/system/cpu for more topology info
#[allow(dead_code)]
const CPU_SYSFS_PATH: &str = "/sys/devices/system/cpu";

/// CPU information structure.
#[derive(Debug, Default)]
pub struct CpuInfo {
    /// Number of logical CPUs (threads)
    pub logical_cpus: u32,
    /// Model name (x86) or CPU part description
    pub model_name: Option<String>,
    /// Vendor ID (GenuineIntel, AuthenticAMD, ARM, etc.)
    pub vendor_id: Option<String>,
    /// CPU family (x86)
    pub cpu_family: Option<u32>,
    /// Model number (x86)
    pub model: Option<u32>,
    /// Stepping (x86)
    pub stepping: Option<u32>,
    /// MHz (may vary per core, we take the first)
    pub cpu_mhz: Option<f64>,
    /// Cache size (x86, usually L2 or L3)
    pub cache_size: Option<String>,
    /// Number of physical cores per socket
    pub cores_per_socket: Option<u32>,
    /// Number of sockets (physical packages)
    pub sockets: Option<u32>,
    /// CPU flags/features (verbose only, can be huge)
    pub flags: Option<String>,
    /// Architecture (from uname or inferred)
    pub architecture: Option<String>,
    /// RISC-V ISA string (e.g., "rv64imafdvcsu")
    pub isa: Option<String>,
    /// RISC-V MMU type (e.g., "sv39")
    pub mmu: Option<String>,
}

impl CpuInfo {
    /// Read CPU information from /proc/cpuinfo.
    pub fn read() -> Option<Self> {
        let contents = io::read_file_string(CPUINFO_PATH)?;
        Some(Self::parse(&contents))
    }

    /// Parse /proc/cpuinfo content.
    ///
    /// The format is a series of "key : value" lines, with blank lines
    /// separating logical CPUs. We parse all of them but extract common
    /// fields from the first CPU for simplicity.
    pub fn parse(content: &str) -> Self {
        let mut info = CpuInfo::default();
        let mut logical_cpus = 0u32;
        let mut physical_ids = HashSet::new();
        let mut core_ids = HashSet::new();

        // Parse all CPUs but primarily use first CPU's detailed info
        let mut first_cpu = true;
        let mut current_block_has_processor = false;

        for line in content.lines() {
            if line.trim().is_empty() {
                // End of a CPU block - only count if we saw "processor" key
                if current_block_has_processor {
                    logical_cpus += 1;
                    first_cpu = false;
                }
                current_block_has_processor = false;
                continue;
            }

            let Some((key, value)) = parse_cpuinfo_line(line) else {
                continue;
            };

            // Track if this block is a real CPU (has "processor" key)
            if key == "processor" {
                current_block_has_processor = true;
            }

            // Track physical IDs and core IDs for topology
            if key == "physical id" {
                if let Ok(id) = value.parse::<u32>() {
                    physical_ids.insert(id);
                }
            }
            if key == "core id" {
                if let Ok(id) = value.parse::<u32>() {
                    core_ids.insert(id);
                }
            }

            // Hardware line appears outside CPU blocks on ARM - always use it if present
            // as it's more readable ("Raspberry Pi 4" vs "ARM Part 0xd08")
            if key == "Hardware" {
                info.model_name = Some(value.to_string());
                continue;
            }

            // Extract info from first CPU only (avoid overwriting with data from CPU 1, 2, etc.)
            if first_cpu || info.model_name.is_none() {
                match key {
                    "model name" => info.model_name = Some(value.to_string()),
                    "vendor_id" => info.vendor_id = Some(value.to_string()),
                    "cpu family" => info.cpu_family = value.parse().ok(),
                    "model" => info.model = value.parse().ok(),
                    "stepping" => info.stepping = value.parse().ok(),
                    "cpu MHz" => info.cpu_mhz = value.parse().ok(),
                    "cache size" => info.cache_size = Some(value.to_string()),
                    "flags" | "Features" => info.flags = Some(value.to_string()),
                    // ARM-specific
                    "CPU implementer" => {
                        if info.vendor_id.is_none() {
                            info.vendor_id = Some(format!("ARM ({})", value));
                        }
                    }
                    "CPU part" => {
                        if info.model_name.is_none() {
                            info.model_name = Some(format!("ARM Part {}", value));
                        }
                    }
                    // RISC-V specific
                    "isa" => info.isa = Some(value.to_string()),
                    "mmu" => info.mmu = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        // Don't forget the last CPU if file doesn't end with blank line
        if current_block_has_processor {
            logical_cpus += 1;
        }

        info.logical_cpus = logical_cpus;

        // Calculate sockets and cores per socket from physical/core IDs
        if !physical_ids.is_empty() {
            info.sockets = Some(physical_ids.len() as u32);
            if !core_ids.is_empty() && !physical_ids.is_empty() {
                info.cores_per_socket = Some(core_ids.len() as u32);
            }
        }

        // Try to detect architecture
        info.architecture = detect_architecture();

        info
    }

    /// Output as text (KEY=VALUE format).
    pub fn print_text(&self, verbose: bool) {
        let mut parts = Vec::new();

        parts.push(format!("{}={}", to_text_key(f::LOGICAL_CPUS), self.logical_cpus));

        if let Some(ref name) = self.model_name {
            // Quote model name since it often has spaces
            parts.push(format!("{}=\"{}\"", to_text_key(f::MODEL_NAME), name));
        }

        if let Some(ref vendor) = self.vendor_id {
            parts.push(format!("{}={}", to_text_key(f::VENDOR_ID), vendor));
        }

        if let Some(sockets) = self.sockets {
            parts.push(format!("{}={}", to_text_key(f::SOCKETS), sockets));
        }

        if let Some(cores) = self.cores_per_socket {
            parts.push(format!("{}={}", to_text_key(f::CORES_PER_SOCKET), cores));
        }

        // RISC-V specific fields (always show if present, very informative)
        if let Some(ref isa) = self.isa {
            parts.push(format!("{}={}", to_text_key(f::ISA), isa));
        }
        if let Some(ref mmu) = self.mmu {
            parts.push(format!("{}={}", to_text_key(f::MMU), mmu));
        }

        if verbose {
            if let Some(family) = self.cpu_family {
                parts.push(format!("{}={}", to_text_key(f::CPU_FAMILY), family));
            }
            if let Some(model) = self.model {
                parts.push(format!("{}={}", to_text_key(f::MODEL), model));
            }
            if let Some(stepping) = self.stepping {
                parts.push(format!("{}={}", to_text_key(f::STEPPING), stepping));
            }
            if let Some(mhz) = self.cpu_mhz {
                parts.push(format!("{}={:.2}", to_text_key(f::CPU_MHZ), mhz));
            }
            if let Some(ref cache) = self.cache_size {
                parts.push(format!("{}=\"{}\"", to_text_key(f::CACHE_SIZE), cache));
            }
            if let Some(ref arch) = self.architecture {
                parts.push(format!("{}={}", to_text_key(f::ARCHITECTURE), arch));
            }
        }

        println!("{}", parts.join(" "));
    }

    /// Output as JSON.
    pub fn print_json(&self, pretty: bool, verbose: bool) {
        let mut w = begin_kv_output(pretty, "cpu");

        w.field_object("data");

        w.field_u64(f::LOGICAL_CPUS, self.logical_cpus as u64);
        w.field_str_opt(f::MODEL_NAME, self.model_name.as_deref());
        w.field_str_opt(f::VENDOR_ID, self.vendor_id.as_deref());
        w.field_u64_opt(f::SOCKETS, self.sockets.map(|v| v as u64));
        w.field_u64_opt(f::CORES_PER_SOCKET, self.cores_per_socket.map(|v| v as u64));
        // RISC-V specific
        w.field_str_opt(f::ISA, self.isa.as_deref());
        w.field_str_opt(f::MMU, self.mmu.as_deref());

        if verbose {
            w.field_u64_opt(f::CPU_FAMILY, self.cpu_family.map(|v| v as u64));
            w.field_u64_opt(f::MODEL, self.model.map(|v| v as u64));
            w.field_u64_opt(f::STEPPING, self.stepping.map(|v| v as u64));
            // For MHz we'll use string to preserve precision
            if let Some(mhz) = self.cpu_mhz {
                w.field_str(f::CPU_MHZ, &format!("{:.2}", mhz));
            }
            w.field_str_opt(f::CACHE_SIZE, self.cache_size.as_deref());
            w.field_str_opt(f::ARCHITECTURE, self.architecture.as_deref());
            w.field_str_opt(f::FLAGS, self.flags.as_deref());
        }

        w.end_field_object();
        w.end_object();

        println!("{}", w.finish());
    }
}

/// Parse a single line from /proc/cpuinfo.
///
/// Format: "key		: value" (with variable whitespace)
fn parse_cpuinfo_line(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim()))
}

/// Try to detect the CPU architecture.
fn detect_architecture() -> Option<String> {
    // First try reading from /sys
    if let Some(arch) = io::read_file_string("/sys/devices/system/cpu/cpu0/topology/arch") {
        return Some(arch);
    }

    // Fall back to compile-time arch (not ideal but better than nothing)
    #[cfg(target_arch = "x86_64")]
    return Some("x86_64".to_string());

    #[cfg(target_arch = "x86")]
    return Some("x86".to_string());

    #[cfg(target_arch = "aarch64")]
    return Some("aarch64".to_string());

    #[cfg(target_arch = "arm")]
    return Some("arm".to_string());

    #[cfg(target_arch = "riscv64")]
    return Some("riscv64".to_string());

    #[cfg(not(any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv64"
    )))]
    None
}

/// Entry point for `kv cpu` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    let Some(info) = CpuInfo::read() else {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "cpu");
            w.key("data");
            w.value_null();
            w.key("error");
            w.value_string("cannot read /proc/cpuinfo");
            w.end_object();
            println!("{}", w.finish());
        } else {
            println!("cpu: cannot read {}", CPUINFO_PATH);
        }
        return 0;
    };

    if opts.json {
        info.print_json(opts.pretty, opts.verbose);
    } else {
        info.print_text(opts.verbose);
    }

    0
}

/// Collect CPU info for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect(_verbose: bool) -> Option<CpuInfo> {
    CpuInfo::read()
}

/// Write CPU info to a JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json(w: &mut JsonWriter, info: &CpuInfo, verbose: bool) {
    w.field_object("cpu");

    w.field_u64(f::LOGICAL_CPUS, info.logical_cpus as u64);
    w.field_str_opt(f::MODEL_NAME, info.model_name.as_deref());
    w.field_str_opt(f::VENDOR_ID, info.vendor_id.as_deref());
    w.field_u64_opt(f::SOCKETS, info.sockets.map(|v| v as u64));
    w.field_u64_opt(f::CORES_PER_SOCKET, info.cores_per_socket.map(|v| v as u64));
    // RISC-V specific
    w.field_str_opt(f::ISA, info.isa.as_deref());
    w.field_str_opt(f::MMU, info.mmu.as_deref());

    if verbose {
        w.field_u64_opt(f::CPU_FAMILY, info.cpu_family.map(|v| v as u64));
        w.field_u64_opt(f::MODEL, info.model.map(|v| v as u64));
        w.field_u64_opt(f::STEPPING, info.stepping.map(|v| v as u64));
        if let Some(mhz) = info.cpu_mhz {
            w.field_str(f::CPU_MHZ, &format!("{:.2}", mhz));
        }
        w.field_str_opt(f::CACHE_SIZE, info.cache_size.as_deref());
        w.field_str_opt(f::ARCHITECTURE, info.architecture.as_deref());
    }

    w.end_field_object();
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_CPUINFO_X86: &str = r#"processor	: 0
vendor_id	: GenuineIntel
cpu family	: 6
model		: 158
model name	: Intel(R) Core(TM) i7-8700 CPU @ 3.20GHz
stepping	: 10
cpu MHz		: 3191.998
cache size	: 12288 KB
physical id	: 0
siblings	: 12
core id		: 0
cpu cores	: 6
flags		: fpu vme de pse tsc msr pae mce cx8 apic sep

processor	: 1
vendor_id	: GenuineIntel
cpu family	: 6
model		: 158
model name	: Intel(R) Core(TM) i7-8700 CPU @ 3.20GHz
stepping	: 10
cpu MHz		: 3191.998
cache size	: 12288 KB
physical id	: 0
siblings	: 12
core id		: 1
cpu cores	: 6
flags		: fpu vme de pse tsc msr pae mce cx8 apic sep
"#;

    const SAMPLE_CPUINFO_ARM: &str = r#"processor	: 0
BogoMIPS	: 48.00
Features	: fp asimd evtstrm aes pmull sha1 sha2 crc32 cpuid
CPU implementer	: 0x41
CPU architecture: 8
CPU variant	: 0x0
CPU part	: 0xd08
CPU revision	: 3

processor	: 1
BogoMIPS	: 48.00
Features	: fp asimd evtstrm aes pmull sha1 sha2 crc32 cpuid
CPU implementer	: 0x41
CPU architecture: 8
CPU variant	: 0x0
CPU part	: 0xd08
CPU revision	: 3

Hardware	: Raspberry Pi 4 Model B Rev 1.2
"#;

    const SAMPLE_CPUINFO_RISCV: &str = r#"processor	: 0
hart		: 0
isa		: rv64imafdvcsu
mmu		: sv39

"#;

    #[test]
    fn parse_x86_cpuinfo() {
        let info = CpuInfo::parse(SAMPLE_CPUINFO_X86);

        assert_eq!(info.logical_cpus, 2);
        assert_eq!(info.vendor_id, Some("GenuineIntel".to_string()));
        assert!(info.model_name.as_ref().unwrap().contains("i7-8700"));
        assert_eq!(info.cpu_family, Some(6));
        assert_eq!(info.model, Some(158));
        assert_eq!(info.stepping, Some(10));
        assert!(info.cpu_mhz.is_some());
        assert_eq!(info.cache_size, Some("12288 KB".to_string()));
        assert_eq!(info.sockets, Some(1)); // Only one physical id
    }

    #[test]
    fn parse_arm_cpuinfo() {
        let info = CpuInfo::parse(SAMPLE_CPUINFO_ARM);

        assert_eq!(info.logical_cpus, 2);
        assert!(info.vendor_id.as_ref().unwrap().contains("ARM"));
        assert!(info.flags.is_some()); // Features becomes flags
        // Hardware line should be picked up as model name
        assert!(info.model_name.as_ref().unwrap().contains("Raspberry Pi"));
    }

    #[test]
    fn parse_riscv_cpuinfo() {
        let info = CpuInfo::parse(SAMPLE_CPUINFO_RISCV);

        assert_eq!(info.logical_cpus, 1);
        assert_eq!(info.isa, Some("rv64imafdvcsu".to_string()));
        assert_eq!(info.mmu, Some("sv39".to_string()));
        // RISC-V doesn't have model_name or vendor_id in cpuinfo
        assert!(info.model_name.is_none());
        assert!(info.vendor_id.is_none());
    }

    #[test]
    fn parse_single_line() {
        let result = parse_cpuinfo_line("model name	: Intel(R) Core(TM)");
        assert_eq!(result, Some(("model name", "Intel(R) Core(TM)")));
    }

    #[test]
    fn parse_empty_value() {
        let result = parse_cpuinfo_line("flags		:");
        assert_eq!(result, Some(("flags", "")));
    }
}
