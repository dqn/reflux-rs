#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

use crate::error::{Error, Result};

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
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

const PROCESS_NAME: &str = "bm2dx.exe";

#[cfg(target_os = "windows")]
pub struct ProcessHandle {
    handle: HANDLE,
    pub base_address: u64,
    pub module_size: u32,
}

#[cfg(not(target_os = "windows"))]
pub struct ProcessHandle {
    pub base_address: u64,
    pub module_size: u32,
}

#[cfg(target_os = "windows")]
impl ProcessHandle {
    pub fn find_and_open() -> Result<Self> {
        let pid = find_process_id(PROCESS_NAME)?;
        Self::open(pid)
    }

    pub fn open(pid: u32) -> Result<Self> {
        let handle = unsafe {
            OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid)
                .map_err(|e| Error::ProcessOpenFailed(e.to_string()))?
        };

        let (base_address, module_size) = get_module_info(handle)?;

        Ok(Self {
            handle,
            base_address,
            module_size,
        })
    }

    pub fn handle(&self) -> HANDLE {
        self.handle
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
}

#[cfg(target_os = "windows")]
impl Drop for ProcessHandle {
    fn drop(&mut self) {
        if !self.handle.is_invalid() {
            let _ = unsafe { CloseHandle(self.handle) };
        }
    }
}

#[cfg(target_os = "windows")]
fn find_process_id(name: &str) -> Result<u32> {
    let snapshot = unsafe {
        CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|e| Error::ProcessNotFound(e.to_string()))?
    };

    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };

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

    let _ = unsafe { CloseHandle(snapshot) };
    result
}

#[cfg(target_os = "windows")]
fn get_module_info(handle: HANDLE) -> Result<(u64, u32)> {
    let mut modules = [windows::Win32::Foundation::HMODULE::default(); 1024];
    let mut needed: u32 = 0;

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
