#![cfg_attr(not(target_os = "windows"), allow(dead_code, unused_variables))]

use crate::error::{Error, Result};
use crate::memory::ProcessHandle;
use encoding_rs::SHIFT_JIS;

#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;

/// Trait for reading memory from a process or buffer
///
/// This trait enables mocking for tests and abstracts over different memory sources.
pub trait ReadMemory {
    /// Read raw bytes from memory at the given address
    fn read_bytes(&self, address: u64, size: usize) -> Result<Vec<u8>>;

    /// Get the base address of the memory region
    fn base_address(&self) -> u64;

    /// Read a signed 32-bit integer from memory
    fn read_i32(&self, address: u64) -> Result<i32> {
        let bytes = self.read_bytes(address, 4)?;
        Ok(i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read an unsigned 32-bit integer from memory
    fn read_u32(&self, address: u64) -> Result<u32> {
        let bytes = self.read_bytes(address, 4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a signed 64-bit integer from memory
    fn read_i64(&self, address: u64) -> Result<i64> {
        let bytes = self.read_bytes(address, 8)?;
        Ok(i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Read an unsigned 64-bit integer from memory
    fn read_u64(&self, address: u64) -> Result<u64> {
        let bytes = self.read_bytes(address, 8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// Read a Shift-JIS encoded string from memory
    fn read_string_shift_jis(&self, address: u64, max_len: usize) -> Result<String> {
        let bytes = self.read_bytes(address, max_len)?;

        // Find null terminator
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let bytes = &bytes[..len];

        let (decoded, _, had_errors) = SHIFT_JIS.decode(bytes);
        if had_errors {
            return Err(Error::EncodingError(
                "Failed to decode Shift-JIS string".to_string(),
            ));
        }

        Ok(decoded.into_owned())
    }

    /// Read a UTF-8 encoded string from memory
    fn read_string_utf8(&self, address: u64, max_len: usize) -> Result<String> {
        let bytes = self.read_bytes(address, max_len)?;

        // Find null terminator
        let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let bytes = &bytes[..len];

        String::from_utf8(bytes.to_vec())
            .map_err(|e| Error::EncodingError(format!("Failed to decode UTF-8 string: {}", e)))
    }
}

pub struct MemoryReader<'a> {
    process: &'a ProcessHandle,
}

impl<'a> MemoryReader<'a> {
    pub fn new(process: &'a ProcessHandle) -> Self {
        Self { process }
    }

    #[cfg(target_os = "windows")]
    fn read_bytes_impl(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        let mut buffer = vec![0u8; size];
        let mut bytes_read = 0;

        // SAFETY: ReadProcessMemory is called with:
        // - A valid process handle from ProcessHandle (obtained via OpenProcess with PROCESS_VM_READ)
        // - An address within the target process's address space
        // - A properly allocated buffer of the requested size
        // - A pointer to receive the actual bytes read
        // The function may fail if the address is invalid, but this is handled via Result.
        unsafe {
            ReadProcessMemory(
                self.process.handle(),
                address as *const _,
                buffer.as_mut_ptr() as *mut _,
                size,
                Some(&mut bytes_read),
            )
            .map_err(|e| Error::MemoryReadFailed {
                address,
                message: e.to_string(),
            })?;
        }

        // This function guarantees all-or-nothing reads. Partial reads are treated as errors
        // because game memory structures require complete data for correct interpretation.
        if bytes_read != size {
            return Err(Error::MemoryReadFailed {
                address,
                message: format!("Expected {} bytes, read {}", size, bytes_read),
            });
        }

        Ok(buffer)
    }

    #[cfg(not(target_os = "windows"))]
    fn read_bytes_impl(&self, address: u64, _size: usize) -> Result<Vec<u8>> {
        Err(Error::MemoryReadFailed {
            address,
            message: "Windows only: memory reading not supported on this platform".to_string(),
        })
    }
}

impl ReadMemory for MemoryReader<'_> {
    fn read_bytes(&self, address: u64, size: usize) -> Result<Vec<u8>> {
        self.read_bytes_impl(address, size)
    }

    fn base_address(&self) -> u64 {
        self.process.base_address
    }
}
