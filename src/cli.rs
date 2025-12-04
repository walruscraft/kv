//! Command-line argument parsing for kv.
//!
//! We roll our own because external crates are forbidden, and honestly,
//! our needs are simple enough that clap would be overkill anyway.
//!
//! The basic pattern:
//! - First positional arg (after program name) is the subcommand
//! - Everything else is flags/options for that subcommand
//! - Global flags like --json, --pretty, --verbose apply to all subcommands

use std::env;

// =============================================================================
// Input Safety Limits
// =============================================================================

/// Maximum length for filter patterns (defense against memory exhaustion).
/// 1024 chars is plenty for any reasonable substring match.
const MAX_FILTER_LEN: usize = 1024;

/// Sanitize a filter pattern: truncate if too long, warn user.
fn sanitize_filter_pattern(pattern: &str) -> String {
    if pattern.len() > MAX_FILTER_LEN {
        eprintln!(
            "Warning: filter pattern truncated to {} characters",
            MAX_FILTER_LEN
        );
        // Truncate at char boundary to avoid splitting UTF-8
        pattern.chars().take(MAX_FILTER_LEN).collect()
    } else {
        pattern.to_string()
    }
}

/// Global options that apply to all subcommands.
#[derive(Debug, Clone, Default)]
pub struct GlobalOptions {
    /// Output as JSON instead of text
    pub json: bool,
    /// Pretty-print JSON (only meaningful with json=true)
    pub pretty: bool,
    /// Verbose output - show extra fields
    pub verbose: bool,
    /// Human-readable output (e.g., "1.5G" instead of bytes)
    pub human: bool,
    /// Show help
    pub help: bool,
    /// Filter pattern (case-sensitive if lowercase, insensitive if uppercase)
    pub filter: Option<String>,
    /// Whether filter is case-insensitive (-F vs -f)
    pub filter_case_insensitive: bool,
    /// Debug mode - show file access and parse errors
    pub debug: bool,
}

impl GlobalOptions {
    /// Parse global options from an iterator of arguments.
    ///
    /// Returns the options and any remaining arguments that weren't recognized
    /// as global flags.
    pub fn parse<I, S>(args: I) -> (Self, Vec<String>)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut opts = GlobalOptions::default();
        let mut remaining = Vec::new();
        let mut args_iter = args.into_iter().peekable();

        // Check KV_DEBUG environment variable first
        if env::var("KV_DEBUG").is_ok() {
            opts.debug = true;
        }

        while let Some(arg) = args_iter.next() {
            let arg = arg.as_ref();
            match arg {
                "-j" | "--json" => opts.json = true,
                "-p" | "--pretty" => opts.pretty = true,
                "-v" | "--verbose" => opts.verbose = true,
                "-h" | "--human" => opts.human = true,
                "-H" | "--help" => opts.help = true,
                "-D" | "--debug" => opts.debug = true,

                // Filter with value: -f <pattern> (case-sensitive) or -F <pattern> (case-insensitive)
                "-f" | "--filter" => {
                    if let Some(pattern) = args_iter.next() {
                        opts.filter = Some(sanitize_filter_pattern(pattern.as_ref()));
                        opts.filter_case_insensitive = false;
                    }
                }
                "-F" | "--ifilter" => {
                    if let Some(pattern) = args_iter.next() {
                        // Lowercase once here so matches_filter() implementations don't have to
                        let pattern = sanitize_filter_pattern(pattern.as_ref()).to_lowercase();
                        opts.filter = Some(pattern);
                        opts.filter_case_insensitive = true;
                    }
                }

                // Combo flags like -jp or -jpv (but not if contains 'f' or 'F')
                s if s.starts_with('-') && !s.starts_with("--") && s.len() > 2 => {
                    let chars: Vec<char> = s[1..].chars().collect();
                    // If 'f' or 'F' is in combo flags, we can't handle it (needs value)
                    // So pass through as remaining
                    if chars.contains(&'f') || chars.contains(&'F') {
                        remaining.push(arg.to_string());
                    } else {
                        for c in chars {
                            match c {
                                'j' => opts.json = true,
                                'p' => opts.pretty = true,
                                'v' => opts.verbose = true,
                                'h' => opts.human = true,
                                'H' => opts.help = true,
                                'D' => opts.debug = true,
                                _ => {
                                    // Unknown short flag - pass through
                                    remaining.push(arg.to_string());
                                    break;
                                }
                            }
                        }
                    }
                }

                _ => remaining.push(arg.to_string()),
            }
        }

