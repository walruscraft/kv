//! Device tree information from /sys/firmware/devicetree/base.
//!
//! The devicetree is the primary hardware description mechanism on ARM,
//! AArch64, and RISC-V systems. It's a hierarchical tree of nodes that
//! describe the hardware topology.
//!
//! Unlike other subcommands, dt supports filtering because the full tree
//! can have hundreds of nodes. Common use cases:
//! - Quick board identification (default: show root model/compatible)
//! - Find disabled devices (common debugging task)
//! - Search for specific device types
//! - Inspect a specific node path

#![cfg(any(target_arch = "arm", target_arch = "aarch64", target_arch = "riscv64"))]

use crate::cli::GlobalOptions;
use crate::filter::Filterable;
use crate::json::{begin_kv_output, JsonWriter};
use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

const DT_BASE_PATH: &str = "/sys/firmware/devicetree/base";

// =============================================================================
// Input Safety Limits
// =============================================================================

/// Maximum recursion depth for traversing devicetree (defense against stack overflow).
/// Real devicetrees rarely exceed 10-15 levels; 64 is generous.
const MAX_RECURSION_DEPTH: usize = 64;

/// Maximum number of nodes to collect (defense against symlink loops or huge trees).
const MAX_NODE_COUNT: usize = 4096;

/// Maximum property file size to read (64 KiB - defense against large file attacks).
const MAX_PROPERTY_SIZE: u64 = 64 * 1024;

/// Options specific to the dt subcommand.
#[derive(Debug, Default)]
pub struct DtOptions {
    /// Show only disabled nodes
    pub disabled_only: bool,
    /// Specific node path to inspect
    pub node_path: Option<String>,
}

impl DtOptions {
    /// Parse dt-specific options from remaining arguments.
    pub fn parse(args: &[String]) -> Self {
        let mut opts = DtOptions::default();

        for arg in args {
            match arg.as_str() {
                "-d" | "--disabled" => {
                    opts.disabled_only = true;
                }
                s if s.starts_with('/') => {
                    // Looks like a DT path
                    opts.node_path = Some(s.to_string());
                }
                _ => {}
            }
        }

        opts
    }
}

/// A node in the device tree.
#[derive(Debug, Clone)]
pub struct DtNode {
    /// Full path from root (e.g., "/soc/serial@12340000")
    pub path: String,
    /// Node name (e.g., "serial@12340000")
    pub name: String,
    /// Properties as key-value pairs
    pub properties: HashMap<String, String>,
}

impl Filterable for DtNode {
    fn filter_fields(&self) -> Vec<&str> {
        let compat = self.properties.get("compatible").map(|s| s.as_str()).unwrap_or("");
        vec![&self.path, compat]
    }
}

impl DtNode {

    /// Check if this node is disabled.
    fn is_disabled(&self) -> bool {
        if let Some(status) = self.properties.get("status") {
            // "okay" or "ok" means enabled, anything else (usually "disabled") means disabled
            let s = status.trim().to_lowercase();
            s != "okay" && s != "ok"
        } else {
            // No status property means enabled by default
            false
        }
    }

    /// Output as text for a single node (detailed view).
    pub fn print_text_detailed(&self) {
        println!("PATH={}", self.path);

        // Sort properties for consistent output
        let mut props: Vec<_> = self.properties.iter().collect();
        props.sort_by_key(|(k, _)| *k);

        for (key, value) in props {
            // Quote values that contain spaces or special chars
            if value.contains(' ') || value.contains(',') || value.is_empty() {
                println!("  {}=\"{}\"", key, value);
            } else {
                println!("  {}={}", key, value);
            }
        }
    }

