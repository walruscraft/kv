//! Filter matching utilities.
//!
//! Provides centralized case-sensitive/insensitive matching so individual
//! subcommand modules don't need to handle this logic.
//!
//! # For Contributors
//!
//! When adding a new subcommand with filterable items, implement the `Filterable`
//! trait on your struct. You only need to implement `filter_fields()` - the
//! `matches_filter()` method is provided automatically.

#![allow(dead_code)]

use crate::stack::StackString;

/// Check if any of the given fields contain the pattern.
///
/// When `case_insensitive` is true, the pattern is assumed to be already
/// lowercased (done by CLI parser when `-F` is used). Each field is lowercased
/// before comparison.
pub fn matches_any(fields: &[&str], pattern: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        // Need to lowercase each field for comparison
        // Use a stack buffer for the lowercase version
        for field in fields {
            if contains_lowercase(field, pattern) {
                return true;
            }
        }
        false
    } else {
        fields.iter().any(|f| f.contains(pattern))
    }
}

/// Check if field (lowercased) contains pattern.
/// Pattern is assumed to be already lowercase.
fn contains_lowercase(field: &str, pattern: &str) -> bool {
    // Simple brute-force search with case folding
    let pattern_len = pattern.len();
    if pattern_len == 0 {
        return true;
    }
    if field.len() < pattern_len {
        return false;
    }

    // Convert field to lowercase into a stack buffer
    let mut lower: StackString<256> = StackString::new();
    for c in field.chars() {
        for lc in c.to_lowercase() {
            lower.push(lc);
        }
    }

    lower.as_str().contains(pattern)
}

/// Extract `&str` from `Option<T>` where T implements AsRef<str>.
/// Returns `""` if `None`.
#[inline]
pub fn opt_str<T: AsRef<str>>(opt: &Option<T>) -> &str {
    opt.as_ref().map(|s| s.as_ref()).unwrap_or("")
}

#[cfg(test)]
mod tests {
    // Tests removed for no_std build
}
