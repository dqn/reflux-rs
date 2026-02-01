mod bytes;
pub mod chunked_reader;
pub mod layout;
pub mod pattern;
mod process;
pub mod provider;
mod reader;

#[cfg(test)]
pub mod mock;

pub use bytes::{ByteBuffer, decode_shift_jis, decode_shift_jis_to_string};
pub use chunked_reader::{ChunkedMemoryIterator, MemoryChunk, DEFAULT_CHUNK_SIZE};
pub use process::*;
pub use provider::{ProcessInfo, ProcessProvider};
pub use reader::{MemoryReader, ReadMemory};

#[cfg(test)]
pub use mock::{MockMemoryBuilder, MockMemoryReader};
