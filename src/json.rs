//! Hand-rolled JSON serialization because we're too cool for serde. Let's pretend
//! it's a choice.
//!
//! This module provides a simple JSON writer that handles proper escaping and
//! supports both compact and pretty-printed output. It's designed for our
//! specific use case: outputting system information as JSON.
//!
//! # Design Notes
//!
//! We use a builder pattern with a `JsonWriter` that writes to a `String`.
//! This avoids the complexity of a full serialization framework while still
//! giving us clean, type-safe JSON generation.
//!
//! The escape handling is modeled after serde_json's approach: a static lookup
//! table for fast character classification, then handle each escape type.

use std::fmt::Write;

/// JSON writer that builds a JSON string.
///
/// Supports both compact and pretty-printed output.
pub struct JsonWriter {
    output: String,
    pretty: bool,
    indent_level: usize,
    /// Track if we need a comma before the next element
    needs_comma: bool,
}

impl JsonWriter {
    /// Create a new JSON writer.
    pub fn new(pretty: bool) -> Self {
        Self {
            // Note: We considered adding hard memory limits for constrained systems, but:
            // - kv's memory footprint is already minimal (reads small sysfs files)
            // - Hard limits add complexity without clear benefit for this use case
            // - The OS provides memory limits via cgroups/ulimit if needed
            output: String::with_capacity(4096), // Pre-allocate for efficiency
            pretty,
            indent_level: 0,
            needs_comma: false,
        }
    }

    /// Get the final JSON string.
    pub fn finish(self) -> String {
        self.output
    }

    /// Write the indentation for pretty printing.
    fn write_indent(&mut self) {
        if self.pretty {
            for _ in 0..self.indent_level {
                self.output.push_str("  ");
            }
        }
    }

    /// Write a newline if pretty printing.
    fn write_newline(&mut self) {
        if self.pretty {
            self.output.push('\n');
        }
    }

    /// Write a comma and newline between elements.
    fn write_separator(&mut self) {
        if self.needs_comma {
            self.output.push(',');
            self.write_newline();
        }
        self.needs_comma = false;
    }

    /// Begin a JSON object `{`.
    pub fn begin_object(&mut self) {
        self.write_separator();
        self.write_indent();
        self.output.push('{');
        self.write_newline();
        self.indent_level += 1;
        self.needs_comma = false;
    }

    /// End a JSON object `}`.
    pub fn end_object(&mut self) {
        self.write_newline();
        self.indent_level -= 1;
        self.write_indent();
        self.output.push('}');
        self.needs_comma = true;
    }

    /// Begin a JSON array `[`.
    #[allow(dead_code)] // Available for future use
    pub fn begin_array(&mut self) {
        self.write_separator();
        self.write_indent();
        self.output.push('[');
        self.write_newline();
        self.indent_level += 1;
        self.needs_comma = false;
    }

    /// End a JSON array `]`.
    #[allow(dead_code)]
    pub fn end_array(&mut self) {
        self.write_newline();
        self.indent_level -= 1;
        self.write_indent();
        self.output.push(']');
        self.needs_comma = true;
    }

    /// Write an object key. Must be followed by a value.
    pub fn key(&mut self, name: &str) {
        self.write_separator();
        self.write_indent();
        self.output.push('"');
        escape_string_into(&mut self.output, name);
        self.output.push_str("\":");
        if self.pretty {
            self.output.push(' ');
        }
        self.needs_comma = false;
    }

    /// Write a string value.
    pub fn value_string(&mut self, value: &str) {
        self.output.push('"');
        escape_string_into(&mut self.output, value);
        self.output.push('"');
        self.needs_comma = true;
    }

    /// Write an integer value.
    #[allow(dead_code)]
    pub fn value_i64(&mut self, value: i64) {
        write!(self.output, "{}", value).unwrap();
        self.needs_comma = true;
    }

    /// Write an unsigned integer value.
    pub fn value_u64(&mut self, value: u64) {
        write!(self.output, "{}", value).unwrap();
        self.needs_comma = true;
    }

    /// Write a boolean value.
    pub fn value_bool(&mut self, value: bool) {
        self.output.push_str(if value { "true" } else { "false" });
        self.needs_comma = true;
    }

    /// Write a null value.
    pub fn value_null(&mut self) {
        self.output.push_str("null");
        self.needs_comma = true;
    }

    /// Write a key-value pair with a string value.
    pub fn field_str(&mut self, key: &str, value: &str) {
        self.key(key);
        self.value_string(value);
    }

    /// Write a key-value pair with an optional string value.
    /// If None, the field is skipped entirely (per our design decision).
    pub fn field_str_opt(&mut self, key: &str, value: Option<&str>) {
        if let Some(v) = value {
            self.field_str(key, v);
        }
    }

    /// Write a key-value pair with an i64 value.
    #[allow(dead_code)]
    pub fn field_i64(&mut self, key: &str, value: i64) {
        self.key(key);
        self.value_i64(value);
    }

    /// Write a key-value pair with a u64 value.
    pub fn field_u64(&mut self, key: &str, value: u64) {
        self.key(key);
        self.value_u64(value);
    }

