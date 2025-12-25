//! Network interface information from /sys/class/net.
//!
//! Shows network interfaces with their MAC addresses, MTU, operational state,
//! statistics, IP addresses, and wireless signal info.
//!
//! IP addresses are parsed from /proc/net/fib_trie (IPv4) and /proc/net/if_inet6 (IPv6).
//! Wireless signal quality comes from /proc/net/wireless.

use crate::cli::GlobalOptions;
use crate::fields::{net as f, to_text_key};
use crate::filter::{opt_str, Filterable};
use crate::io;
use crate::json::{begin_kv_output, JsonWriter};
use std::collections::HashMap;
use std::path::PathBuf;

const NET_SYSFS_PATH: &str = "/sys/class/net";
const PROC_NET_WIRELESS: &str = "/proc/net/wireless";
const PROC_NET_IF_INET6: &str = "/proc/net/if_inet6";
const PROC_NET_FIB_TRIE: &str = "/proc/net/fib_trie";
const PROC_NET_ROUTE: &str = "/proc/net/route";

// =============================================================================
// Endianness Handling
// =============================================================================

// The kernel's /proc/net/route outputs IP addresses as raw u32 via %08X.
// This gives different hex strings on big-endian vs little-endian systems:
//   IP 192.168.1.0 (network order: C0 A8 01 00)
//   - Little-endian output: "0001A8C0"
//   - Big-endian output:    "C0A80100"
// We normalize everything to network byte order (big-endian) for comparison.

// =============================================================================
// Input Safety Limits
// =============================================================================

/// Maximum file size for /proc/net/fib_trie (2 MiB - defense against huge routing tables).
const MAX_FIB_TRIE_SIZE: u64 = 2 * 1024 * 1024;

/// Maximum number of IPs to track per interface (defense against memory exhaustion).
const MAX_IPS_PER_INTERFACE: usize = 64;

/// Maximum total IPs to track across all interfaces.
const MAX_TOTAL_IPS: usize = 1024;

/// Wireless signal information.
#[derive(Debug, Clone)]
pub struct WirelessInfo {
    /// Link quality (0-100 typically)
    pub link_quality: i32,
    /// Signal level in dBm
    pub signal_dbm: i32,
    /// Noise level in dBm (often -256 meaning unavailable)
    pub noise_dbm: i32,
}

/// Information about a single network interface.
#[derive(Debug, Clone)]
pub struct NetInterface {
    /// Interface name (e.g., "eth0", "wlan0")
    pub name: String,
    /// MAC address (e.g., "00:11:22:33:44:55")
    pub mac_address: Option<String>,
    /// MTU (Maximum Transmission Unit)
    pub mtu: Option<u32>,
    /// Operational state (up, down, unknown, etc.)
    pub operstate: Option<String>,
    /// Link speed in Mbps (may not be available for all interfaces)
    pub speed_mbps: Option<u32>,
    /// Interface type (from /sys/class/net/<if>/type)
    pub if_type: Option<u16>,
    /// TX queue length
    pub tx_queue_len: Option<u32>,
    /// Is the interface up (carrier detected)?
    pub carrier: Option<bool>,
    /// Duplex mode (full, half)
    pub duplex: Option<String>,
    /// IPv4 addresses
    pub ipv4_addresses: Vec<String>,
    /// IPv6 addresses
    pub ipv6_addresses: Vec<String>,
    /// Wireless info (signal, noise)
    pub wireless: Option<WirelessInfo>,
    /// Bytes received
    pub rx_bytes: Option<u64>,
    /// Bytes transmitted
    pub tx_bytes: Option<u64>,
    /// Packets received
    pub rx_packets: Option<u64>,
    /// Packets transmitted
    pub tx_packets: Option<u64>,
    /// Receive errors
    pub rx_errors: Option<u64>,
    /// Transmit errors
    pub tx_errors: Option<u64>,
    /// Receive dropped
    pub rx_dropped: Option<u64>,
    /// Transmit dropped
    pub tx_dropped: Option<u64>,
}

