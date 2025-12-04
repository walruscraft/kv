//! Integration tests for kv
//!
//! These tests run the actual binary against the real /sys and /proc.
//! They verify output format and exit codes, not specific values
//! (since those vary by system).

use std::process::Command;

fn kv() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kv"))
}

// Helper to run kv and check success
fn run_kv(args: &[&str]) -> (bool, String, String) {
    let output = kv().args(args).output().expect("failed to execute kv");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    (output.status.success(), stdout, stderr)
}

#[test]
fn version_flag() {
    let (ok, stdout, _) = run_kv(&["--version"]);
    assert!(ok);
    assert!(stdout.contains("kv "));
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_flag() {
    let (ok, stdout, _) = run_kv(&["--help"]);
    assert!(ok);
    assert!(stdout.contains("USAGE:"));
    assert!(stdout.contains("SUBCOMMANDS:"));
}

#[test]
fn no_args_shows_error() {
    let (ok, _, stderr) = run_kv(&[]);
    assert!(!ok);
    assert!(stderr.contains("no subcommand"));
}

#[test]
fn unknown_subcommand() {
    let (ok, _, stderr) = run_kv(&["notreal"]);
    assert!(!ok);
    assert!(stderr.contains("unknown subcommand"));
}

// Subcommand tests - these read real system data

#[test]
fn mem_text_output() {
    let (ok, stdout, _) = run_kv(&["mem"]);
    assert!(ok);
    assert!(stdout.contains("MEM_TOTAL_KB="));
}

#[test]
fn mem_json_output() {
    let (ok, stdout, _) = run_kv(&["mem", "-j"]);
    assert!(ok);
    assert!(stdout.contains("\"kv_version\""));
    assert!(stdout.contains("\"subcommand\":\"mem\""));
    assert!(stdout.contains("\"mem_total_kb\""));
}

#[test]
fn mem_pretty_json() {
    let (ok, stdout, _) = run_kv(&["mem", "-jp"]);
    assert!(ok);
    // Pretty JSON has newlines and indentation
    assert!(stdout.contains("\n"));
    assert!(stdout.contains("  "));
}

#[test]
fn cpu_runs() {
    let (ok, stdout, _) = run_kv(&["cpu"]);
    assert!(ok);
    // Should have CPU info
    assert!(stdout.contains("LOGICAL_CPUS=") || stdout.contains("cpu:"));
}

#[test]
fn cpu_json() {
    let (ok, stdout, _) = run_kv(&["cpu", "-j"]);
    assert!(ok);
    assert!(stdout.contains("\"subcommand\":\"cpu\""));
}

#[test]
fn block_runs() {
    let (ok, _stdout, _) = run_kv(&["block"]);
    assert!(ok);
    // Even minimal systems have some block device (loop, ram)
    // But might be empty in containers, so just check it doesn't error
}

#[test]
fn block_json() {
    let (ok, stdout, _) = run_kv(&["block", "-j"]);
    assert!(ok);
    assert!(stdout.contains("\"subcommand\":\"block\""));
}

#[test]
fn net_runs() {
    let (ok, stdout, _) = run_kv(&["net"]);
    assert!(ok);
    // lo (loopback) should exist on any Linux system
    assert!(stdout.contains("lo") || stdout.contains("IFACE="));
}

#[test]
fn net_json() {
    let (ok, stdout, _) = run_kv(&["net", "-j"]);
    assert!(ok);
    assert!(stdout.contains("\"subcommand\":\"net\""));
}

#[test]
fn mounts_runs() {
    let (ok, stdout, _) = run_kv(&["mounts"]);
    assert!(ok);
    // Should have mount info (TARGET= is the field name)
    assert!(stdout.contains("TARGET=") || stdout.contains("mounts:"));
}

#[test]
fn mounts_json() {
    let (ok, stdout, _) = run_kv(&["mounts", "-j"]);
    assert!(ok);
    assert!(stdout.contains("\"subcommand\":\"mounts\""));
}

#[test]
fn pci_runs() {
    let (ok, _, _) = run_kv(&["pci"]);
    // PCI might not exist (containers, some VMs), but shouldn't error
    assert!(ok);
}

#[test]
fn usb_runs() {
    let (ok, _, _) = run_kv(&["usb"]);
    // USB might not exist (WSL, containers), but shouldn't error
    assert!(ok);
}

#[test]
fn snapshot_json() {
    let (ok, stdout, _) = run_kv(&["snapshot"]);
    assert!(ok);
    // Snapshot is always JSON
    assert!(stdout.contains("\"kv_version\""));
    assert!(stdout.contains("\"subcommand\":\"snapshot\""));
}

#[test]
fn combined_flags() {
    // Test that -jpv works (combined short flags)
    let (ok, stdout, _) = run_kv(&["mem", "-jpv"]);
    assert!(ok);
    assert!(stdout.contains("\n")); // pretty
    assert!(stdout.contains("\"mem_total_kb\"")); // json
}

#[test]
fn verbose_adds_fields() {
    let (ok_normal, stdout_normal, _) = run_kv(&["mem"]);
    let (ok_verbose, stdout_verbose, _) = run_kv(&["mem", "-v"]);
    assert!(ok_normal);
    assert!(ok_verbose);
    // Verbose output should be longer (more fields)
    assert!(stdout_verbose.len() >= stdout_normal.len());
}

#[test]
fn human_readable_mem() {
    let (ok, stdout, _) = run_kv(&["mem", "-h"]);
    assert!(ok);
    // Human mode uses MEM_TOTAL= (no _KB suffix) with suffixes like M, G
    assert!(stdout.contains("MEM_TOTAL="));
    assert!(!stdout.contains("MEM_TOTAL_KB="));
    // Should have size suffixes
    assert!(stdout.contains("M") || stdout.contains("G") || stdout.contains("K"));
}

#[test]
fn human_readable_block() {
    let (ok, stdout, _) = run_kv(&["block", "-h"]);
    assert!(ok);
    // Human mode uses SIZE= (not SIZE_SECTORS=)
    if stdout.contains("SIZE=") {
        assert!(!stdout.contains("SIZE_SECTORS="));
    }
}

#[test]
fn help_short_flag() {
    // -H should show help (not -h, which is human-readable now)
    let (ok, stdout, _) = run_kv(&["-H"]);
    assert!(ok);
    assert!(stdout.contains("USAGE:"));
    assert!(stdout.contains("SUBCOMMANDS:"));
}

#[test]
fn thermal_runs() {
    let (ok, _, _) = run_kv(&["thermal"]);
    // Thermal might not exist in containers/VMs, but shouldn't error
    assert!(ok);
}

#[test]
fn thermal_json() {
    let (ok, stdout, _) = run_kv(&["thermal", "-j"]);
    assert!(ok);
    assert!(stdout.contains("\"subcommand\":\"thermal\""));
}

#[test]
fn power_runs() {
    let (ok, _, _) = run_kv(&["power"]);
    // Power supplies might not exist in VMs, but shouldn't error
    assert!(ok);
}

#[test]
fn power_json() {
    let (ok, stdout, _) = run_kv(&["power", "-j"]);
    assert!(ok);
    assert!(stdout.contains("\"subcommand\":\"power\""));
}

// dt (device tree) is only available on ARM/RISC-V, so we test conditionally
#[test]
#[cfg(any(target_arch = "aarch64", target_arch = "arm", target_arch = "riscv64"))]
fn dt_runs() {
    let (ok, _, _) = run_kv(&["dt"]);
    // DT might not exist even on ARM (containers), but shouldn't error
    assert!(ok);
}

#[test]
#[cfg(any(target_arch = "aarch64", target_arch = "arm", target_arch = "riscv64"))]
fn dt_json() {
    let (ok, stdout, _) = run_kv(&["dt", "-j"]);
    assert!(ok);
    assert!(stdout.contains("\"subcommand\":\"dt\""));
}

// Filter tests
#[test]
fn filter_block() {
    let (ok, _, _) = run_kv(&["block", "-f", "loop"]);
    assert!(ok);
    // Filter should work even if no matches
}

#[test]
fn filter_net() {
    let (ok, stdout, _) = run_kv(&["net", "-f", "lo"]);
    assert!(ok);
    // Loopback should match on any system
    assert!(stdout.contains("lo") || stdout.is_empty());
}

#[test]
fn filter_json() {
    // Note: -f must be separate because it takes an argument
    let (ok, stdout, _) = run_kv(&["net", "-j", "-f", "lo"]);
    assert!(ok);
    assert!(stdout.contains("\"subcommand\":\"net\""));
}
