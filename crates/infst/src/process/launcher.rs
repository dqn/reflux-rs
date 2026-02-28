//! Game launcher — registry lookup, token extraction, and direct game launch.

/// Extract the authentication token from an INFINITAS URI.
///
/// The URI contains a `tk=` parameter followed by a 64-character hex token.
/// Matches the pattern from inf_launch_ext: `$Args[0] -match "tk=(.{64})"`.
pub fn extract_token_from_uri(uri: &str) -> anyhow::Result<String> {
    let marker = "tk=";
    let pos = uri
        .find(marker)
        .ok_or_else(|| anyhow::anyhow!("URI does not contain 'tk=' parameter"))?;

    let start = pos + marker.len();
    let remaining = &uri[start..];

    if remaining.len() < 64 {
        anyhow::bail!(
            "Token too short: expected 64 characters, found {}",
            remaining.len()
        );
    }

    // Take exactly 64 characters (stop at & if present, but token should be 64 chars)
    let token: String = remaining.chars().take(64).collect();
    if token.len() < 64 {
        anyhow::bail!(
            "Token too short: expected 64 characters, found {}",
            token.len()
        );
    }

    Ok(token)
}

/// Find the game executable path from the Windows registry.
///
/// Reads `HKLM\SOFTWARE\KONAMI\beatmania IIDX INFINITAS\InstallDir`
/// and returns `{InstallDir}\game\app\bm2dx.exe`.
#[cfg(target_os = "windows")]
pub fn find_game_executable() -> anyhow::Result<std::path::PathBuf> {
    use windows::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW};
    use windows::core::HSTRING;

    let subkey = HSTRING::from(r"SOFTWARE\KONAMI\beatmania IIDX INFINITAS");
    let value_name = HSTRING::from("InstallDir");

    // First call to get the required buffer size
    let mut size: u32 = 0;
    // SAFETY: RegGetValueW with null buffer queries the required size.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value_name,
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
        .ok()
        .map_err(|e| anyhow::anyhow!("Failed to query registry value size: {e}"))?;
    }

    // Allocate buffer and read the value
    let mut buffer = vec![0u16; (size as usize) / 2];
    // SAFETY: RegGetValueW reads the registry value into the provided buffer.
    unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            &subkey,
            &value_name,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&mut size),
        )
        .ok()
        .map_err(|e| anyhow::anyhow!("Failed to read registry value: {e}"))?;
    }

    // Trim null terminator
    if buffer.last() == Some(&0) {
        buffer.pop();
    }

    let install_dir = String::from_utf16(&buffer)
        .map_err(|e| anyhow::anyhow!("Invalid UTF-16 in registry value: {e}"))?;

    let exe_path = std::path::PathBuf::from(install_dir)
        .join("game")
        .join("app")
        .join("bm2dx.exe");

    if !exe_path.exists() {
        anyhow::bail!("Game executable not found at: {}", exe_path.display());
    }

    Ok(exe_path)
}

#[cfg(not(target_os = "windows"))]
pub fn find_game_executable() -> anyhow::Result<std::path::PathBuf> {
    anyhow::bail!("Game executable lookup is only supported on Windows")
}

/// Launch the game directly with the given token in windowed mode.
///
/// Runs `bm2dx.exe -t TOKEN -w` and returns the child process ID.
#[cfg(target_os = "windows")]
pub fn launch_game(token: &str) -> anyhow::Result<u32> {
    let exe = find_game_executable()?;
    let child = std::process::Command::new(&exe)
        .args(["-t", token, "-w"])
        .spawn()
        .map_err(|e| anyhow::anyhow!("Failed to launch game: {e}"))?;

    Ok(child.id())
}

#[cfg(not(target_os = "windows"))]
pub fn launch_game(_token: &str) -> anyhow::Result<u32> {
    anyhow::bail!("Game launching is only supported on Windows")
}

