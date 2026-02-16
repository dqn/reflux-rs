//! Common CLI utility functions shared across commands.

use anyhow::Result;
use infst::{MemoryReader, OffsetSearcher, OffsetsCollection, ProcessHandle, builtin_signatures};

/// Open a game process by PID or auto-detect.
pub fn open_process(pid: Option<u32>) -> Result<ProcessHandle> {
    if let Some(pid) = pid {
        Ok(ProcessHandle::open(pid)?)
    } else {
        Ok(ProcessHandle::find_and_open()?)
    }
}

/// Search for all memory offsets using builtin signatures.
pub fn search_offsets(reader: &MemoryReader) -> Result<OffsetsCollection> {
    let signatures = builtin_signatures();
    let mut searcher = OffsetSearcher::new(reader);
    Ok(searcher.search_all_with_signatures(&signatures)?)
}
