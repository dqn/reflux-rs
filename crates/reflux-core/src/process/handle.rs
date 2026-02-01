#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use crate::error::{Error, Result};
use crate::process::provider::ProcessInfo;

#[cfg(target_os = "windows")]
use tracing::warn;

#[cfg(target_os = "windows")]
use std::ffi::OsString;
#[cfg(target_os = "windows")]
use std::os::windows::ffi::OsStringExt;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(target_os = "windows")]
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::ProcessStatus::{
    EnumProcessModulesEx, GetModuleInformation, LIST_MODULES_ALL, MODULEINFO,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ,
};

const PROCESS_NAME: &str = "bm2dx.exe";

#[cfg(target_os = "windows")]
pub struct ProcessHandle {
    handle: HANDLE,
    pub pid: u32,
    pub base_address: u64,
    pub module_size: u32,
}

#[cfg(not(target_os = "windows"))]
pub struct ProcessHandle {
    pub pid: u32,
    pub base_address: u64,
    pub module_size: u32,
}

#[cfg(target_os = "windows")]
impl ProcessHandle {
    pub fn find_and_open() -> Result<Self> {
        let pid = find_process_id(PROCESS_NAME).map_err(|e| {
            tracing::debug!("Process detection failed: {}", e);
            e
        })?;
        tracing::debug!("Found {} with PID {}", PROCESS_NAME, pid);
        Self::open(pid)
    }

    pub fn open(pid: u32) -> Result<Self> {
        // SAFETY: OpenProcess is called with valid flags (PROCESS_QUERY_INFORMATION | PROCESS_VM_READ)
        // and a process ID obtained from CreateToolhelp32Snapshot. The returned handle is managed
        // by this struct and closed in Drop.
        let handle = unsafe {
            OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).map_err(|e| {
                tracing::debug!("OpenProcess failed for PID {}: {}", pid, e);
                Error::ProcessOpenFailed(e.to_string())
            })?
        };

        let (base_address, module_size) = get_module_info(handle).map_err(|e| {
            tracing::debug!("get_module_info failed: {}", e);
            e
        })?;

        Ok(Self {
            handle,
            pid,
            base_address,
            module_size,
        })
    }

    pub fn handle(&self) -> HANDLE {
        self.handle
    }

    /// Check if the process is still running
    pub fn is_alive(&self) -> bool {
        const STILL_ACTIVE: u32 = 259;

        let mut exit_code: u32 = 0;
        // SAFETY: GetExitCodeProcess is called with a valid process handle obtained from OpenProcess.
        // The exit_code variable is properly initialized and passed by mutable reference.
        unsafe {
            if GetExitCodeProcess(self.handle, &mut exit_code).is_ok() {
                exit_code == STILL_ACTIVE
            } else {
                false
            }
        }
    }
}

#[cfg(target_os = "windows")]
impl ProcessInfo for ProcessHandle {
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
        ProcessHandle::is_alive(self)
    }
}

#[cfg(not(target_os = "windows"))]
impl ProcessHandle {
    pub fn find_and_open() -> Result<Self> {
        Err(Error::ProcessNotFound(
            "Windows only: process access not supported on this platform".to_string(),
        ))
    }

    pub fn open(_pid: u32) -> Result<Self> {
        Err(Error::ProcessNotFound(
            "Windows only: process access not supported on this platform".to_string(),
        ))
    }

    /// Check if the process is still running (stub for non-Windows)
    pub fn is_alive(&self) -> bool {
        false
    }
}

#[cfg(not(target_os = "windows"))]
impl ProcessInfo for ProcessHandle {
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
        ProcessHandle::is_alive(self)
    }
}

#[cfg(target_os = "windows")]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            // SAFETY: self.handle is a valid handle obtained from OpenProcess and has not been
            // closed yet. CloseHandle is safe to call on a valid handle.
            if let Err(e) = unsafe { CloseHandle(self.handle) } {
                warn!("Failed to close process handle: {}", e);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn find_process_id(name: &str) -> Result<u32> {
    // SAFETY: CreateToolhelp32Snapshot with TH32CS_SNAPPROCESS is safe to call.
    // The returned handle is closed at the end of this function.
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|e| Error::ProcessNotFound(e.to_string()))?
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

    // SAFETY: Process32FirstW and Process32NextW are safe to call with a valid snapshot handle
    // and properly initialized PROCESSENTRY32W structure.
    //
    // Note on null termination: The Windows API guarantees that szExeFile is always null-terminated.
    // The .unwrap_or(entry.szExeFile.len()) is a defensive fallback that can never be reached in
    // practice, but ensures safety if the invariant were ever violated.
    let result = unsafe {
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let exe_name = OsString::from_wide(
                    &entry.szExeFile[..entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len())],
                );

                if exe_name.to_string_lossy().eq_ignore_ascii_case(name) {
                    let _ = CloseHandle(snapshot);
                    return Ok(entry.th32ProcessID);
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        Err(Error::ProcessNotFound(format!(
            "Process '{}' not found",
            name
        )))
    };

    // SAFETY: snapshot is a valid handle from CreateToolhelp32Snapshot
    let _ = unsafe { CloseHandle(snapshot) };
    result
}

#[cfg(target_os = "windows")]
fn get_module_info(handle: HANDLE) -> Result<(u64, u32)> {
    let mut modules = [windows::Win32::Foundation::HMODULE::default(); 1024];
    let mut needed: u32 = 0;

    // SAFETY: EnumProcessModulesEx is called with a valid process handle from OpenProcess,
    // and the modules array is large enough to hold typical module counts. The needed
    // parameter receives the actual bytes required.
    unsafe {
        EnumProcessModulesEx(
            handle,
            modules.as_mut_ptr(),
            (modules.len() * std::mem::size_of::<windows::Win32::Foundation::HMODULE>()) as u32,
            &mut needed,
            LIST_MODULES_ALL,
        )
        .map_err(|e| Error::ProcessOpenFailed(format!("Failed to enumerate modules: {}", e)))?;
    }

    if needed == 0 {
        return Err(Error::ProcessOpenFailed(
            "No modules found in process".to_string(),
        ));
    }

    let mut info = MODULEINFO::default();
    // SAFETY: GetModuleInformation is called with a valid process handle and the first module
    // handle from the enumeration. The info struct is properly sized.
    unsafe {
        GetModuleInformation(
            handle,
            modules[0],
            &mut info,
            std::mem::size_of::<MODULEINFO>() as u32,
        )
        .map_err(|e| Error::ProcessOpenFailed(format!("Failed to get module info: {}", e)))?;
    }

    Ok((info.lpBaseOfDll as u64, info.SizeOfImage))
}