    /// Output as text for list view (one line per node).
    pub fn print_text_line(&self, verbose: bool) {
        let mut parts = Vec::new();

        parts.push(format!("PATH={}", self.path));

        if let Some(compatible) = self.properties.get("compatible") {
            parts.push(format!("COMPATIBLE=\"{}\"", compatible));
        }

        if let Some(status) = self.properties.get("status") {
            parts.push(format!("STATUS={}", status));
        }

        if verbose {
            if let Some(reg) = self.properties.get("reg") {
                parts.push(format!("REG=\"{}\"", reg));
            }
        }

        println!("{}", parts.join(" "));
    }
}

/// Read a property file and try to interpret it as a readable string.
/// Skips symlinks and files larger than MAX_PROPERTY_SIZE for safety.
fn read_property(path: &Path) -> Option<String> {
    // Use symlink_metadata to detect symlinks without following them
    let meta = fs::symlink_metadata(path).ok()?;

    // Skip symlinks - they could point outside the DT base
    if meta.file_type().is_symlink() {
        return None;
    }

    // Skip files that are too large (defense against malicious large files)
    if meta.len() > MAX_PROPERTY_SIZE {
        return None;
    }

    let data = fs::read(path).ok()?;

    if data.is_empty() {
        return Some(String::new());
    }

    // Try to interpret as string(s)
    let is_stringy = data.iter().all(|&b| {
        b == 0 || (b >= 0x20 && b < 0x7f) || b == b'\n' || b == b'\t'
    });

    if is_stringy {
        let strings: Vec<&str> = data
            .split(|&b| b == 0)
            .filter_map(|bytes| {
                if bytes.is_empty() {
                    None
                } else {
                    std::str::from_utf8(bytes).ok()
                }
            })
            .collect();

        if !strings.is_empty() {
            return Some(strings.join(", "));
        }
    }

    // Fall back to hex for binary data (limit to 32 bytes)
    let display_data = if data.len() > 32 { &data[..32] } else { &data };
    let hex: Vec<String> = display_data.iter().map(|b| format!("{:02x}", b)).collect();
    let mut result = hex.join(" ");
    if data.len() > 32 {
        result.push_str("...");
    }
    Some(result)
}

/// Sanitize a relative path, rejecting any path traversal attempts.
/// Returns None if the path contains ".." or other unsafe components.
fn sanitize_relative_path(base_path: &Path, relative_path: &str) -> Option<PathBuf> {
    if relative_path == "/" {
        return Some(base_path.to_path_buf());
    }

    let clean = relative_path.trim_start_matches('/');
    let candidate = Path::new(clean);

    // Reject any path with parent directory references
    for component in candidate.components() {
        match component {
            Component::ParentDir => return None, // ".." is not allowed
            Component::Normal(_) => {}           // Regular path component, OK
            _ => return None,                    // Reject prefix, root, curdir
        }
    }

    Some(base_path.join(clean))
}

/// Read a single DT node (non-recursive).
fn read_single_node(base_path: &Path, relative_path: &str) -> Option<DtNode> {
    let full_path = sanitize_relative_path(base_path, relative_path)?;

    if !full_path.is_dir() {
        return None;
    }

    let name = if relative_path == "/" {
        "/".to_string()
    } else {
        Path::new(relative_path)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| relative_path.to_string())
    };

    let mut properties = HashMap::new();

    if let Ok(entries) = fs::read_dir(&full_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            let entry_name = entry.file_name().to_string_lossy().to_string();
            if entry_name == "name" {
                continue; // Redundant with node name
            }

            // Use symlink_metadata to avoid following symlinks
            if let Ok(metadata) = fs::symlink_metadata(&entry.path()) {
                // Only read regular files, skip symlinks and directories
                if metadata.is_file() {
                    if let Some(value) = read_property(&entry.path()) {
                        properties.insert(entry_name, value);
                    }
                }
            }
        }
    }

    Some(DtNode {
        path: if relative_path.is_empty() { "/".to_string() } else { relative_path.to_string() },
        name,
        properties,
    })
}