        (opts, remaining)
    }
}

/// The parsed command-line invocation.
#[derive(Debug)]
pub struct Invocation {
    /// The subcommand to run (pci, usb, block, etc.), if any
    pub subcommand: Option<String>,
    /// Global options
    pub options: GlobalOptions,
    /// Remaining arguments for the subcommand
    pub args: Vec<String>,
}

impl Invocation {
    /// Parse command-line arguments into an Invocation.
    ///
    /// Handles the full parsing pipeline:
    /// 1. Skip program name
    /// 2. Extract subcommand (first non-flag argument)
    /// 3. Parse global options from remaining args
    pub fn parse() -> Self {
        Self::parse_from(env::args())
    }

    /// Parse from a custom iterator (useful for testing).
    pub fn parse_from<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str> + Into<String>,
    {
        let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();

        // Skip program name
        let args = if args.is_empty() { vec![] } else { args[1..].to_vec() };

        // Handle special cases first
        if args.is_empty() {
            return Invocation {
                subcommand: None,
                options: GlobalOptions::default(),
                args: vec![],
            };
        }

        // Check for top-level --help, --version, or help subcommand
        let first = &args[0];
        if first == "--help" || first == "-H" || first == "--version" || first == "-V" {
            let mut opts = GlobalOptions::default();
            if first == "--help" || first == "-H" {
                opts.help = true;
            }
            return Invocation {
                subcommand: Some(first.clone()),
                options: opts,
                args: args[1..].to_vec(),
            };
        }

        // Check for "help <subcommand>" pattern
        if first == "help" {
            return Invocation {
                subcommand: Some("help".to_string()),
                options: GlobalOptions { help: true, ..Default::default() },
                args: args[1..].to_vec(),
            };
        }

        // First non-flag argument is the subcommand
        let subcommand = if first.starts_with('-') {
            None
        } else {
            Some(first.clone())
        };

        // Parse options from remaining args
        let remaining = if subcommand.is_some() {
            args[1..].to_vec()
        } else {
            args
        };

        let (options, args) = GlobalOptions::parse(remaining);

        Invocation {
            subcommand,
            options,
            args,
        }
    }

    /// Check if help was requested (either via flag or "help" subcommand).
    pub fn wants_help(&self) -> bool {
        self.options.help || self.subcommand.as_deref() == Some("help")
    }

    /// Check if version was requested.
    pub fn wants_version(&self) -> bool {
        matches!(self.subcommand.as_deref(), Some("--version") | Some("-V"))
    }

    /// Get the subcommand to show help for, if any.
    ///
    /// Returns the subject of the help request:
    /// - `kv help pci` -> Some("pci")
    /// - `kv pci -H` -> Some("pci")
    /// - `kv --help` -> None (show main help)
    pub fn help_subject(&self) -> Option<&str> {
        // "kv help pci" - subject is in args
        if let Some(subcmd) = self.args.first() {
            return Some(subcmd);
        }
        // "kv pci -H" - subject is subcommand (unless it's "help" itself)
        if let Some(ref subcmd) = self.subcommand {
            if subcmd != "help" && subcmd != "--help" && subcmd != "-H" {
                return Some(subcmd);
            }
        }
        None
    }
}

