//! Mock memory reader for testing
//!
//! Provides a configurable mock implementation of ReadMemory trait
//! that reads from an in-memory buffer instead of a real process.

use crate::error::{Error, Result};
use crate::process::ReadMemory;

/// Mock memory reader for testing
///
/// Reads from an in-memory buffer, allowing tests to verify memory reading
/// logic without requiring access to a real process.
#[derive(Debug, Clone)]
pub struct MockMemoryReader {
    data: Vec<u8>,
    base: u64,
}

impl MockMemoryReader {
    /// Create a new mock reader with the given data at base address 0x1000
    pub fn new(data: Vec<u8>) -> Self {
        Self { data, base: 0x1000 }
    }

    /// Create a new mock reader with custom base address
    pub fn with_base(data: Vec<u8>, base: u64) -> Self {
        Self { data, base }
    }

    /// Get the size of the underlying buffer
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl ReadMemory for MockMemoryReader {
    fn read_bytes(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        if address < self.base {
            return Err(Error::MemoryReadFailed {
                address,
                message: format!("Address below base (base=0x{:X})", self.base),
            });
        }
        let offset = (address - self.base) as usize;
        if offset + size > self.data.len() {
            return Err(Error::MemoryReadFailed {
                address,
                message: format!(
                    "Out of bounds: offset={}, size={}, len={}",
                    offset,
                    size,
                    self.data.len()
                ),
            });
        }
        Ok(self.data[offset..offset + size].to_vec())
    }

    fn base_address(&self) -> u64 {
        self.base
    }
}

/// Builder for creating test memory buffers
///
/// Provides a fluent API for constructing memory layouts for testing.
#[derive(Debug, Clone, Default)]
pub struct MockMemoryBuilder {
    data: Vec<u8>,
    base: u64,
}

impl MockMemoryBuilder {
    /// Create a new builder with default base address (0x1000)
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            base: 0x1000,
        }
    }

    /// Set the base address for the mock reader
    pub fn base(mut self, base: u64) -> Self {
        self.base = base;
        self
    }

    /// Pre-allocate buffer with zeros up to the specified size
    pub fn with_size(mut self, size: usize) -> Self {
        self.data.resize(size, 0);
        self
    }

    /// Write a signed 32-bit integer at the specified offset from base
    pub fn write_i32(mut self, offset: usize, value: i32) -> Self {
        self.ensure_size(offset + 4);
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        self
    }

    /// Write an unsigned 32-bit integer at the specified offset from base
    pub fn write_u32(mut self, offset: usize, value: u32) -> Self {
        self.ensure_size(offset + 4);
        self.data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        self
    }

    /// Write a signed 64-bit integer at the specified offset from base
    pub fn write_i64(mut self, offset: usize, value: i64) -> Self {
        self.ensure_size(offset + 8);
        self.data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        self
    }

    /// Write an unsigned 64-bit integer at the specified offset from base
    pub fn write_u64(mut self, offset: usize, value: u64) -> Self {
        self.ensure_size(offset + 8);
        self.data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        self
    }

    /// Write raw bytes at the specified offset from base
    pub fn write_bytes(mut self, offset: usize, bytes: &[u8]) -> Self {
        self.ensure_size(offset + bytes.len());
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
        self
    }

    /// Write a null-terminated Shift-JIS string at the specified offset
    pub fn write_shift_jis(mut self, offset: usize, text: &str) -> Self {
        use encoding_rs::SHIFT_JIS;
        let (encoded, _, _) = SHIFT_JIS.encode(text);
        let bytes = encoded.into_owned();
        self.ensure_size(offset + bytes.len() + 1);
        self.data[offset..offset + bytes.len()].copy_from_slice(&bytes);
        self.data[offset + bytes.len()] = 0; // null terminator
        self
    }

    /// Write a null-terminated UTF-8 string at the specified offset
    pub fn write_utf8(mut self, offset: usize, text: &str) -> Self {
        let bytes = text.as_bytes();
        self.ensure_size(offset + bytes.len() + 1);
        self.data[offset..offset + bytes.len()].copy_from_slice(bytes);
        self.data[offset + bytes.len()] = 0; // null terminator
        self
    }

    /// Build the MockMemoryReader
    pub fn build(self) -> MockMemoryReader {
        MockMemoryReader {
            data: self.data,
            base: self.base,
        }
    }

    fn ensure_size(&mut self, required: usize) {
        if self.data.len() < required {
            self.data.resize(required, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_reader_basic() {
        let data = vec![0x78, 0x56, 0x34, 0x12];
        let reader = MockMemoryReader::new(data);

        let value = reader.read_i32(0x1000).unwrap();
        assert_eq!(value, 0x12345678);
    }

    #[test]
    fn test_mock_reader_with_base() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let reader = MockMemoryReader::with_base(data, 0x140000000);

        let bytes = reader.read_bytes(0x140000000, 4).unwrap();
        assert_eq!(bytes, vec![0x01, 0x02, 0x03, 0x04]);
    }

    #[test]
    fn test_mock_reader_out_of_bounds() {
        let data = vec![0x01, 0x02];
        let reader = MockMemoryReader::new(data);

        let result = reader.read_u32(0x1000);
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_reader_below_base() {
        let data = vec![0x01, 0x02, 0x03, 0x04];
        let reader = MockMemoryReader::with_base(data, 0x2000);

        let result = reader.read_bytes(0x1000, 4);
        assert!(result.is_err());
    }

    #[test]
    fn test_builder_basic() {
        let reader = MockMemoryBuilder::new()
            .write_i32(0, 0x12345678)
            .write_u64(4, 0xDEADBEEFCAFEBABE)
            .build();

        assert_eq!(reader.read_i32(0x1000).unwrap(), 0x12345678);
        assert_eq!(reader.read_u64(0x1004).unwrap(), 0xDEADBEEFCAFEBABE);
    }

    #[test]
    fn test_builder_with_base() {
        let reader = MockMemoryBuilder::new()
            .base(0x140000000)
            .write_i32(0, 42)
            .build();

        assert_eq!(reader.base_address(), 0x140000000);
        assert_eq!(reader.read_i32(0x140000000).unwrap(), 42);
    }

    #[test]
    fn test_builder_with_size() {
        let reader = MockMemoryBuilder::new()
            .with_size(100)
            .write_i32(96, 123)
            .build();

        assert_eq!(reader.len(), 100);
        assert_eq!(reader.read_i32(0x1000 + 96).unwrap(), 123);
    }

    #[test]
    fn test_builder_shift_jis() {
        let reader = MockMemoryBuilder::new()
            .with_size(16)
            .write_shift_jis(0, "テスト")
            .build();

        let value = reader.read_string_shift_jis(0x1000, 10).unwrap();
        assert_eq!(value, "テスト");
    }

    #[test]
    fn test_builder_utf8() {
        let reader = MockMemoryBuilder::new()
            .with_size(16)
            .write_utf8(0, "Hello")
            .build();

        let value = reader.read_string_utf8(0x1000, 10).unwrap();
        assert_eq!(value, "Hello");
    }

    #[test]
    fn test_builder_raw_bytes() {
        let reader = MockMemoryBuilder::new()
            .write_bytes(0, &[0xDE, 0xAD, 0xBE, 0xEF])
            .build();

        let bytes = reader.read_bytes(0x1000, 4).unwrap();
        assert_eq!(bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);
    }
}
