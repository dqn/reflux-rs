mod bytes;
pub mod chunked_reader;
mod handle;
pub mod layout;
pub mod pattern;
pub mod provider;
mod reader;

#[cfg(test)]
pub mod mock;

pub use bytes::{ByteBuffer, decode_shift_jis, decode_shift_jis_to_string};
pub use chunked_reader::{ChunkedMemoryIterator, MemoryChunk, DEFAULT_CHUNK_SIZE};
pub use handle::*;
pub use provider::{ProcessInfo, ProcessProvider};
pub use reader::{MemoryReader, ReadMemory};

#[cfg(test)]
pub use mock::{MockMemoryBuilder, MockMemoryReader};