    /// Write a key-value pair with an optional u64 value.
    pub fn field_u64_opt(&mut self, key: &str, value: Option<u64>) {
        if let Some(v) = value {
            self.field_u64(key, v);
        }
    }

    /// Write a key-value pair with a boolean value.
    pub fn field_bool(&mut self, key: &str, value: bool) {
        self.key(key);
        self.value_bool(value);
    }

    /// Begin an object value for a key.
    pub fn field_object(&mut self, key: &str) {
        self.key(key);
        self.needs_comma = false;
        // Don't write indent, begin_object_value will handle it
        self.output.push('{');
        self.write_newline();
        self.indent_level += 1;
    }

    /// End an object that was started with field_object.
    pub fn end_field_object(&mut self) {
        self.write_newline();
        self.indent_level -= 1;
        self.write_indent();
        self.output.push('}');
        self.needs_comma = true;
    }

    /// Begin an array value for a key.
    pub fn field_array(&mut self, key: &str) {
        self.key(key);
        self.needs_comma = false;
        self.output.push('[');
        self.write_newline();
        self.indent_level += 1;
    }

    /// End an array that was started with field_array.
    pub fn end_field_array(&mut self) {
        self.write_newline();
        self.indent_level -= 1;
        self.write_indent();
        self.output.push(']');
        self.needs_comma = true;
    }

    /// Write an array element that's a string.
    #[allow(dead_code)]
    pub fn array_string(&mut self, value: &str) {
        self.write_separator();
        self.write_indent();
        self.output.push('"');
        escape_string_into(&mut self.output, value);
        self.output.push('"');
        self.needs_comma = true;
    }

    /// Begin an array element that's an object.
    pub fn array_object_begin(&mut self) {
        self.write_separator();
        self.write_indent();
        self.output.push('{');
        self.write_newline();
        self.indent_level += 1;
        self.needs_comma = false;
    }

    /// End an array element that's an object.
    pub fn array_object_end(&mut self) {
        self.write_newline();
        self.indent_level -= 1;
        self.write_indent();
        self.output.push('}');
        self.needs_comma = true;
    }
}

/// Escape types for JSON string escaping.
/// Inspired by serde_json's approach - use a lookup table for speed.
#[derive(Clone, Copy)]
enum EscapeType {
    /// No escaping needed
    None,
    /// Backspace -> \b
    Backspace,
    /// Tab -> \t
    Tab,
    /// Newline -> \n
    Newline,
    /// Form feed -> \f (rare but valid)
    FormFeed,
    /// Carriage return -> \r
    CarriageReturn,
    /// Quote -> \"
    Quote,
    /// Backslash -> \\
    Backslash,
    /// Other control character -> \uXXXX
    Unicode,
}

/// Lookup table for ASCII characters (0-127).
/// Characters >= 128 are passed through as-is (valid UTF-8).
static ESCAPE_TABLE: [EscapeType; 128] = {
    use EscapeType::*;
    [
        // 0x00 - 0x1F: Control characters
        Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, // 0x00-0x07
        Backspace, Tab, Newline, Unicode, FormFeed, CarriageReturn, Unicode, Unicode, // 0x08-0x0F
        Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, // 0x10-0x17
        Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, Unicode, // 0x18-0x1F
        // 0x20 - 0x7F: Printable ASCII
        None, None, Quote, None, None, None, None, None, // 0x20-0x27 (space, !, ", #, $, %, &, ')
        None, None, None, None, None, None, None, None, // 0x28-0x2F
        None, None, None, None, None, None, None, None, // 0x30-0x37
        None, None, None, None, None, None, None, None, // 0x38-0x3F
        None, None, None, None, None, None, None, None, // 0x40-0x47
        None, None, None, None, None, None, None, None, // 0x48-0x4F
        None, None, None, None, None, None, None, None, // 0x50-0x57
        None, None, None, None, Backslash, None, None, None, // 0x58-0x5F (includes \)
        None, None, None, None, None, None, None, None, // 0x60-0x67
        None, None, None, None, None, None, None, None, // 0x68-0x6F
        None, None, None, None, None, None, None, None, // 0x70-0x77
        None, None, None, None, None, None, None, Unicode, // 0x78-0x7F (DEL is control)
    ]
};

/// Escape a string into the output buffer.
///
/// This handles all the JSON escaping rules:
/// - Control characters (0x00-0x1F) become \uXXXX or their shorthand
/// - Quote becomes \"
/// - Backslash becomes \\
/// - Everything else passes through (including valid UTF-8 > 127)
fn escape_string_into(output: &mut String, s: &str) {
    for c in s.chars() {
        if c.is_ascii() {
            let byte = c as u8;
            match ESCAPE_TABLE[byte as usize] {
                EscapeType::None => output.push(c),
                EscapeType::Backspace => output.push_str("\\b"),
                EscapeType::Tab => output.push_str("\\t"),
                EscapeType::Newline => output.push_str("\\n"),
                EscapeType::FormFeed => output.push_str("\\f"),
                EscapeType::CarriageReturn => output.push_str("\\r"),
                EscapeType::Quote => output.push_str("\\\""),
                EscapeType::Backslash => output.push_str("\\\\"),
                EscapeType::Unicode => {
                    // \uXXXX format for control characters
                    write!(output, "\\u{:04x}", byte).unwrap();
                }
            }
        } else {
            // Non-ASCII UTF-8 characters are passed through as-is.
            // JSON allows them unescaped, and it's more readable.
            output.push(c);
        }
    }
}