/// Register the `bm2dxinf://` URI scheme handler in the Windows registry.
///
/// Writes to `HKCU\Software\Classes\bm2dxinf` so that the OS launches
/// `infst.exe "<URI>"` when a `bm2dxinf://` link is activated.
#[cfg(target_os = "windows")]
pub fn register_uri_scheme() -> anyhow::Result<()> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_SZ, RegCreateKeyExW, RegSetValueExW,
    };
    use windows::core::HSTRING;

    let exe_path = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("Failed to get current executable path: {e}"))?;

    let command_value = format!("\"{}\" \"%1\"", exe_path.display());

    // Helper: create/open a registry key under HKCU
    fn create_key(parent: HKEY, subkey: &str) -> anyhow::Result<HKEY> {
        let hkey_subkey = HSTRING::from(subkey);
        let mut key = HKEY::default();
        // SAFETY: RegCreateKeyExW creates or opens a registry key.
        unsafe {
            RegCreateKeyExW(
                parent,
                &hkey_subkey,
                0,
                None,
                windows::Win32::System::Registry::REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut key,
                None,
            )
            .ok()
            .map_err(|e| anyhow::anyhow!("Failed to create registry key '{subkey}': {e}"))?;
        }
        Ok(key)
    }

    // Helper: set a string value on an open key
    fn set_string_value(key: HKEY, name: Option<&str>, value: &str) -> anyhow::Result<()> {
        use windows::core::PCWSTR;

        let wide: Vec<u16> = OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // Build the value name as a null-terminated wide string, or use PCWSTR::null()
        // for the default value (which writes to the unnamed "" value).
        let name_wide: Vec<u16>;
        let pcwstr_name = if let Some(n) = name {
            name_wide = OsStr::new(n)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            PCWSTR::from_raw(name_wide.as_ptr())
        } else {
            PCWSTR::null()
        };
        // SAFETY: RegSetValueExW writes a REG_SZ value. The PCWSTR pointers remain valid
        // for the duration of the call because `name_wide` and `wide` are alive.
        unsafe {
            RegSetValueExW(
                key,
                pcwstr_name,
                0,
                REG_SZ,
                Some(std::slice::from_raw_parts(
                    wide.as_ptr().cast::<u8>(),
                    wide.len() * 2,
                )),
            )
            .ok()
            .map_err(|e| anyhow::anyhow!("Failed to set registry value: {e}"))?;
        }
        Ok(())
    }

    // Helper: close a registry key
    fn close_key(key: HKEY) {
        // SAFETY: RegCloseKey closes an open registry key handle.
        unsafe {
            let _ = windows::Win32::System::Registry::RegCloseKey(key);
        }
    }

    // HKCU\Software\Classes\bm2dxinf
    let root_key = create_key(HKEY_CURRENT_USER, r"Software\Classes\bm2dxinf")?;
    set_string_value(root_key, None, "URL:bm2dxinf Protocol")?;
    set_string_value(root_key, Some("URL Protocol"), "")?;

    // HKCU\Software\Classes\bm2dxinf\shell\open\command
    let cmd_key = create_key(
        HKEY_CURRENT_USER,
        r"Software\Classes\bm2dxinf\shell\open\command",
    )?;
    set_string_value(cmd_key, None, &command_value)?;

    close_key(cmd_key);
    close_key(root_key);

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn register_uri_scheme() -> anyhow::Result<()> {
    anyhow::bail!("URI scheme registration is only supported on Windows")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_token_valid_uri() {
        let uri = "bm2dxinf://something?tk=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA&other=param";
        let token = extract_token_from_uri(uri).unwrap();
        assert_eq!(token.len(), 64);
        assert_eq!(token, "A".repeat(64));
    }

    #[test]
    fn extract_token_at_end_of_uri() {
        let uri = format!("bm2dxinf://launch?tk={}", "B".repeat(64));
        let token = extract_token_from_uri(&uri).unwrap();
        assert_eq!(token, "B".repeat(64));
    }

    #[test]
    fn extract_token_missing_tk() {
        let uri = "bm2dxinf://something?foo=bar";
        let result = extract_token_from_uri(uri);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("tk="));
    }

    #[test]
    fn extract_token_too_short() {
        let uri = "bm2dxinf://something?tk=tooshort";
        let result = extract_token_from_uri(uri);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }

    #[test]
    fn extract_token_exactly_64_chars() {
        let token_str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(token_str.len(), 64);
        let uri = format!("bm2dxinf://x?tk={token_str}");
        let token = extract_token_from_uri(&uri).unwrap();
        assert_eq!(token, token_str);
    }
}