/// Recursively read all DT nodes with depth and node count limits.
/// Uses symlink_metadata to avoid following symlinks outside the DT base.
fn read_dt_recursive(base_path: &Path, relative_path: &str, nodes: &mut Vec<DtNode>, depth: usize) {
    // Safety: stop recursion if we've gone too deep or collected too many nodes
    if depth > MAX_RECURSION_DEPTH || nodes.len() >= MAX_NODE_COUNT {
        return;
    }

    let full_path = if relative_path == "/" {
        base_path.to_path_buf()
    } else {
        base_path.join(relative_path.trim_start_matches('/'))
    };

    // Read this node
    if let Some(node) = read_single_node(base_path, relative_path) {
        nodes.push(node);
    }

    // Stop if we hit the node limit
    if nodes.len() >= MAX_NODE_COUNT {
        return;
    }

    // Recurse into children
    if let Ok(entries) = fs::read_dir(&full_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            // Use symlink_metadata to detect symlinks without following them
            if let Ok(metadata) = fs::symlink_metadata(&entry.path()) {
                // Skip symlinks - they could point outside DT base or create loops
                if metadata.file_type().is_symlink() {
                    continue;
                }

                if metadata.is_dir() {
                    let entry_name = entry.file_name().to_string_lossy().to_string();

                    // Verify child path stays within base (defense against path manipulation)
                    let child_full = entry.path();
                    if let (Ok(canon_child), Ok(canon_base)) =
                        (child_full.canonicalize(), base_path.canonicalize())
                    {
                        if !canon_child.starts_with(&canon_base) {
                            continue; // Path escaped the DT base, skip it
                        }
                    }

                    let child_path = if relative_path == "/" {
                        format!("/{}", entry_name)
                    } else {
                        format!("{}/{}", relative_path, entry_name)
                    };
                    read_dt_recursive(base_path, &child_path, nodes, depth + 1);
                }
            }
        }
    }
}

/// Count total nodes without loading all data (with depth and count limits).
/// Uses symlink_metadata to avoid following symlinks.
fn count_nodes(base_path: &Path, relative_path: &str, depth: usize) -> usize {
    // Safety: stop recursion if we've gone too deep
    if depth > MAX_RECURSION_DEPTH {
        return 0;
    }

    let full_path = if relative_path == "/" {
        base_path.to_path_buf()
    } else {
        base_path.join(relative_path.trim_start_matches('/'))
    };

    let mut count = 1; // This node

    if let Ok(entries) = fs::read_dir(&full_path) {
        for entry in entries.filter_map(|e| e.ok()) {
            // Use symlink_metadata to detect symlinks without following them
            if let Ok(metadata) = fs::symlink_metadata(&entry.path()) {
                // Skip symlinks
                if metadata.file_type().is_symlink() {
                    continue;
                }

                if metadata.is_dir() {
                    // Verify child path stays within base
                    let child_full = entry.path();
                    if let (Ok(canon_child), Ok(canon_base)) =
                        (child_full.canonicalize(), base_path.canonicalize())
                    {
                        if !canon_child.starts_with(&canon_base) {
                            continue;
                        }
                    }

                    let entry_name = entry.file_name().to_string_lossy().to_string();
                    let child_path = if relative_path == "/" {
                        format!("/{}", entry_name)
                    } else {
                        format!("{}/{}", relative_path, entry_name)
                    };
                    count += count_nodes(base_path, &child_path, depth + 1);

                    // Stop if we've counted enough
                    if count >= MAX_NODE_COUNT {
                        return count;
                    }
                }
            }
        }
    }

    count
}