/// Helper to create the standard kv JSON envelope.
///
/// All kv JSON output has this structure:
/// ```json
/// {
///   "kv_version": "0.1.0",
///   "subcommand": "pci",
///   "data": ...
/// }
/// ```
pub fn begin_kv_output(pretty: bool, subcommand: &str) -> JsonWriter {
    let mut w = JsonWriter::new(pretty);
    w.begin_object();
    w.field_str("kv_version", env!("CARGO_PKG_VERSION"));
    w.field_str("subcommand", subcommand);
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    mod escape_tests {
        use super::*;

        #[test]
        fn plain_string() {
            let mut out = String::new();
            escape_string_into(&mut out, "hello world");
            assert_eq!(out, "hello world");
        }

        #[test]
        fn string_with_quotes() {
            let mut out = String::new();
            escape_string_into(&mut out, r#"say "hello""#);
            assert_eq!(out, r#"say \"hello\""#);
        }

        #[test]
        fn string_with_backslash() {
            let mut out = String::new();
            escape_string_into(&mut out, r"path\to\file");
            assert_eq!(out, r"path\\to\\file");
        }

        #[test]
        fn string_with_newline() {
            let mut out = String::new();
            escape_string_into(&mut out, "line1\nline2");
            assert_eq!(out, "line1\\nline2");
        }

        #[test]
        fn string_with_tab() {
            let mut out = String::new();
            escape_string_into(&mut out, "col1\tcol2");
            assert_eq!(out, "col1\\tcol2");
        }

        #[test]
        fn string_with_control_char() {
            let mut out = String::new();
            escape_string_into(&mut out, "null\x00char");
            assert_eq!(out, "null\\u0000char");
        }

        #[test]
        fn string_with_unicode() {
            let mut out = String::new();
            escape_string_into(&mut out, "hello 世界 🌍");
            assert_eq!(out, "hello 世界 🌍"); // Unicode passes through
        }

        #[test]
        fn empty_string() {
            let mut out = String::new();
            escape_string_into(&mut out, "");
            assert_eq!(out, "");
        }
    }

    mod writer_tests {
        use super::*;

        #[test]
        fn simple_object_compact() {
            let mut w = JsonWriter::new(false);
            w.begin_object();
            w.field_str("name", "test");
            w.field_u64("count", 42);
            w.end_object();
            let json = w.finish();
            // Compact: no extra whitespace
            assert!(json.contains("\"name\":\"test\""));
            assert!(json.contains("\"count\":42"));
        }

        #[test]
        fn simple_object_pretty() {
            let mut w = JsonWriter::new(true);
            w.begin_object();
            w.field_str("name", "test");
            w.end_object();
            let json = w.finish();
            // Pretty: has newlines and indentation
            assert!(json.contains("\n"));
            assert!(json.contains("  ")); // 2-space indent
        }

        #[test]
        fn nested_object() {
            let mut w = JsonWriter::new(false);
            w.begin_object();
            w.field_object("inner");
            w.field_str("key", "value");
            w.end_field_object();
            w.end_object();
            let json = w.finish();
            assert!(json.contains("\"inner\":{"));
        }

        #[test]
        fn array_of_strings() {
            let mut w = JsonWriter::new(false);
            w.begin_object();
            w.field_array("items");
            w.array_string("one");
            w.array_string("two");
            w.end_field_array();
            w.end_object();
            let json = w.finish();
            assert!(json.contains("["));
            assert!(json.contains("\"one\""));
            assert!(json.contains("\"two\""));
        }

        #[test]
        fn boolean_values() {
            let mut w = JsonWriter::new(false);
            w.begin_object();
            w.field_bool("active", true);
            w.field_bool("deleted", false);
            w.end_object();
            let json = w.finish();
            assert!(json.contains("\"active\":true"));
            assert!(json.contains("\"deleted\":false"));
        }

        #[test]
        fn kv_envelope() {
            let mut w = begin_kv_output(false, "test");
            w.field_array("data");
            w.end_field_array();
            w.end_object();
            let json = w.finish();
            assert!(json.contains("\"kv_version\":"));
            assert!(json.contains("\"subcommand\":\"test\""));
        }

        #[test]
        fn optional_fields() {
            let mut w = JsonWriter::new(false);
            w.begin_object();
            w.field_str_opt("present", Some("yes"));
            w.field_str_opt("absent", None);
            w.field_u64_opt("num_present", Some(42));
            w.field_u64_opt("num_absent", None);
            w.end_object();
            let json = w.finish();
            assert!(json.contains("\"present\":\"yes\""));
            assert!(!json.contains("absent")); // None fields are skipped
            assert!(json.contains("\"num_present\":42"));
            assert!(!json.contains("num_absent"));
        }
    }
}
