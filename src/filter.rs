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
//!
//! ```ignore
//! impl Filterable for MyDevice {
//!     fn filter_fields(&self) -> Vec<&str> {
//!         vec![
//!             &self.name,
//!             opt_str(&self.model),
//!             opt_str(&self.driver),
//!         ]
//!     }
//! }
//! ```

/// Trait for types that can be filtered by pattern matching.
///
/// Implement `filter_fields()` to return the fields that should be searched.
/// The `matches_filter()` method is provided with a default implementation.
pub trait Filterable {
    /// Returns the fields to match against as string slices.
    ///
    /// Use `opt_str()` for Option<String> fields.
    fn filter_fields(&self) -> Vec<&str>;

    /// Check if this item matches a filter pattern.
    ///
    /// Default implementation uses `matches_any()` on the fields from `filter_fields()`.
    fn matches_filter(&self, pattern: &str, case_insensitive: bool) -> bool {
        matches_any(&self.filter_fields(), pattern, case_insensitive)
    }
}

/// Extract `&str` from `Option<String>`, returning `""` if `None`.
///
/// Convenience helper for building filter field lists.
///
/// # Example
///
/// ```ignore
/// vec![&self.name, opt_str(&self.model), opt_str(&self.driver)]
/// ```
#[inline]
pub fn opt_str(opt: &Option<String>) -> &str {
    opt.as_deref().unwrap_or("")
}

/// Check if any of the given fields contain the pattern.
///
/// When `case_insensitive` is true, the pattern is assumed to be already
/// lowercased (done by CLI parser when `-F` is used). Each field is lowercased
/// before comparison.
///
/// # Example
///
/// ```ignore
/// let fields = [name, model.unwrap_or(""), driver.unwrap_or("")];
/// filter::matches_any(&fields, pattern, case_insensitive)
/// ```
pub fn matches_any(fields: &[&str], pattern: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        fields.iter().any(|f| f.to_lowercase().contains(pattern))
    } else {
        fields.iter().any(|f| f.contains(pattern))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_sensitive_match() {
        let fields = ["Hello", "World"];
        assert!(matches_any(&fields, "ello", false));
        assert!(matches_any(&fields, "World", false));
        assert!(!matches_any(&fields, "world", false)); // case matters
    }

    #[test]
    fn case_insensitive_match() {
        let fields = ["Hello", "World"];
        // Pattern should be pre-lowercased when case_insensitive=true
        assert!(matches_any(&fields, "hello", true));
        assert!(matches_any(&fields, "world", true));
        assert!(matches_any(&fields, "ello", true));
    }

    #[test]
    fn no_match() {
        let fields = ["foo", "bar"];
        assert!(!matches_any(&fields, "baz", false));
        assert!(!matches_any(&fields, "baz", true));
    }

    #[test]
    fn empty_fields() {
        let fields: [&str; 0] = [];
        assert!(!matches_any(&fields, "x", false));
    }

    #[test]
    fn opt_str_some() {
        let s = Some("hello".to_string());
        assert_eq!(opt_str(&s), "hello");
    }

    #[test]
    fn opt_str_none() {
        let s: Option<String> = None;
        assert_eq!(opt_str(&s), "");
    }

    // Test the Filterable trait with a mock struct
    struct MockDevice {
        name: String,
        model: Option<String>,
    }

    impl Filterable for MockDevice {
        fn filter_fields(&self) -> Vec<&str> {
            vec![&self.name, opt_str(&self.model)]
        }
    }

    #[test]
    fn filterable_trait_matches() {
        let dev = MockDevice {
            name: "test-device".to_string(),
            model: Some("Model X".to_string()),
        };

        assert!(dev.matches_filter("test", false));
        assert!(dev.matches_filter("Model", false));
        assert!(!dev.matches_filter("model", false)); // case sensitive
        assert!(dev.matches_filter("model", true));   // case insensitive
    }

    #[test]
    fn filterable_trait_with_none() {
        let dev = MockDevice {
            name: "device".to_string(),
            model: None,
        };

        assert!(dev.matches_filter("device", false));
        assert!(!dev.matches_filter("Model", false));
    }
}