/// Entry point for `kv dt` subcommand.
pub fn run(opts: &GlobalOptions, args: &[String]) -> i32 {
    let dt_opts = DtOptions::parse(args);
    let base = PathBuf::from(DT_BASE_PATH);

    if !base.exists() {
        if opts.json {
            let mut w = begin_kv_output(opts.pretty, "dt");
            w.key("data");
            w.value_null();
            w.key("error");
            w.value_string("devicetree not found");
            w.end_object();
            println!("{}", w.finish());
        } else {
            println!("dt: devicetree not found ({})", DT_BASE_PATH);
        }
        return 0;
    }

    // Mode 1: Specific node path
    if let Some(ref node_path) = dt_opts.node_path {
        return run_single_node(opts, &base, node_path);
    }

    // Mode 2: Filtered list (disabled or global filter pattern)
    if dt_opts.disabled_only || opts.filter.is_some() {
        return run_filtered(opts, &base, &dt_opts);
    }

    // Mode 3: Default - show root summary (or full list with -v)
    if opts.verbose {
        return run_full_list(opts, &base);
    }

    run_summary(opts, &base)
}

/// Show summary: root node info + node count.
fn run_summary(opts: &GlobalOptions, base: &Path) -> i32 {
    let root = read_single_node(base, "/");
    let count = count_nodes(base, "/", 0);

    if opts.json {
        let mut w = begin_kv_output(opts.pretty, "dt");
        w.field_object("data");

        if let Some(ref node) = root {
            if let Some(model) = node.properties.get("model") {
                w.field_str("model", model);
            }
            if let Some(compatible) = node.properties.get("compatible") {
                w.field_str("compatible", compatible);
            }
        }
        w.field_u64("node_count", count as u64);

        w.end_field_object();
        w.end_object();
        println!("{}", w.finish());
    } else {
        if let Some(ref node) = root {
            if let Some(model) = node.properties.get("model") {
                println!("MODEL=\"{}\"", model);
            }
            if let Some(compatible) = node.properties.get("compatible") {
                println!("COMPATIBLE=\"{}\"", compatible);
            }
        }
        println!("NODES={}", count);
        println!();
        println!("Use -v for full list, -f <pattern> to search, -d for disabled nodes");
    }

    0
}

/// Show a single node in detail.
fn run_single_node(opts: &GlobalOptions, base: &Path, node_path: &str) -> i32 {
    let node = match read_single_node(base, node_path) {
        Some(n) => n,
        None => {
            if opts.json {
                let mut w = begin_kv_output(opts.pretty, "dt");
                w.key("data");
                w.value_null();
                w.key("error");
                w.value_string(&format!("node not found: {}", node_path));
                w.end_object();
                println!("{}", w.finish());
            } else {
                println!("dt: node not found: {}", node_path);
            }
            return 0;
        }
    };

    if opts.json {
        let mut w = begin_kv_output(opts.pretty, "dt");
        w.field_object("data");
        w.field_str("path", &node.path);
        w.field_str("name", &node.name);

        if !node.properties.is_empty() {
            w.field_object("properties");
            let mut props: Vec<_> = node.properties.iter().collect();
            props.sort_by_key(|(k, _)| *k);
            for (key, value) in props {
                w.field_str(key, value);
            }
            w.end_field_object();
        }

        w.end_field_object();
        w.end_object();
        println!("{}", w.finish());
    } else {
        node.print_text_detailed();
    }

    0
}

/// Show filtered list of nodes.
fn run_filtered(opts: &GlobalOptions, base: &Path, dt_opts: &DtOptions) -> i32 {
    let mut nodes = Vec::new();
    read_dt_recursive(base, "/", &mut nodes, 0);

    // Apply filters (use global filter from opts)
    let filtered: Vec<_> = nodes
        .into_iter()
        .filter(|n| {
            if dt_opts.disabled_only && !n.is_disabled() {
                return false;
            }
            if let Some(ref pattern) = opts.filter {
                if !n.matches_filter(pattern, opts.filter_case_insensitive) {
                    return false;
                }
            }
            true
        })
        .collect();

    if opts.json {
        print_nodes_json(&filtered, opts.pretty, opts.verbose);
    } else {
        if filtered.is_empty() {
            println!("dt: no matching nodes");
        } else {
            for node in &filtered {
                node.print_text_line(opts.verbose);
            }
            println!();
            println!("({} nodes)", filtered.len());
        }
    }

    0
}