impl Filterable for NetInterface {
    fn filter_fields(&self) -> Vec<&str> {
        vec![&self.name, opt_str(&self.mac_address), opt_str(&self.operstate)]
    }
}

impl NetInterface {
    /// Read interface information from sysfs (basic, no IP/wireless info).
    #[allow(dead_code)]
    pub fn read(name: &str) -> Option<Self> {
        Self::read_with_extra(name, &HashMap::new(), &HashMap::new(), &HashMap::new())
    }

    /// Read interface with pre-parsed IP and wireless data.
    fn read_with_extra(
        name: &str,
        ipv4_map: &HashMap<String, Vec<String>>,
        ipv6_map: &HashMap<String, Vec<String>>,
        wireless_map: &HashMap<String, WirelessInfo>,
    ) -> Option<Self> {
        let base = PathBuf::from(NET_SYSFS_PATH).join(name);

        // Interface directory must exist
        if !base.exists() {
            return None;
        }

        let stats_base = base.join("statistics");

        Some(NetInterface {
            name: name.to_string(),
            mac_address: io::read_file_string(base.join("address")),
            mtu: io::read_file_parse(base.join("mtu")),
            operstate: io::read_file_string(base.join("operstate")),
            // Speed file often fails to read on interfaces that aren't up or don't support it
            speed_mbps: io::read_file_parse(base.join("speed")),
            if_type: io::read_file_parse(base.join("type")),
            tx_queue_len: io::read_file_parse(base.join("tx_queue_len")),
            carrier: io::read_file_parse::<u8>(base.join("carrier")).map(|v| v != 0),
            duplex: io::read_file_string(base.join("duplex")),
            ipv4_addresses: ipv4_map.get(name).cloned().unwrap_or_default(),
            ipv6_addresses: ipv6_map.get(name).cloned().unwrap_or_default(),
            wireless: wireless_map.get(name).cloned(),
            // Statistics
            rx_bytes: io::read_file_parse(stats_base.join("rx_bytes")),
            tx_bytes: io::read_file_parse(stats_base.join("tx_bytes")),
            rx_packets: io::read_file_parse(stats_base.join("rx_packets")),
            tx_packets: io::read_file_parse(stats_base.join("tx_packets")),
            rx_errors: io::read_file_parse(stats_base.join("rx_errors")),
            tx_errors: io::read_file_parse(stats_base.join("tx_errors")),
            rx_dropped: io::read_file_parse(stats_base.join("rx_dropped")),
            tx_dropped: io::read_file_parse(stats_base.join("tx_dropped")),
        })
    }
}

/// Parse /proc/net/wireless for signal info.
/// Format: "interface: status link level noise nwid crypt frag retry misc beacon"
/// Values may have trailing "." for missing/invalid readings.
fn parse_proc_net_wireless() -> HashMap<String, WirelessInfo> {
    let mut map = HashMap::new();

    let content = match io::read_file_string(PROC_NET_WIRELESS) {
        Some(s) => s,
        None => return map,
    };

    // Skip header lines (first two lines)
    for line in content.lines().skip(2) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Format: "wlan0: 0000   70.  -40.  -256."
        // Split on ":" to get interface name
        let mut parts = line.splitn(2, ':');
        let iface = match parts.next() {
            Some(s) => s.trim(),
            None => continue,
        };
        let rest = match parts.next() {
            Some(s) => s,
            None => continue,
        };

        // Parse the numeric values (link, level, noise)
        // They may have trailing "." which we strip
        let values: Vec<&str> = rest.split_whitespace().collect();
        if values.len() < 4 {
            continue;
        }

        // values[0] = status, [1] = link, [2] = level, [3] = noise
        let parse_val = |s: &str| -> i32 {
            s.trim_end_matches('.').parse().unwrap_or(0)
        };

        let link = parse_val(values[1]);
        let level = parse_val(values[2]);
        let noise = parse_val(values[3]);

        map.insert(
            iface.to_string(),
            WirelessInfo {
                link_quality: link,
                signal_dbm: level,
                noise_dbm: noise,
            },
        );
    }

    map
}