/// Print the main help text.
///
/// Only lists subcommands that are actually compiled in (via features).
pub fn print_help() {
    println!("{}\n", env!("CARGO_PKG_DESCRIPTION"));
    print!(concat!(
        "USAGE:\n",
        "    kv <SUBCOMMAND> [OPTIONS]\n",
        "\n",
        "OPTIONS:\n",
        "    -j, --json        Output as JSON\n",
        "    -p, --pretty      Pretty-print JSON (use with -j)\n",
        "    -v, --verbose     Show additional fields (most commands, see -H)\n",
        "    -h, --human       Human-readable sizes (1K, 2.5M, 3G)\n",
        "    -f <pattern>      Filter output (case-sensitive)\n",
        "    -F <pattern>      Filter output (case-insensitive)\n",
        "    -D, --debug       Show debug info (file access, parse errors)\n",
        "    -H, --help        Show help (use 'kv <cmd> -H' for subcommand details)\n",
        "    -V, --version     Show version and compiled features\n",
        "\n",
        "SUBCOMMANDS:\n",
    ));

    // Only print subcommands that are compiled in.
    #[cfg(feature = "pci")]
    print!("    pci        Show PCI devices\n");
    #[cfg(feature = "usb")]
    print!("    usb        Show USB devices\n");
    #[cfg(feature = "block")]
    print!("    block      Show block devices and partitions\n");
    #[cfg(feature = "net")]
    print!("    net        Show network interfaces\n");
    #[cfg(feature = "cpu")]
    print!("    cpu        Show CPU information\n");
    #[cfg(feature = "mem")]
    print!("    mem        Show memory information\n");
    #[cfg(feature = "mounts")]
    print!("    mounts     Show mounted filesystems\n");
    #[cfg(feature = "thermal")]
    print!("    thermal    Show temperature sensors\n");
    #[cfg(feature = "power")]
    print!("    power      Show power supplies/batteries\n");
    #[cfg(feature = "dt")]
    print!("    dt         Show devicetree nodes (use -H for dt-specific options)\n");
    #[cfg(feature = "snapshot")]
    print!("    snapshot   Combined JSON dump of all info\n");

    print!(concat!(
        "\n",
        "ENVIRONMENT:\n",
        "    KV_DEBUG=1    Enable debug mode (same as -D)\n",
        "\n",
        "EXIT CODES:\n",
        "    0    Success (even if some data unavailable)\n",
        "    1    Error (bad arguments, severe I/O failure)\n",
        "\n",
        "EXAMPLES:\n",
        "    kv pci                # List PCI devices\n",
        "    kv pci -jph           # As pretty JSON with human-readable sizes\n",
        "    kv net -f wlP         # Network interfaces containing exactly 'wlP'\n",
        "    kv net -F up          # Same, case-insensitive\n",
        "    kv snapshot           # Everything, as JSON\n",
        "    KV_DEBUG=1 kv mem     # With debug output\n",
    ));
}

/// Print version information including compiled features.
pub fn print_version() {
    println!("kv {}", env!("CARGO_PKG_VERSION"));

    // Collect compiled-in features
    let mut features: Vec<&str> = Vec::new();

    #[cfg(feature = "pci")]
    features.push("pci");
    #[cfg(feature = "usb")]
    features.push("usb");
    #[cfg(feature = "block")]
    features.push("block");
    #[cfg(feature = "net")]
    features.push("net");
    #[cfg(feature = "cpu")]
    features.push("cpu");
    #[cfg(feature = "mem")]
    features.push("mem");
    #[cfg(feature = "mounts")]
    features.push("mounts");
    #[cfg(feature = "thermal")]
    features.push("thermal");
    #[cfg(feature = "power")]
    features.push("power");
    #[cfg(feature = "dt")]
    features.push("dt");
    #[cfg(feature = "snapshot")]
    features.push("snapshot");

    if features.is_empty() {
        println!("features: (none)");
    } else {
        println!("features: {}", features.join(", "));
    }

    // Print target triple if we know it
    #[cfg(target_arch = "x86_64")]
    println!("arch: x86_64");
    #[cfg(target_arch = "aarch64")]
    println!("arch: aarch64");
    #[cfg(target_arch = "arm")]
    println!("arch: arm");
    #[cfg(target_arch = "riscv64")]
    println!("arch: riscv64");
}

