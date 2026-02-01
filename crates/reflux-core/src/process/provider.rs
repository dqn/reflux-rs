//! Process provider abstraction for testability.
//!
//! This module provides traits that abstract process discovery and access,
//! enabling mock implementations for testing without a running game process.

use crate::error::Result;

/// Trait for accessing process information.
///
/// This trait abstracts the properties of a process handle, allowing
/// mock implementations for testing.
pub trait ProcessInfo {
    /// Get the process ID.
    fn pid(&self) -> u32;

    /// Get the base address of the main module.
    fn base_address(&self) -> u64;

    /// Get the size of the main module.
    fn module_size(&self) -> u32;

    /// Check if the process is still running.
    fn is_alive(&self) -> bool;
}

/// Trait for finding and opening processes.
///
/// This trait abstracts process discovery, allowing mock implementations
/// that don't require actual system processes.
pub trait ProcessProvider {
    /// The type of process info returned by this provider.
    type Process: ProcessInfo;

    /// Find and open the target game process.
    fn find_process(&self) -> Result<Self::Process>;

    /// Open a process by its PID.
    fn open_process(&self, pid: u32) -> Result<Self::Process>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    /// Mock process info for testing.
    pub struct MockProcessInfo {
        pub pid: u32,
        pub base_address: u64,
        pub module_size: u32,
        pub alive: bool,
    }

    impl ProcessInfo for MockProcessInfo {
        fn pid(&self) -> u32 {
            self.pid
        }

        fn base_address(&self) -> u64 {
            self.base_address
        }

        fn module_size(&self) -> u32 {
            self.module_size
        }

        fn is_alive(&self) -> bool {
            self.alive
        }
    }

    /// Mock process provider for testing.
    pub struct MockProcessProvider {
        pub process: Option<MockProcessInfo>,
    }

    impl ProcessProvider for MockProcessProvider {
        type Process = MockProcessInfo;

        fn find_process(&self) -> Result<Self::Process> {
            self.process
                .as_ref()
                .map(|p| MockProcessInfo {
                    pid: p.pid,
                    base_address: p.base_address,
                    module_size: p.module_size,
                    alive: p.alive,
                })
                .ok_or_else(|| Error::ProcessNotFound("Mock process not configured".to_string()))
        }

        fn open_process(&self, pid: u32) -> Result<Self::Process> {
            self.process
                .as_ref()
                .filter(|p| p.pid == pid)
                .map(|p| MockProcessInfo {
                    pid: p.pid,
                    base_address: p.base_address,
                    module_size: p.module_size,
                    alive: p.alive,
                })
                .ok_or_else(|| Error::ProcessNotFound(format!("Mock process {} not found", pid)))
        }
    }

    #[test]
    fn test_mock_process_info() {
        let info = MockProcessInfo {
            pid: 1234,
            base_address: 0x140000000,
            module_size: 0x1000000,
            alive: true,
        };

        assert_eq!(info.pid(), 1234);
        assert_eq!(info.base_address(), 0x140000000);
        assert_eq!(info.module_size(), 0x1000000);
        assert!(info.is_alive());
    }

    #[test]
    fn test_mock_provider_find_process() {
        let provider = MockProcessProvider {
            process: Some(MockProcessInfo {
                pid: 1234,
                base_address: 0x140000000,
                module_size: 0x1000000,
                alive: true,
            }),
        };

        let process = provider.find_process().unwrap();
        assert_eq!(process.pid(), 1234);
    }

    #[test]
    fn test_mock_provider_not_found() {
        let provider = MockProcessProvider { process: None };

        let result = provider.find_process();
        assert!(result.is_err());
    }

    #[test]
    fn test_mock_provider_open_process() {
        let provider = MockProcessProvider {
            process: Some(MockProcessInfo {
                pid: 1234,
                base_address: 0x140000000,
                module_size: 0x1000000,
                alive: true,
            }),
        };

        let process = provider.open_process(1234).unwrap();
        assert_eq!(process.pid(), 1234);

        let result = provider.open_process(9999);
        assert!(result.is_err());
    }
}