/// Parse /proc/net/if_inet6 for IPv6 addresses.
/// Format: "address ifindex prefix_len scope flags ifname"
/// Example: "fe8000000000000002155dfffeab62d2 02 40 20 80 eth0"
fn parse_proc_net_if_inet6() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();

    let content = match io::read_file_string(PROC_NET_IF_INET6) {
        Some(s) => s,
        None => return map,
    };

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 6 {
            continue;
        }

        let addr_hex = parts[0];
        let prefix_len = parts[2];
        let ifname = parts[5];

        // Convert hex address to IPv6 notation
        if let Some(addr) = hex_to_ipv6(addr_hex) {
            let addr_with_prefix = format!("{}/{}", addr, prefix_len);
            map.entry(ifname.to_string())
                .or_default()
                .push(addr_with_prefix);
        }
    }

    map
}

/// Convert 32-char hex string to IPv6 address notation.
fn hex_to_ipv6(hex: &str) -> Option<String> {
    if hex.len() != 32 {
        return None;
    }

    // Split into 8 groups of 4 hex chars
    let groups: Vec<&str> = (0..8).map(|i| &hex[i * 4..(i + 1) * 4]).collect();

    // Join with colons, could compress zeroes but let's keep it simple
    Some(groups.join(":"))
}

/// Parse /proc/net/fib_trie to extract local IPv4 addresses.
/// This is complex; we look for "/32 host LOCAL" entries.
/// Includes safety limits for file size and IP count.
fn parse_proc_net_fib_trie() -> HashMap<String, Vec<String>> {
    // fib_trie doesn't directly give us interface names, so we need
    // to cross-reference with /proc/net/route to map IPs to interfaces.
    let route_map = parse_proc_net_route();

    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    // Track IPs we've already seen to avoid duplicates (Main: and Local: have same data)
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Check file size before reading (defense against huge routing tables)
    if let Ok(meta) = std::fs::metadata(PROC_NET_FIB_TRIE) {
        if meta.len() > MAX_FIB_TRIE_SIZE {
            return map; // File too large, skip
        }
    }

    let content = match io::read_file_string(PROC_NET_FIB_TRIE) {
        Some(s) => s,
        None => return map,
    };

    let mut current_ip: Option<String> = None;

    for line in content.lines() {
        // Stop if we've collected too many IPs
        if seen.len() >= MAX_TOTAL_IPS {
            break;
        }

        let trimmed = line.trim();

        // Look for IP address lines: "|-- 192.168.1.100"
        if let Some(ip_part) = trimmed.strip_prefix("|-- ") {
            // Validate it looks like an IP
            if ip_part.chars().all(|c| c.is_ascii_digit() || c == '.') {
                current_ip = Some(ip_part.to_string());
            }
        } else if trimmed.contains("/32 host LOCAL") {
            // This line indicates the previous IP is a local address
            if let Some(ref ip) = current_ip {
                // Skip if we've already seen this IP
                if seen.contains(ip) {
                    continue;
                }
                seen.insert(ip.clone());

                // Loopback addresses (127.x.x.x) go to 'lo'
                if ip.starts_with("127.") {
                    let entry = map.entry("lo".to_string()).or_default();
                    if entry.len() < MAX_IPS_PER_INTERFACE {
                        entry.push(ip.clone());
                    }
                } else if let Some(iface) = find_interface_for_ip(ip, &route_map) {
                    // Try to find which interface owns this IP via routing table
                    let entry = map.entry(iface).or_default();
                    if entry.len() < MAX_IPS_PER_INTERFACE {
                        entry.push(ip.clone());
                    }
                }
            }
        }
    }

    map
}