/// Show full list of all nodes.
fn run_full_list(opts: &GlobalOptions, base: &Path) -> i32 {
    let mut nodes = Vec::new();
    read_dt_recursive(base, "/", &mut nodes, 0);

    if opts.json {
        print_nodes_json(&nodes, opts.pretty, opts.verbose);
    } else {
        for node in &nodes {
            node.print_text_line(opts.verbose);
        }
    }

    0
}

/// Print nodes as JSON array.
fn print_nodes_json(nodes: &[DtNode], pretty: bool, verbose: bool) {
    let mut w = begin_kv_output(pretty, "dt");

    w.field_array("data");
    for node in nodes {
        write_node_json(&mut w, node, verbose);
    }
    w.end_field_array();

    w.end_object();
    println!("{}", w.finish());
}

/// Write a single node to JSON.
fn write_node_json(w: &mut JsonWriter, node: &DtNode, verbose: bool) {
    w.array_object_begin();

    w.field_str("path", &node.path);
    w.field_str("name", &node.name);

    if let Some(compatible) = node.properties.get("compatible") {
        w.field_str("compatible", compatible);
    }
    if let Some(status) = node.properties.get("status") {
        w.field_str("status", status);
    }

    if verbose && !node.properties.is_empty() {
        w.field_object("properties");
        let mut props: Vec<_> = node.properties.iter().collect();
        props.sort_by_key(|(k, _)| *k);
        for (key, value) in props {
            w.field_str(key, value);
        }
        w.end_field_object();
    }

    w.array_object_end();
}

/// Collect DT nodes for snapshot.
#[cfg(feature = "snapshot")]
pub fn collect() -> Option<Vec<DtNode>> {
    let base = PathBuf::from(DT_BASE_PATH);
    if !base.exists() {
        return None;
    }
    let mut nodes = Vec::new();
    read_dt_recursive(&base, "/", &mut nodes, 0);
    Some(nodes)
}

