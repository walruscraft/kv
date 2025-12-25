//! kv - Kernel View
//!
//! A tiny, dependency-free system inspector for embedded Linux.
//! See README.md for full documentation.

mod cli;
#[macro_use]
mod debug;
mod fields;
mod filter;
mod io;
mod json;

// Subcommand modules - conditionally compiled based on features.
// If you don't need USB support, don't compile it. Simple.

#[cfg(feature = "pci")]
mod pci;

#[cfg(feature = "usb")]
mod usb;

#[cfg(feature = "block")]
mod block;

#[cfg(feature = "net")]
mod net;

#[cfg(feature = "cpu")]
mod cpu;

#[cfg(feature = "mem")]
mod mem;

#[cfg(feature = "mounts")]
mod mounts;

#[cfg(feature = "thermal")]
mod thermal;

#[cfg(feature = "power")]
mod power;

// dt module is special - only compile on architectures where devicetree makes sense
#[cfg(all(
    feature = "dt",
    any(target_arch = "arm", target_arch = "aarch64", target_arch = "riscv64")
))]
mod dt;

// For x86 with dt feature, we still compile a stub that gracefully reports no DT
#[cfg(all(
    feature = "dt",
    not(any(target_arch = "arm", target_arch = "aarch64", target_arch = "riscv64"))
))]
mod dt {
    pub fn run(_opts: &crate::cli::GlobalOptions, _args: &[String]) -> i32 {
        // On x86, devicetree is basically never used (except some odd UEFI cases)
        println!("dt: devicetree not typically available on this architecture");
        0
    }
}

#[cfg(feature = "snapshot")]
mod snapshot;

use cli::{Invocation, print_help, print_version, print_subcommand_help};

fn main() {
    let exit_code = run();
    std::process::exit(exit_code);
}

fn run() -> i32 {
    let inv = Invocation::parse();

    // Initialize debug mode from CLI flag (env var is checked during parse)
    debug::set_enabled(inv.options.debug);

    if inv.options.debug {
        dbg_print!("kv {} starting", env!("CARGO_PKG_VERSION"));
        dbg_print!("subcommand: {:?}", inv.subcommand);
    }

    // Handle version request
    if inv.wants_version() {
        print_version();
        return 0;
    }

    // Handle help request
    if inv.wants_help() {
        match inv.help_subject() {
            Some(subcmd) => print_subcommand_help(subcmd),
            None => print_help(),
        }
        return 0;
    }

    // No subcommand? Print usage and exit with error.
    let Some(ref subcommand) = inv.subcommand else {
        eprintln!("Error: no subcommand specified");
        eprintln!();
        eprintln!("Run 'kv --help' for usage information.");
        return 1;
    };

    // Dispatch to the appropriate subcommand.
    // Each match arm is conditionally compiled - if feature is off, it's not here.
    match subcommand.as_str() {
        #[cfg(feature = "pci")]
        "pci" => pci::run(&inv.options),

        #[cfg(feature = "usb")]
        "usb" => usb::run(&inv.options),

        #[cfg(feature = "block")]
        "block" => block::run(&inv.options),

        #[cfg(feature = "net")]
        "net" => net::run(&inv.options),

        #[cfg(feature = "cpu")]
        "cpu" => cpu::run(&inv.options),

        #[cfg(feature = "mem")]
        "mem" => mem::run(&inv.options),

        #[cfg(feature = "mounts")]
        "mounts" => mounts::run(&inv.options),

        #[cfg(feature = "thermal")]
        "thermal" => thermal::run(&inv.options),

        #[cfg(feature = "power")]
        "power" => power::run(&inv.options),

        #[cfg(feature = "dt")]
        "dt" => dt::run(&inv.options, &inv.args),

        #[cfg(feature = "snapshot")]
        "snapshot" => snapshot::run(&inv.options),

        unknown => {
            eprintln!("Error: unknown subcommand '{}'", unknown);
            eprintln!();

            // Be helpful - maybe it's a feature that wasn't compiled in?
            let maybe_disabled = matches!(
                unknown,
                "pci" | "usb" | "block" | "net" | "cpu" | "mem" | "mounts" | "thermal" | "power" | "dt" | "snapshot"
            );

            if maybe_disabled {
                eprintln!(
                    "Note: '{}' might be disabled in this build. Run 'kv --version' to see enabled features.",
                    unknown
                );
            } else {
                eprintln!("Run 'kv --help' for a list of available subcommands.");
            }
            1
        }
    }
}