/// Parse /proc/net/route to build interface -> network mapping.
/// Returns map of (network_addr, netmask) -> interface
///
/// NOTE: The kernel prints IPs as raw u32 values via %08X, which gives different
/// output on big vs little endian systems. We normalize to network byte order.
fn parse_proc_net_route() -> Vec<(String, u32, u32)> {
    let mut routes = Vec::new();

    let content = match io::read_file_string(PROC_NET_ROUTE) {
        Some(s) => s,
        None => return routes,
    };

    // Skip header line
    for line in content.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 8 {
            continue;
        }

        let iface = parts[0];
        let dest_hex = parts[1];
        let mask_hex = parts[7];

        // Parse hex values and normalize to network byte order
        if let (Some(dest), Some(mask)) = (parse_route_hex(dest_hex), parse_route_hex(mask_hex)) {
            routes.push((iface.to_string(), dest, mask));
        }
    }

    routes
}

/// Parse hex IP from /proc/net/route and normalize to network byte order.
///
/// The kernel outputs the raw u32 value, which differs by endianness:
/// - On little-endian: bytes are reversed, so we swap to get network order
/// - On big-endian: already in network order, no swap needed
fn parse_route_hex(hex: &str) -> Option<u32> {
    let val = u32::from_str_radix(hex, 16).ok()?;
    // On little-endian, swap bytes to get network order
    #[cfg(target_endian = "little")]
    let val = val.swap_bytes();
    Some(val)
}

/// Find which interface an IP belongs to based on routing table.
fn find_interface_for_ip(ip: &str, routes: &[(String, u32, u32)]) -> Option<String> {
    // Parse IP to u32 in network byte order
    let parts: Vec<u8> = ip
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();

    if parts.len() != 4 {
        return None;
    }

    // Convert to network byte order (big-endian) to match route table values
    let ip_val = u32::from_be_bytes([parts[0], parts[1], parts[2], parts[3]]);

    // Find the most specific matching route (highest netmask)
    let mut best_match: Option<(&str, u32)> = None;

    for (iface, dest, mask) in routes {
        if (ip_val & mask) == *dest {
            match best_match {
                None => best_match = Some((iface, *mask)),
                Some((_, best_mask)) if *mask > best_mask => {
                    best_match = Some((iface, *mask));
                }
                _ => {}
            }
        }
    }

    best_match.map(|(iface, _)| iface.to_string())
}

impl NetInterface {
    /// Output as text (single line, KEY=VALUE format).
    pub fn print_text(&self, verbose: bool, human: bool) {
        let mut parts = Vec::new();

        parts.push(format!("{}={}", to_text_key(f::NAME), self.name));

        if let Some(ref mac) = self.mac_address {
            parts.push(format!("{}={}", to_text_key(f::MAC), mac));
        }
        if let Some(mtu) = self.mtu {
            parts.push(format!("{}={}", to_text_key(f::MTU), mtu));
        }
        if let Some(ref state) = self.operstate {
            parts.push(format!("{}={}", to_text_key(f::STATE), state));
        }
        if let Some(speed) = self.speed_mbps {
            parts.push(format!("{}={}", to_text_key(f::SPEED), speed));
        }

        // Show first IPv4 address in basic mode
        if let Some(ip) = self.ipv4_addresses.first() {
            parts.push(format!("{}={}", to_text_key(f::IP), ip));
        }

        // Show wireless signal if available
        if let Some(ref wifi) = self.wireless {
            parts.push(format!("{}={}dBm", to_text_key(f::SIGNAL), wifi.signal_dbm));
        }

        if verbose {
            // Show all IP addresses
            if self.ipv4_addresses.len() > 1 {
                parts.push(format!("{}={}", to_text_key(f::IPV4), self.ipv4_addresses.join(",")));
            }
            if !self.ipv6_addresses.is_empty() {
                parts.push(format!("{}={}", to_text_key(f::IPV6), self.ipv6_addresses.join(",")));
            }

            // Wireless details
            if let Some(ref wifi) = self.wireless {
                parts.push(format!("{}={}", to_text_key(f::LINK), wifi.link_quality));
                if wifi.noise_dbm != -256 {
                    parts.push(format!("{}={}dBm", to_text_key(f::NOISE), wifi.noise_dbm));
                }
            }

            if let Some(ref duplex) = self.duplex {
                parts.push(format!("{}={}", to_text_key(f::DUPLEX), duplex));
            }
            if let Some(carrier) = self.carrier {
                parts.push(format!("{}={}", to_text_key(f::CARRIER), if carrier { 1 } else { 0 }));
            }
            if human {
                // Human-readable byte counts
                if let Some(rx) = self.rx_bytes {
                    parts.push(format!("RX={}", io::format_size_human(rx)));
                }
                if let Some(tx) = self.tx_bytes {
                    parts.push(format!("TX={}", io::format_size_human(tx)));
                }
            } else {
                if let Some(rx) = self.rx_bytes {
                    parts.push(format!("{}={}", to_text_key(f::RX_BYTES), rx));
                }
                if let Some(tx) = self.tx_bytes {
                    parts.push(format!("{}={}", to_text_key(f::TX_BYTES), tx));
                }
            }
            if let Some(rx) = self.rx_packets {
                parts.push(format!("{}={}", to_text_key(f::RX_PACKETS), rx));
            }
            if let Some(tx) = self.tx_packets {
                parts.push(format!("{}={}", to_text_key(f::TX_PACKETS), tx));
            }
            if let Some(v) = self.rx_errors {
                parts.push(format!("{}={}", to_text_key(f::RX_ERRORS), v));
            }
            if let Some(v) = self.tx_errors {
                parts.push(format!("{}={}", to_text_key(f::TX_ERRORS), v));
            }
        }

        println!("{}", parts.join(" "));
    }
}

