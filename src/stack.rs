//! Stack-allocated string and buffer types for no-alloc operation.
//!
//! These types provide fixed-capacity alternatives to String and Vec
//! that don't require a heap allocator. They're designed for the
//! constrained environment of sysfs/procfs reading where we know
//! reasonable upper bounds on data sizes.

#![allow(dead_code)]

/// A stack-allocated string with fixed capacity.
/// N is the capacity in bytes.
#[derive(Clone)]
pub struct StackString<const N: usize> {
    buf: [u8; N],
    len: usize,
}


impl<const N: usize> StackString<N> {
    /// Create a new empty StackString.
    #[inline]
    pub const fn new() -> Self {
        Self {
            buf: [0u8; N],
            len: 0,
        }
    }

    /// Create from a string slice, truncating if necessary.
    #[inline]
    pub fn from_str(s: &str) -> Self {
        let mut this = Self::new();
        this.push_str(s);
        this
    }

    /// Get the string as a slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: We only ever write valid UTF-8
        unsafe { core::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }

    /// Get the length in bytes.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Get remaining capacity.
    #[inline]
    pub fn remaining(&self) -> usize {
        N - self.len
    }

    /// Push a character, returning false if full.
    #[inline]
    pub fn push(&mut self, c: char) -> bool {
        let mut buf = [0u8; 4];
        let encoded = c.encode_utf8(&mut buf);
        self.push_str(encoded)
    }

    /// Push a string slice, returning true if it fit (or was truncated).
    #[inline]
    pub fn push_str(&mut self, s: &str) -> bool {
        let bytes = s.as_bytes();
        let to_copy = bytes.len().min(self.remaining());
        if to_copy > 0 {
            self.buf[self.len..self.len + to_copy].copy_from_slice(&bytes[..to_copy]);
            self.len += to_copy;
        }
        to_copy == bytes.len()
    }

    /// Clear the string.
    #[inline]
    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Trim whitespace from both ends (returns a new StackString).
    pub fn trim(&self) -> StackString<N> {
        StackString::from_str(self.as_str().trim())
    }
}

impl<const N: usize> Default for StackString<N> {
    fn default() -> Self {
        Self::new()
    }
}


impl<const N: usize> core::ops::Deref for StackString<N> {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl<const N: usize> AsRef<str> for StackString<N> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

/// A stack-allocated buffer for reading files.
pub struct StackBuf<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> StackBuf<N> {
    /// Create a new empty buffer.
    #[inline]
    pub const fn new() -> Self {
        Self {
            buf: [0u8; N],
            len: 0,
        }
    }

    /// Get the buffer as a mutable slice for reading into.
    #[inline]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.buf
    }

    /// Set the length after reading.
    #[inline]
    pub fn set_len(&mut self, len: usize) {
        self.len = len.min(N);
    }

    /// Get the filled portion as a byte slice.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Try to interpret as UTF-8 string.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        core::str::from_utf8(&self.buf[..self.len]).ok()
    }

    /// Get as trimmed string.
    pub fn as_str_trimmed(&self) -> Option<&str> {
        self.as_str().map(|s| s.trim())
    }
}

impl<const N: usize> Default for StackBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Push an integer to a StackString using itoa.
pub fn push_u64<const N: usize>(s: &mut StackString<N>, val: u64) {
    let mut buf = itoa::Buffer::new();
    s.push_str(buf.format(val));
}

/// Push an integer to a StackString using itoa.
pub fn push_i64<const N: usize>(s: &mut StackString<N>, val: i64) {
    let mut buf = itoa::Buffer::new();
    s.push_str(buf.format(val));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_string_basic() {
        let mut s: StackString<32> = StackString::new();
        assert!(s.is_empty());
        s.push_str("hello");
        assert_eq!(s.as_str(), "hello");
        s.push(' ');
        s.push_str("world");
        assert_eq!(s.as_str(), "hello world");
    }

    #[test]
    fn test_stack_string_truncate() {
        let mut s: StackString<5> = StackString::new();
        s.push_str("hello world"); // Should truncate
        assert_eq!(s.as_str(), "hello");
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn test_stack_string_trim() {
        let s: StackString<32> = StackString::from_str("  hello  ");
        let trimmed = s.trim();
        assert_eq!(trimmed.as_str(), "hello");
    }
}
