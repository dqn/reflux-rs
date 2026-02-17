//! Common CLI utility functions shared across commands.

use anyhow::Result;
use infst::ProcessHandle;

/// Open a game process by PID or auto-detect.
pub fn open_process(pid: Option<u32>) -> Result<ProcessHandle> {
    if let Some(pid) = pid {
        Ok(ProcessHandle::open(pid)?)
    } else {
        Ok(ProcessHandle::find_and_open()?)
    }
}