/// Read all network interfaces with IP and wireless info.
pub fn read_interfaces() -> Vec<NetInterface> {
    // Pre-parse IP addresses and wireless info (shared across all interfaces)
    let ipv4_map = parse_proc_net_fib_trie();
    let ipv6_map = parse_proc_net_if_inet6();
    let wireless_map = parse_proc_net_wireless();

    let names = io::read_dir_names_sorted(NET_SYSFS_PATH);
    names
        .iter()
        .filter_map(|name| NetInterface::read_with_extra(name, &ipv4_map, &ipv6_map, &wireless_map))
        .collect()
}

/// Entry point for `kv net` subcommand.
pub fn run(opts: &GlobalOptions) -> i32 {
    let interfaces = read_interfaces();

    // Apply filter if specified
    let interfaces: Vec<_> = if let Some(ref pattern) = opts.filter {
        interfaces
            .into_iter()
            .filter(|i| i.matches_filter(pattern, opts.filter_case_insensitive))
            .collect()
    } else {
        interfaces
    };

    if interfaces.is_empty() {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "net");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            println!("{}", w.finish());
        } else if opts.filter.is_some() {
            println!("net: no matching interfaces");
        } else {
            println!("net: no network interfaces found");
        }
        return 0;
    }

    if opts.json {
        print_json(&interfaces, opts.pretty, opts.verbose, opts.human);
    } else {
        for iface in &interfaces {
            iface.print_text(opts.verbose, opts.human);
        }
    }

    0
}

/// Print interfaces as JSON.
fn print_json(interfaces: &[NetInterface], pretty: bool, verbose: bool, human: bool) {
    let mut w = begin_kv_output(pretty, "net");

    w.field_array("data");
    for iface in interfaces {
        write_interface_json(&mut w, iface, verbose, human);
    }
    w.end_field_array();

    w.end_object();
    println!("{}", w.finish());
}

