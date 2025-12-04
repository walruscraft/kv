//! Combined snapshot of all system information.
//!
//! This is the "give me everything" command. It outputs all available
//! system information in a single JSON object. Useful for:
//!
//! - Saving system state for later analysis
//! - Comparing systems ("why does this board work but that one doesn't?")
//! - Automated inventory collection
//!
//! Note: This always outputs JSON. If you want text output, run the
//! individual subcommands instead.

use crate::cli::GlobalOptions;
use crate::json::begin_kv_output;

/// Entry point for `kv snapshot` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    // Snapshot always outputs JSON
    let pretty = opts.pretty;
    let verbose = opts.verbose;

    let mut w = begin_kv_output(pretty, "snapshot");

    w.field_object("data");

    // Collect and write each section if the feature is enabled AND data exists.
    // Per our design decision: omit keys entirely for missing runtime data.

    #[cfg(feature = "pci")]
    {
        let devices = crate::pci::collect();
        if !devices.is_empty() {
            crate::pci::write_json_snapshot(&mut w, &devices, verbose);
        }
    }

    #[cfg(feature = "usb")]
    {
        let devices = crate::usb::collect();
        if !devices.is_empty() {
            crate::usb::write_json_snapshot(&mut w, &devices, verbose);
        }
    }

    #[cfg(feature = "block")]
    {
        let devices = crate::block::collect();
        if !devices.is_empty() {
            crate::block::write_json_snapshot(&mut w, &devices, verbose);
        }
    }

    #[cfg(feature = "net")]
    {
        let interfaces = crate::net::collect();
        if !interfaces.is_empty() {
            crate::net::write_json_snapshot(&mut w, &interfaces, verbose);
        }
    }

    #[cfg(feature = "cpu")]
    {
        if let Some(info) = crate::cpu::collect(verbose) {
            crate::cpu::write_json(&mut w, &info, verbose);
        }
    }

    #[cfg(feature = "mem")]
    {
        if let Some(info) = crate::mem::collect(verbose) {
            crate::mem::write_json(&mut w, &info, verbose);
        }
    }

    #[cfg(feature = "mounts")]
    {
        let mounts = crate::mounts::collect();
        if !mounts.is_empty() {
            crate::mounts::write_json(&mut w, &mounts, verbose);
        }
    }

    #[cfg(feature = "thermal")]
    {
        let zones = crate::thermal::collect(verbose);
        if !zones.is_empty() {
            crate::thermal::write_json(&mut w, &zones, verbose);
        }
    }

    #[cfg(feature = "power")]
    {
        let supplies = crate::power::collect(verbose);
        if !supplies.is_empty() {
            crate::power::write_json(&mut w, &supplies, verbose);
        }
    }

    // dt is special - architecture gated
    #[cfg(all(
        feature = "dt",
        any(target_arch = "arm", target_arch = "aarch64", target_arch = "riscv64")
    ))]
    {
        if let Some(nodes) = crate::dt::collect() {
            crate::dt::write_json_snapshot(&mut w, &nodes, verbose);
        }
    }

    w.end_field_object();
    w.end_object();

    println!("{}", w.finish());

    0
}
