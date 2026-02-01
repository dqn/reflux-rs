//! Chunked memory reading utilities.
//!
//! This module provides an iterator-based approach to reading large memory regions
//! in fixed-size chunks, avoiding memory pressure from loading entire regions at once.

use super::ReadMemory;
use crate::error::Result;

/// Default chunk size for memory reading (4MB).
pub const DEFAULT_CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// A chunk of memory read from a process.
#[derive(Debug)]
pub struct MemoryChunk {
    /// Starting address of this chunk.
    pub address: u64,
    /// The actual bytes read.
    pub data: Vec<u8>,
}

/// Iterator that reads memory in fixed-size chunks.
///
/// This is useful for searching large memory regions without loading
/// everything into memory at once.
///
/// # Example
///
/// ```ignore
/// use reflux_core::memory::{ChunkedMemoryIterator, DEFAULT_CHUNK_SIZE};
///
/// let iter = ChunkedMemoryIterator::new(&reader, start, end, DEFAULT_CHUNK_SIZE);
/// for chunk in iter {
///     if let Ok(chunk) = chunk {
///         // Process chunk.data
///     }
/// }
/// ```
pub struct ChunkedMemoryIterator<'a, R: ReadMemory> {
    reader: &'a R,
    current: u64,
    end: u64,
    chunk_size: usize,
}

impl<'a, R: ReadMemory> ChunkedMemoryIterator<'a, R> {
    /// Create a new chunked memory iterator.
    ///
    /// # Arguments
    ///
    /// * `reader` - The memory reader to use
    /// * `start` - Starting address
    /// * `end` - Ending address (exclusive)
    /// * `chunk_size` - Size of each chunk to read
    pub fn new(reader: &'a R, start: u64, end: u64, chunk_size: usize) -> Self {
        Self {
            reader,
            current: start,
            end,
            chunk_size,
        }
    }

    /// Create a new chunked memory iterator with default chunk size.
    pub fn with_default_chunk_size(reader: &'a R, start: u64, end: u64) -> Self {
        Self::new(reader, start, end, DEFAULT_CHUNK_SIZE)
    }
}

impl<R: ReadMemory> Iterator for ChunkedMemoryIterator<'_, R> {
    type Item = Result<MemoryChunk>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.end {
            return None;
        }

        let read_size = self.chunk_size.min((self.end - self.current) as usize);
        let address = self.current;
        self.current += read_size as u64;

        Some(
            self.reader
                .read_bytes(address, read_size)
                .map(|data| MemoryChunk { address, data }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::mock::MockMemoryBuilder;

    #[test]
    fn test_chunked_iterator_single_chunk() {
        let reader = MockMemoryBuilder::new()
            .write_bytes(0, &[1, 2, 3, 4, 5, 6, 7, 8])
            .build();

        let chunks: Vec<_> = ChunkedMemoryIterator::new(&reader, 0x1000, 0x1008, 16)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].address, 0x1000);
        assert_eq!(chunks[0].data, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn test_chunked_iterator_multiple_chunks() {
        let reader = MockMemoryBuilder::new()
            .write_bytes(0, &[1, 2, 3, 4, 5, 6, 7, 8])
            .build();

        let chunks: Vec<_> = ChunkedMemoryIterator::new(&reader, 0x1000, 0x1008, 4)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].address, 0x1000);
        assert_eq!(chunks[0].data, vec![1, 2, 3, 4]);
        assert_eq!(chunks[1].address, 0x1004);
        assert_eq!(chunks[1].data, vec![5, 6, 7, 8]);
    }

    #[test]
    fn test_chunked_iterator_empty_range() {
        let reader = MockMemoryBuilder::new()
            .write_bytes(0, &[1, 2, 3, 4])
            .build();

        let chunks: Vec<_> = ChunkedMemoryIterator::new(&reader, 0x1000, 0x1000, 4)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunked_iterator_partial_last_chunk() {
        let reader = MockMemoryBuilder::new()
            .write_bytes(0, &[1, 2, 3, 4, 5])
            .build();

        let chunks: Vec<_> = ChunkedMemoryIterator::new(&reader, 0x1000, 0x1005, 4)
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].data.len(), 4);
        assert_eq!(chunks[1].data.len(), 1);
        assert_eq!(chunks[1].data, vec![5]);
    }
}
