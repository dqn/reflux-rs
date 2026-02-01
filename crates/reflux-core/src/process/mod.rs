mod bytes;
pub mod chunked_reader;
mod handle;
pub mod layout;
pub mod pattern;
pub mod provider;
mod reader;

// Mock memory reader for testing (always available for unit and integration tests)
#[doc(hidden)]
pub mod mock;

pub use bytes::{ByteBuffer, decode_shift_jis, decode_shift_jis_to_string};
pub use chunked_reader::{ChunkedMemoryIterator, DEFAULT_CHUNK_SIZE, MemoryChunk};
pub use handle::*;
pub use provider::{ProcessInfo, ProcessProvider};
pub use reader::{MemoryReader, ReadMemory};

// Re-export mock for convenient access in tests
#[doc(hidden)]
pub use mock::{MockMemoryBuilder, MockMemoryReader};
