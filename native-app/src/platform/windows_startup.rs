use std::{mem::size_of, os::windows::ffi::OsStrExt, path::Path};

use windows::{
    core::w,
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
            RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_NONE,
            REG_OPTION_NON_VOLATILE, REG_SZ,
        },
    },
};

use crate::core::settings::StartupRegistrar;

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        let status = unsafe { RegCloseKey(self.0) };
        if status != ERROR_SUCCESS {
            log::warn!(
                "Could not close the Carambola startup registry key (Windows error {})",
                status.0
            );
        }
    }
}

fn registry_error(operation: &str, status: windows::Win32::Foundation::WIN32_ERROR) -> String {
    format!(
        "Could not {operation} the per-user Windows startup registration (Windows error {})",
        status.0
    )
}

fn open_run_key(
    access: windows::Win32::System::Registry::REG_SAM_FLAGS,
) -> Result<Option<RegistryKey>, String> {
    let mut key = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            access,
            &mut key,
        )
    };
    if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(registry_error("open", status));
    }
    Ok(Some(RegistryKey(key)))
}

fn startup_command(executable: &Path) -> Vec<u16> {
    let mut command = Vec::new();
    command.push('"' as u16);
    command.extend(executable.as_os_str().encode_wide());
    command.push('"' as u16);
    command
}

fn read_registration() -> Result<Option<Vec<u16>>, String> {
    let Some(key) = open_run_key(KEY_QUERY_VALUE)? else {
        return Ok(None);
    };

    let mut value_type = REG_NONE;
    let mut byte_length = 0u32;
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            w!("Carambola"),
            None,
            Some(&mut value_type),
            None,
            Some(&mut byte_length),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(registry_error("read", status));
    }
    if value_type != REG_SZ || byte_length % 2 != 0 {
        return Err(
            "The Carambola startup registration has an invalid Windows registry value".into(),
        );
    }

    let mut bytes = vec![0u8; byte_length as usize];
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            w!("Carambola"),
            None,
            Some(&mut value_type),
            Some(bytes.as_mut_ptr()),
            Some(&mut byte_length),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    if status != ERROR_SUCCESS {
        return Err(registry_error("read", status));
    }
    bytes.truncate(byte_length as usize);
    if bytes.len() % 2 != 0 {
        return Err("The Carambola startup registration contains malformed UTF-16 data".into());
    }

    let mut value: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    while value.last() == Some(&0) {
        value.pop();
    }
    Ok(Some(value))
}

pub fn is_startup_enabled() -> Result<bool, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not locate the Carambola executable: {error}"))?;
    Ok(read_registration()?.is_some_and(|value| value == startup_command(&executable)))
}

pub fn set_startup_enabled(enabled: bool) -> Result<(), String> {
    if !enabled {
        let Some(key) = open_run_key(KEY_SET_VALUE)? else {
            return Ok(());
        };
        let status = unsafe { RegDeleteValueW(key.0, w!("Carambola")) };
        if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        return Err(registry_error("remove", status));
    }

    let mut key = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(registry_error("open", status));
    }
    let key = RegistryKey(key);

    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not locate the Carambola executable: {error}"))?;
    let mut value = startup_command(&executable);
    value.push(0);
    let bytes = unsafe {
        std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), value.len() * size_of::<u16>())
    };
    let status = unsafe { RegSetValueExW(key.0, w!("Carambola"), None, REG_SZ, Some(bytes)) };
    if status != ERROR_SUCCESS {
        return Err(registry_error("write", status));
    }
    Ok(())
}

pub struct WindowsStartupRegistrar;

impl StartupRegistrar for WindowsStartupRegistrar {
    fn is_enabled(&mut self) -> Result<bool, String> {
        is_startup_enabled()
    }

    fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
        set_startup_enabled(enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::startup_command;
    use std::path::Path;

    #[test]
    fn startup_command_quotes_executable_paths_with_spaces() {
        assert_eq!(
            startup_command(Path::new(r"C:\Users\Test User\Carambola.exe")),
            r#""C:\Users\Test User\Carambola.exe""#.encode_utf16().collect::<Vec<_>>()
        );
    }
}
