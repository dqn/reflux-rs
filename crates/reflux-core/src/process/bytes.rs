//! Byte buffer utilities for parsing binary data structures.
//!
//! This module provides `ByteBuffer`, a position-tracking byte reader for parsing
//! binary data from game memory. It consolidates common byte parsing patterns used
//! throughout the codebase.

use std::sync::Arc;

use encoding_rs::SHIFT_JIS;
use tracing::debug;

use crate::error::{Error, Result};

/// A position-tracking byte reader for parsing binary data structures.
///
/// `ByteBuffer` wraps a byte slice and maintains a current position, allowing
/// sequential reads of primitive types. This eliminates the need for manual
/// offset calculations and reduces code duplication in binary parsing code.
///
/// # Example
///
/// ```
/// use reflux_core::process::ByteBuffer;
///
/// let data = [0x78, 0x56, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00];
/// let mut buf = ByteBuffer::new(&data);
///
/// let value = buf.read_i32().unwrap();
/// assert_eq!(value, 0x12345678);
/// assert_eq!(buf.position(), 4);
/// ```
pub struct ByteBuffer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteBuffer<'a> {
    /// Creates a new `ByteBuffer` wrapping the given byte slice.
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Returns the current read position.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Returns the total length of the underlying buffer.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns true if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Returns the number of bytes remaining from the current position.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Sets the current read position.
    ///
    /// # Errors
    ///
    /// Returns an error if the position is beyond the buffer length.
    pub fn set_position(&mut self, pos: usize) -> Result<()> {
        if pos > self.data.len() {
            return Err(Error::MemoryReadFailed {
                address: pos as u64,
                message: format!("Position {} exceeds buffer length {}", pos, self.data.len()),
            });
        }
        self.pos = pos;
        Ok(())
    }

    /// Skips the specified number of bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if skipping would go beyond the buffer length.
    pub fn skip(&mut self, count: usize) -> Result<()> {
        self.set_position(self.pos + count)
    }

    /// Returns a slice of bytes at the specified offset without advancing position.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds.
    pub fn slice_at(&self, offset: usize, len: usize) -> Result<&'a [u8]> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| Error::MemoryReadFailed {
                address: offset as u64,
                message: "Offset overflow".to_string(),
            })?;

        if end > self.data.len() {
            return Err(Error::MemoryReadFailed {
                address: offset as u64,
                message: format!(
                    "Slice range {}..{} exceeds buffer length {}",
                    offset,
                    end,
                    self.data.len()
                ),
            });
        }

        Ok(&self.data[offset..end])
    }

    /// Reads a signed 8-bit integer and advances the position.
    pub fn read_i8(&mut self) -> Result<i8> {
        let bytes = self.read_bytes(1)?;
        Ok(bytes[0] as i8)
    }

    /// Reads an unsigned 8-bit integer and advances the position.
    pub fn read_u8(&mut self) -> Result<u8> {
        let bytes = self.read_bytes(1)?;
        Ok(bytes[0])
    }

    /// Reads a signed 16-bit integer (little-endian) and advances the position.
    pub fn read_i16(&mut self) -> Result<i16> {
        let bytes = self.read_bytes(2)?;
        Ok(i16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads an unsigned 16-bit integer (little-endian) and advances the position.
    pub fn read_u16(&mut self) -> Result<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Reads a signed 32-bit integer (little-endian) and advances the position.
    pub fn read_i32(&mut self) -> Result<i32> {
        let bytes = self.read_bytes(4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads an unsigned 32-bit integer (little-endian) and advances the position.
    pub fn read_u32(&mut self) -> Result<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads a signed 64-bit integer (little-endian) and advances the position.
    pub fn read_i64(&mut self) -> Result<i64> {
        let bytes = self.read_bytes(8)?;
        Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads an unsigned 64-bit integer (little-endian) and advances the position.
    pub fn read_u64(&mut self) -> Result<u64> {
        let bytes = self.read_bytes(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Reads the specified number of bytes and advances the position.
    ///
    /// # Errors
    ///
    /// Returns an error if there are not enough bytes remaining.
    pub fn read_bytes(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or_else(|| Error::MemoryReadFailed {
                address: self.pos as u64,
                message: "Position overflow".to_string(),
            })?;

        if end > self.data.len() {
            return Err(Error::MemoryReadFailed {
                address: self.pos as u64,
                message: format!(
                    "Read of {} bytes at position {} exceeds buffer length {}",
                    count,
                    self.pos,
                    self.data.len()
                ),
            });
        }

        let result = &self.data[self.pos..end];
        self.pos = end;
        Ok(result)
    }

    /// Reads a Shift-JIS encoded string of the specified maximum length.
    ///
    /// The string is terminated at the first null byte or at `max_len`.
    /// Returns the decoded string as an `Arc<str>`.
    pub fn read_shift_jis_string(&mut self, max_len: usize) -> Result<Arc<str>> {
        let bytes = self.read_bytes(max_len)?;
        Ok(decode_shift_jis(bytes))
    }

    /// Reads a signed 32-bit integer at the specified offset without advancing position.
    pub fn read_i32_at(&self, offset: usize) -> Result<i32> {
        let bytes = self.slice_at(offset, 4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads an unsigned 32-bit integer at the specified offset without advancing position.
    pub fn read_u32_at(&self, offset: usize) -> Result<u32> {
        let bytes = self.slice_at(offset, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Reads an unsigned 64-bit integer at the specified offset without advancing position.
    pub fn read_u64_at(&self, offset: usize) -> Result<u64> {
        let bytes = self.slice_at(offset, 8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }
}

/// Decodes Shift-JIS bytes to `Arc<str>`, removing null terminators.
///
/// This is a utility function for decoding Japanese text from game memory.
pub fn decode_shift_jis(bytes: &[u8]) -> Arc<str> {
    // Find null terminator
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let bytes = &bytes[..len];

    let (decoded, _, had_errors) = SHIFT_JIS.decode(bytes);
    if had_errors {
        debug!(
            "Shift-JIS decoding had errors for bytes: {:?}",
            &bytes[..bytes.len().min(20)]
        );
    }
    Arc::from(decoded.into_owned())
}

/// Decodes Shift-JIS bytes to `String`, removing null terminators.
///
/// This is a simpler variant that returns a `String` instead of `Arc<str>`.
pub fn decode_shift_jis_to_string(bytes: &[u8]) -> String {
    let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let (decoded, _, _) = SHIFT_JIS.decode(&bytes[..len]);
    decoded.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_buffer_read_i32() {
        let data = [0x78, 0x56, 0x34, 0x12];
        let mut buf = ByteBuffer::new(&data);

        let value = buf.read_i32().unwrap();
        assert_eq!(value, 0x12345678);
        assert_eq!(buf.position(), 4);
    }

    #[test]
    fn test_byte_buffer_read_u64() {
        let data = [0xEF, 0xCD, 0xAB, 0x90, 0x78, 0x56, 0x34, 0x12];
        let mut buf = ByteBuffer::new(&data);

        let value = buf.read_u64().unwrap();
        assert_eq!(value, 0x1234567890ABCDEF);
        assert_eq!(buf.position(), 8);
    }

    #[test]
    fn test_byte_buffer_sequential_reads() {
        let data = [
            0x01, 0x00, 0x00, 0x00, // i32: 1
            0x02, 0x00, 0x00, 0x00, // i32: 2
            0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // u64: 3
        ];
        let mut buf = ByteBuffer::new(&data);

        assert_eq!(buf.read_i32().unwrap(), 1);
        assert_eq!(buf.read_i32().unwrap(), 2);
        assert_eq!(buf.read_u64().unwrap(), 3);
        assert_eq!(buf.position(), 16);
    }

    #[test]
    fn test_byte_buffer_read_at() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let buf = ByteBuffer::new(&data);

        // Read at offset 4 without changing position
        let value = buf.read_u32_at(4).unwrap();
        assert_eq!(value, 0x08070605);
        assert_eq!(buf.position(), 0); // Position unchanged
    }

    #[test]
    fn test_byte_buffer_skip() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let mut buf = ByteBuffer::new(&data);

        buf.skip(4).unwrap();
        assert_eq!(buf.position(), 4);

        let value = buf.read_u32().unwrap();
        assert_eq!(value, 0x08070605);
    }

    #[test]
    fn test_byte_buffer_overflow_error() {
        let data = [0x01, 0x02];
        let mut buf = ByteBuffer::new(&data);

        let result = buf.read_i32();
        assert!(result.is_err());
    }

    #[test]
    fn test_byte_buffer_set_position() {
        let data = [0x01, 0x02, 0x03, 0x04];
        let mut buf = ByteBuffer::new(&data);

        buf.set_position(2).unwrap();
        assert_eq!(buf.position(), 2);

        let result = buf.set_position(10);
        assert!(result.is_err());
    }

    #[test]
    fn test_decode_shift_jis() {
        // "テスト" in Shift-JIS
        let data = [0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0x00];
        let result = decode_shift_jis(&data);
        assert_eq!(&*result, "テスト");
    }

    #[test]
    fn test_decode_shift_jis_no_null() {
        // "ABC" without null terminator
        let data = [0x41, 0x42, 0x43];
        let result = decode_shift_jis(&data);
        assert_eq!(&*result, "ABC");
    }

    #[test]
    fn test_byte_buffer_read_shift_jis_string() {
        // "テスト" followed by padding
        let mut data = vec![0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0x00];
        data.resize(64, 0); // Pad to 64 bytes

        let mut buf = ByteBuffer::new(&data);
        let result = buf.read_shift_jis_string(64).unwrap();
        assert_eq!(&*result, "テスト");
        assert_eq!(buf.position(), 64);
    }

    #[test]
    fn test_byte_buffer_remaining() {
        let data = [0x01, 0x02, 0x03, 0x04];
        let mut buf = ByteBuffer::new(&data);

        assert_eq!(buf.remaining(), 4);
        buf.skip(2).unwrap();
        assert_eq!(buf.remaining(), 2);
    }
}