/// Write a single interface to JSON.
fn write_interface_json(w: &mut JsonWriter, iface: &NetInterface, verbose: bool, human: bool) {
    w.array_object_begin();

    w.field_str(f::NAME, &iface.name);
    w.field_str_opt(f::MAC, iface.mac_address.as_deref());
    w.field_u64_opt(f::MTU, iface.mtu.map(|v| v as u64));
    w.field_str_opt(f::STATE, iface.operstate.as_deref());
    w.field_u64_opt(f::SPEED, iface.speed_mbps.map(|v| v as u64));

    // First IPv4 address in basic mode
    if let Some(ip) = iface.ipv4_addresses.first() {
        w.field_str(f::IP, ip);
    }

    // Wireless signal in basic mode
    if let Some(ref wifi) = iface.wireless {
        w.field_i64(f::SIGNAL, wifi.signal_dbm as i64);
    }

    if verbose {
        // All IPv4 addresses
        if !iface.ipv4_addresses.is_empty() {
            w.field_array(f::IPV4);
            for addr in &iface.ipv4_addresses {
                w.array_string(addr);
            }
            w.end_field_array();
        }
        // All IPv6 addresses
        if !iface.ipv6_addresses.is_empty() {
            w.field_array(f::IPV6);
            for addr in &iface.ipv6_addresses {
                w.array_string(addr);
            }
            w.end_field_array();
        }

        // Full wireless info
        if let Some(ref wifi) = iface.wireless {
            w.field_i64(f::LINK, wifi.link_quality as i64);
            if wifi.noise_dbm != -256 {
                w.field_i64(f::NOISE, wifi.noise_dbm as i64);
            }
        }

        w.field_str_opt(f::DUPLEX, iface.duplex.as_deref());
        w.field_u64_opt("if_type", iface.if_type.map(|v| v as u64));
        w.field_u64_opt("tx_queue_len", iface.tx_queue_len.map(|v| v as u64));
        if let Some(carrier) = iface.carrier {
            w.field_bool(f::CARRIER, carrier);
        }
        if human {
            if let Some(rx) = iface.rx_bytes {
                w.field_str("rx", &io::format_size_human(rx));
            }
            if let Some(tx) = iface.tx_bytes {
                w.field_str("tx", &io::format_size_human(tx));
            }
        } else {
            w.field_u64_opt(f::RX_BYTES, iface.rx_bytes);
            w.field_u64_opt(f::TX_BYTES, iface.tx_bytes);
        }
        w.field_u64_opt(f::RX_PACKETS, iface.rx_packets);
        w.field_u64_opt(f::TX_PACKETS, iface.tx_packets);
        w.field_u64_opt(f::RX_ERRORS, iface.rx_errors);
        w.field_u64_opt(f::TX_ERRORS, iface.tx_errors);
        w.field_u64_opt(f::RX_DROPPED, iface.rx_dropped);
        w.field_u64_opt(f::TX_DROPPED, iface.tx_dropped);
    }

    w.array_object_end();
}

/// Collect interfaces for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect() -> Vec<NetInterface> {
    read_interfaces()
}

/// Write interfaces to JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json_snapshot(w: &mut JsonWriter, interfaces: &[NetInterface], verbose: bool) {
    w.field_array("net");
    for iface in interfaces {
        write_interface_json(w, iface, verbose, false); // snapshot uses raw values
    }
    w.end_field_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    // Most tests require actual /sys/class/net access, which we have on Linux.
    // These are effectively integration tests.

    #[test]
    fn lo_interface_exists() {
        // The loopback interface should always exist on Linux
        let lo = NetInterface::read("lo");
        assert!(lo.is_some(), "loopback interface should exist");

        let lo = lo.unwrap();
        assert_eq!(lo.name, "lo");
        // lo typically has MAC 00:00:00:00:00:00
        assert!(lo.mac_address.is_some());
    }

    #[test]
    fn read_all_interfaces() {
        let interfaces = read_interfaces();
        // At minimum, we should have 'lo'
        assert!(!interfaces.is_empty(), "should have at least loopback");
        assert!(
            interfaces.iter().any(|i| i.name == "lo"),
            "loopback should be in the list"
        );
    }

    #[test]
    fn nonexistent_interface() {
        let iface = NetInterface::read("this_interface_does_not_exist_12345");
        assert!(iface.is_none());
    }
}