/// Print help for a specific subcommand.
pub fn print_subcommand_help(subcommand: &str) {
    match subcommand {
        #[cfg(feature = "pci")]
        "pci" => print!(concat!(
            "kv pci - Show PCI devices\n\n",
            "Reads PCI device information from /sys/bus/pci/devices/\n\n",
            "FIELDS (default):\n",
            "    bdf            Bus:Device.Function address\n",
            "    vendor_id      PCI vendor ID\n",
            "    device_id      PCI device ID\n",
            "    class          Device class code\n",
            "    driver         Bound driver name (if any)\n\n",
            "FIELDS (verbose):\n",
            "    subsystem_vendor_id, subsystem_device_id\n",
            "    numa_node, iommu_group\n",
        )),

        #[cfg(feature = "usb")]
        "usb" => print!(concat!(
            "kv usb - Show USB devices\n\n",
            "Reads USB device information from /sys/bus/usb/devices/\n",
            "Filters out root hub entries for cleaner output.\n",
        )),

        #[cfg(feature = "block")]
        "block" => print!(concat!(
            "kv block - Show block devices and partitions\n\n",
            "Reads block device information from /sys/block/\n",
            "Associates partitions with their parent disks.\n",
        )),

        #[cfg(feature = "net")]
        "net" => print!(concat!(
            "kv net - Show network interfaces\n\n",
            "Reads network interface information from /sys/class/net/\n",
        )),

        #[cfg(feature = "cpu")]
        "cpu" => print!(concat!(
            "kv cpu - Show CPU information\n\n",
            "Reads CPU information from /proc/cpuinfo and /sys/devices/system/cpu/\n",
        )),

        #[cfg(feature = "mem")]
        "mem" => print!(concat!(
            "kv mem - Show memory information\n\n",
            "Reads memory information from /proc/meminfo\n\n",
            "FIELDS:\n",
            "    mem_total_kb      Total physical memory\n",
            "    mem_free_kb       Free memory\n",
            "    mem_available_kb  Available memory (free + reclaimable)\n",
            "    swap_total_kb     Total swap space\n",
            "    swap_free_kb      Free swap space\n",
        )),

        #[cfg(feature = "mounts")]
        "mounts" => print!(concat!(
            "kv mounts - Show mounted filesystems\n\n",
            "Reads mount information from /proc/self/mounts\n",
        )),

        #[cfg(feature = "thermal")]
        "thermal" => print!(concat!(
            "kv thermal - Show temperature sensors\n\n",
            "Reads thermal data from /sys/class/thermal/ (thermal zones)\n",
            "or /sys/class/hwmon/ (hardware monitors) as fallback.\n\n",
            "FIELDS:\n",
            "    sensor     Sensor type (cpu-thermal, coretemp, etc.)\n",
            "    label      Sensor label (Core 0, Package, etc.) - hwmon only\n",
            "    temp_c     Current temperature in Celsius\n\n",
            "FIELDS (verbose):\n",
            "    crit_c     Critical temperature threshold\n",
            "    policy     Thermal policy (step_wise, etc.)\n",
            "    source     Data source (thermal or hwmon)\n",
        )),

        #[cfg(feature = "power")]
        "power" => print!(concat!(
            "kv power - Show power supplies and batteries\n\n",
            "Reads power supply info from /sys/class/power_supply/\n\n",
            "TYPES:\n",
            "    Battery    Laptop/device batteries\n",
            "    Mains      AC adapters\n",
            "    USB        USB power delivery sources\n",
            "    UPS        Uninterruptible power supplies\n\n",
            "FIELDS (verbose):\n",
            "    voltage_v, current_a, power_w\n",
        )),

        #[cfg(feature = "dt")]
        "dt" => print!(concat!(
            "kv dt - Show devicetree nodes\n\n",
            "USAGE:\n",
            "    kv dt                  Show board model/compatible + node count\n",
            "    kv dt -v               List all nodes\n",
            "    kv dt /soc/uart@1000   Show specific node with all properties\n",
            "    kv dt -f <pattern>     Filter nodes by path or compatible\n",
            "    kv dt -d               Show only disabled nodes\n\n",
            "DT-SPECIFIC OPTIONS:\n",
            "    -d, --disabled      Show only nodes with status != okay\n\n",
            "Reads devicetree from /sys/firmware/devicetree/base/\n",
            "NOTE: Only available on systems with devicetree (ARM, RISC-V)\n",
        )),

        #[cfg(feature = "snapshot")]
        "snapshot" => print!(concat!(
            "kv snapshot - Combined JSON dump\n\n",
            "Outputs all available system information as a single JSON object.\n",
            "Always outputs JSON (--json is implied).\n\n",
            "Use --pretty for human-readable formatting.\n",
        )),

        _ => {
            eprintln!("Unknown subcommand: {}", subcommand);
            eprintln!("Run 'kv --help' for a list of subcommands.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod global_options_tests {
        use super::*;

        #[test]
        fn empty_args() {
            let (opts, remaining) = GlobalOptions::parse::<[&str; 0], &str>([]);
            assert!(!opts.json);
            assert!(!opts.pretty);
            assert!(!opts.verbose);
            assert!(remaining.is_empty());
        }

        #[test]
        fn json_flag_long() {
            let (opts, _) = GlobalOptions::parse(["--json"]);
            assert!(opts.json);
        }

        #[test]
        fn json_flag_short() {
            let (opts, _) = GlobalOptions::parse(["-j"]);
            assert!(opts.json);
        }

        #[test]
        fn combined_short_flags() {
            let (opts, _) = GlobalOptions::parse(["-jpv"]);
            assert!(opts.json);
            assert!(opts.pretty);
            assert!(opts.verbose);
        }

        #[test]
        fn mixed_flags() {
            let (opts, _) = GlobalOptions::parse(["--json", "-p", "--verbose"]);
            assert!(opts.json);
            assert!(opts.pretty);
            assert!(opts.verbose);
        }

        #[test]
        fn non_flag_args_passed_through() {
            let (opts, remaining) = GlobalOptions::parse(["--json", "extra", "args"]);
            assert!(opts.json);
            assert_eq!(remaining, vec!["extra", "args"]);
        }

        #[test]
        fn filter_pattern_truncated_when_too_long() {
            // Create a pattern longer than MAX_FILTER_LEN
            let long_pattern: String = "x".repeat(2000);
            let (opts, _) = GlobalOptions::parse(["-f", &long_pattern]);
            assert!(opts.filter.is_some());
            let filter = opts.filter.unwrap();
            assert_eq!(filter.len(), super::MAX_FILTER_LEN);
            assert!(filter.chars().all(|c| c == 'x'));
        }

        #[test]
        fn filter_pattern_preserved_when_short() {
            let (opts, _) = GlobalOptions::parse(["-f", "short"]);
            assert_eq!(opts.filter, Some("short".to_string()));
        }
    }

    mod invocation_tests {
        use super::*;

        #[test]
        fn no_args() {
            let inv = Invocation::parse_from(["kv"]);
            assert!(inv.subcommand.is_none());
        }

        #[test]
        fn simple_subcommand() {
            let inv = Invocation::parse_from(["kv", "pci"]);
            assert_eq!(inv.subcommand.as_deref(), Some("pci"));
        }

        #[test]
        fn subcommand_with_flags() {
            let inv = Invocation::parse_from(["kv", "pci", "--json", "-v"]);
            assert_eq!(inv.subcommand.as_deref(), Some("pci"));
            assert!(inv.options.json);
            assert!(inv.options.verbose);
        }

        #[test]
        fn help_flag_long() {
            let inv = Invocation::parse_from(["kv", "--help"]);
            assert!(inv.wants_help());
        }

        #[test]
        fn help_flag_short() {
            let inv = Invocation::parse_from(["kv", "-H"]);
            assert!(inv.wants_help());
        }

        #[test]
        fn human_flag() {
            let inv = Invocation::parse_from(["kv", "mem", "-h"]);
            assert!(inv.options.human);
            assert!(!inv.options.help);
        }

        #[test]
        fn help_subcommand() {
            let inv = Invocation::parse_from(["kv", "help"]);
            assert!(inv.wants_help());
        }

        #[test]
        fn help_for_subcommand() {
            let inv = Invocation::parse_from(["kv", "help", "pci"]);
            assert!(inv.wants_help());
            assert_eq!(inv.args, vec!["pci"]);
        }

        #[test]
        fn version_flag() {
            let inv = Invocation::parse_from(["kv", "--version"]);
            assert!(inv.wants_version());
        }

        #[test]
        fn version_short() {
            let inv = Invocation::parse_from(["kv", "-V"]);
            assert!(inv.wants_version());
        }
    }
}
