//! kv - Kernel View
//!
//! A tiny, dependency-free system inspector for embedded Linux.
//! See README.md for full documentation.

#![no_std]
#![no_main]

// Force link origin to get startup code and mem functions
extern crate origin;

mod cli;
#[macro_use]
mod debug;
mod fields;
mod filter;
mod io;
mod json;
mod print;
mod stack;

// Subcommand modules - conditionally compiled based on features.
// For now, we only enable mem for the no_std conversion.

#[cfg(feature = "mem")]
mod mem;

// Stub modules for disabled features
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
#[cfg(feature = "mounts")]
mod mounts;
#[cfg(feature = "thermal")]
mod thermal;
#[cfg(feature = "power")]
mod power;
#[cfg(feature = "snapshot")]
mod snapshot;

#[cfg(all(
    feature = "dt",
    any(target_arch = "arm", target_arch = "aarch64", target_arch = "riscv64", target_arch = "powerpc64", target_arch = "mips")
))]
mod dt;

#[cfg(all(
    feature = "dt",
    not(any(target_arch = "arm", target_arch = "aarch64", target_arch = "riscv64", target_arch = "powerpc64", target_arch = "mips"))
))]
mod dt {
    pub fn run(_opts: &crate::cli::GlobalOptions, _args: &crate::cli::ExtraArgs) -> i32 {
        crate::print::println("dt: devicetree not typically available on this architecture");
        0
    }
}

use cli::{Invocation, print_help, print_version, print_subcommand_help};

/// Panic handler - minimal, just exits
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    // In release builds, just exit immediately
    rustix::runtime::exit_group(101)
}

/// Entry point called by origin.
/// Origin calls this after performing program initialization.
#[unsafe(no_mangle)]
unsafe fn origin_main(argc: usize, argv: *mut *mut u8, _envp: *mut *mut u8) -> i32 {
    // SAFETY: origin guarantees argc/argv are valid
    let inv = unsafe { Invocation::parse_from_raw(argc as i32, argv as *const *const u8) };
    run(inv)
}

fn run(inv: Invocation) -> i32 {

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
        print::eprintln("Error: no subcommand specified");
        print::eprintln_empty();
        print::eprintln("Run 'kv --help' for usage information.");
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

        _unknown => {
            print::eprintln("Error: unknown subcommand");
            print::eprintln_empty();
            print::eprintln("Run 'kv --help' for a list of available subcommands.");
            1
        }
    }
}