/// Write DT nodes to JSON writer (for snapshot).
#[cfg(feature = "snapshot")]
pub fn write_json_snapshot(w: &mut JsonWriter, nodes: &[DtNode], verbose: bool) {
    w.field_array("dt");
    for node in nodes {
        write_node_json(w, node, verbose);
    }
    w.end_field_array();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dt_options_empty() {
        let opts = DtOptions::parse(&[]);
        assert!(!opts.disabled_only);
        assert!(opts.node_path.is_none());
    }

    #[test]
    fn parse_dt_options_disabled() {
        let args: Vec<String> = vec!["-d".into()];
        let opts = DtOptions::parse(&args);
        assert!(opts.disabled_only);
    }

    #[test]
    fn parse_dt_options_path() {
        let args: Vec<String> = vec!["/soc/uart@10000".into()];
        let opts = DtOptions::parse(&args);
        assert_eq!(opts.node_path.as_deref(), Some("/soc/uart@10000"));
    }

    #[test]
    fn node_matches_filter_case_insensitive() {
        let mut props = HashMap::new();
        props.insert("compatible".to_string(), "arm,pl011".to_string());

        let node = DtNode {
            path: "/soc/serial@12340000".to_string(),
            name: "serial@12340000".to_string(),
            properties: props,
        };

        // Case-insensitive matching (-F)
        assert!(node.matches_filter("serial", true));
        assert!(node.matches_filter("SERIAL", true));
        assert!(node.matches_filter("pl011", true));
        assert!(node.matches_filter("PL011", true));
        assert!(!node.matches_filter("gpio", true));
    }

    #[test]
    fn node_matches_filter_case_sensitive() {
        let mut props = HashMap::new();
        props.insert("compatible".to_string(), "arm,pl011".to_string());

        let node = DtNode {
            path: "/soc/serial@12340000".to_string(),
            name: "serial@12340000".to_string(),
            properties: props,
        };

        // Case-sensitive matching (-f)
        assert!(node.matches_filter("serial", false));
        assert!(!node.matches_filter("SERIAL", false)); // no match
        assert!(node.matches_filter("pl011", false));
        assert!(!node.matches_filter("PL011", false)); // no match
    }

    #[test]
    fn node_is_disabled() {
        let mut props = HashMap::new();
        props.insert("status".to_string(), "disabled".to_string());
        let disabled = DtNode {
            path: "/test".to_string(),
            name: "test".to_string(),
            properties: props,
        };
        assert!(disabled.is_disabled());

        let mut props2 = HashMap::new();
        props2.insert("status".to_string(), "okay".to_string());
        let enabled = DtNode {
            path: "/test2".to_string(),
            name: "test2".to_string(),
            properties: props2,
        };
        assert!(!enabled.is_disabled());

        // No status = enabled
        let no_status = DtNode {
            path: "/test3".to_string(),
            name: "test3".to_string(),
            properties: HashMap::new(),
        };
        assert!(!no_status.is_disabled());
    }

    #[test]
    fn sanitize_path_valid() {
        let base = Path::new("/sys/firmware/devicetree/base");

        // Root path
        assert_eq!(
            sanitize_relative_path(base, "/"),
            Some(PathBuf::from("/sys/firmware/devicetree/base"))
        );

        // Normal paths
        assert_eq!(
            sanitize_relative_path(base, "/soc"),
            Some(PathBuf::from("/sys/firmware/devicetree/base/soc"))
        );
        assert_eq!(
            sanitize_relative_path(base, "/soc/uart@10000"),
            Some(PathBuf::from("/sys/firmware/devicetree/base/soc/uart@10000"))
        );
    }

    #[test]
    fn sanitize_path_rejects_traversal() {
        let base = Path::new("/sys/firmware/devicetree/base");

        // Path traversal attempts should be rejected
        assert_eq!(sanitize_relative_path(base, "/.."), None);
        assert_eq!(sanitize_relative_path(base, "/../etc"), None);
        assert_eq!(sanitize_relative_path(base, "/soc/../../etc"), None);
        assert_eq!(sanitize_relative_path(base, "/soc/../.."), None);
    }

    #[test]
    fn max_recursion_depth_is_reasonable() {
        // Verify the constant exists and is sensible
        // Real devicetrees rarely exceed 10-15 levels
        assert!(MAX_RECURSION_DEPTH >= 32, "depth limit too restrictive");
        assert!(MAX_RECURSION_DEPTH <= 128, "depth limit too permissive");
    }

    #[test]
    fn recursion_stops_at_depth_limit() {
        // Test that read_dt_recursive respects depth limit
        // Use a non-existent path so it returns immediately without I/O
        let fake_base = Path::new("/nonexistent/dt/path");
        let mut nodes = Vec::new();

        // Starting at depth 0 should work (even though path doesn't exist)
        read_dt_recursive(fake_base, "/", &mut nodes, 0);
        assert!(nodes.is_empty()); // No nodes since path doesn't exist

        // Starting beyond max depth should also work (just returns early)
        read_dt_recursive(fake_base, "/", &mut nodes, MAX_RECURSION_DEPTH + 1);
        assert!(nodes.is_empty());
    }

    #[test]
    fn count_nodes_respects_depth_limit() {
        let fake_base = Path::new("/nonexistent/dt/path");

        // At depth 0, returns 1 for this node (even if can't read children)
        // Actually, it returns 1 only if the path exists; since it doesn't, returns 1 anyway
        // because count starts at 1 before trying to read_dir
        let count_normal = count_nodes(fake_base, "/", 0);

        // Beyond max depth should return 0
        let count_too_deep = count_nodes(fake_base, "/", MAX_RECURSION_DEPTH + 1);
        assert_eq!(count_too_deep, 0, "should return 0 when depth exceeded");
    }
}
