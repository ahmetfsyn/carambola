use std::{
    ffi::{OsStr, OsString},
    fs::{self, OpenOptions},
    io::Write,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process::Command,
    ptr,
};

use windows::{
    core::{w, PCWSTR},
    Win32::{
        Foundation::{CloseHandle, GetLastError, ERROR_INVALID_PARAMETER, HANDLE, WAIT_OBJECT_0},
        Networking::WinHttp::{
            WinHttpAddRequestHeaders, WinHttpCloseHandle, WinHttpConnect, WinHttpOpen,
            WinHttpOpenRequest, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData,
            WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE,
            WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
        },
        System::Threading::{
            GetCurrentProcessId, OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
        },
        UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_TASKMODAL, MB_YESNO,
        },
    },
};

use crate::core::updater::{
    compatible_update, expected_installer_checksum, parse_releases_json, verify_installer,
    UpdateOffer,
};

const API_HOST: &str = "api.github.com";
const RELEASE_HOST: &str = "github.com";
const RELEASES_PATH: &str = "/repos/ahmetfsyn/carambola/releases?per_page=100";
const MAX_RELEASES_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_CHECKSUMS_BYTES: usize = 64 * 1024;
const MAX_INSTALLER_BYTES: usize = 250 * 1024 * 1024;
const HANDOFF_SWITCH: &str = "--carambola-update-handoff";

pub struct DownloadedInstaller {
    pub path: PathBuf,
    pub sha256: String,
}

struct InternetHandle(*mut std::ffi::c_void);

impl InternetHandle {
    fn new(handle: *mut std::ffi::c_void, operation: &str) -> Result<Self, String> {
        if handle.is_null() {
            Err(format!(
                "Could not {operation} (Windows error {})",
                unsafe { GetLastError().0 }
            ))
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for InternetHandle {
    fn drop(&mut self) {
        let _ = unsafe { WinHttpCloseHandle(self.0) };
    }
}

struct ProcessHandle(HANDLE);

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

pub fn is_installed_copy() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|path| {
            path.parent()
                .map(|directory| directory.join("unins000.exe"))
        })
        .is_some_and(|uninstaller| uninstaller.is_file())
}

pub fn check_for_updates(current_version: &str) -> Result<Option<UpdateOffer>, String> {
    log::info!("Checking GitHub for a compatible stable Carambola update");
    let response = https_get(API_HOST, RELEASES_PATH, MAX_RELEASES_JSON_BYTES)?;
    let releases = parse_releases_json(&response)?;
    let offer = compatible_update(current_version, &releases)?;
    if let Some(offer) = &offer {
        log::info!("Found compatible Carambola update {}", offer.version);
    } else {
        log::info!("No newer compatible stable Carambola release was found");
    }
    Ok(offer)
}

pub fn download_and_verify(offer: &UpdateOffer) -> Result<DownloadedInstaller, String> {
    log::info!("Downloading verified Carambola update {}", offer.version);
    let (checksum_host, checksum_path) = validate_release_url(&offer.checksums_url)?;
    let manifest = https_get(checksum_host, checksum_path, MAX_CHECKSUMS_BYTES)?;
    let expected_checksum = expected_installer_checksum(&manifest, &offer.installer_name)?;

    let (installer_host, installer_path) = validate_release_url(&offer.installer_url)?;
    let bytes = https_get(installer_host, installer_path, MAX_INSTALLER_BYTES)?;
    verify_installer(&bytes, &expected_checksum)?;

    let directory = create_private_update_directory()?;
    let path = directory.join(&offer.installer_name);
    let write_result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| format!("Could not create the downloaded installer: {error}"))?;
        file.write_all(&bytes)
            .map_err(|error| format!("Could not save the verified installer: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Could not finish writing the verified installer: {error}"))?;
        Ok::<(), String>(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&directory);
        return Err(error);
    }

    log::info!("Downloaded update installer passed SHA-256 verification");
    Ok(DownloadedInstaller {
        path,
        sha256: expected_checksum,
    })
}

pub fn confirm_update(version: &str) -> bool {
    let message = wide(&format!(
        "Carambola {version} is available. Download and install this update now? Carambola will close before the normal Setup program opens."
    ));
    unsafe {
        MessageBoxW(
            None,
            PCWSTR::from_raw(message.as_ptr()),
            w!("Carambola update"),
            MB_YESNO | MB_ICONINFORMATION | MB_TASKMODAL,
        ) == IDYES
    }
}

pub fn start_handoff(installer: &DownloadedInstaller) -> Result<(), String> {
    validate_downloaded_installer(&installer.path, &installer.sha256)?;
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not locate Carambola for the update handoff: {error}"))?;
    let process_id = unsafe { GetCurrentProcessId() };
    Command::new(executable)
        .arg(HANDOFF_SWITCH)
        .arg(&installer.path)
        .arg(process_id.to_string())
        .arg(&installer.sha256)
        .spawn()
        .map_err(|error| format!("Could not start the safe update handoff: {error}"))?;
    log::info!("Started update handoff; Carambola will close before Setup runs");
    Ok(())
}

pub fn run_handoff_if_requested() -> Option<Result<(), String>> {
    let arguments: Vec<OsString> = std::env::args_os().collect();
    if arguments
        .get(1)
        .is_none_or(|argument| argument != OsStr::new(HANDOFF_SWITCH))
    {
        return None;
    }
    let result = run_handoff(&arguments);
    if let Err(error) = &result {
        show_error(error);
    }
    Some(result)
}

fn run_handoff(arguments: &[OsString]) -> Result<(), String> {
    if arguments.len() != 5 {
        return Err("The update handoff arguments are incomplete".into());
    }
    let installer = PathBuf::from(&arguments[2]);
    let parent_id = arguments[3]
        .to_str()
        .and_then(|text| text.parse::<u32>().ok())
        .ok_or_else(|| "The update handoff has an invalid parent process ID".to_string())?;
    let checksum = arguments[4]
        .to_str()
        .ok_or_else(|| "The update handoff checksum is malformed".to_string())?;

    validate_downloaded_installer(&installer, checksum)?;
    wait_for_parent_exit(parent_id)?;
    validate_downloaded_installer(&installer, checksum)?;
    log::info!("Starting the verified update installer after Carambola exited");
    let result = Command::new(&installer)
        .current_dir(installer.parent().unwrap_or_else(|| Path::new(".")))
        .status()
        .map_err(|error| format!("Could not start the verified installer: {error}"))?;
    if !result.success() {
        let message = format!("Carambola Setup exited with status {result}");
        return Err(message);
    }
    Ok(())
}

fn wait_for_parent_exit(process_id: u32) -> Result<(), String> {
    let process = match unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, process_id) } {
        Ok(process) => ProcessHandle(process),
        Err(error) if error.code() == ERROR_INVALID_PARAMETER.into() => return Ok(()),
        Err(error) => return Err(format!("Could not wait for Carambola to exit: {error}")),
    };
    let result = unsafe { WaitForSingleObject(process.0, INFINITE) };
    if result == WAIT_OBJECT_0 {
        Ok(())
    } else {
        Err(format!(
            "Waiting for Carambola to exit failed (Windows result {})",
            result.0
        ))
    }
}

fn validate_downloaded_installer(path: &Path, checksum: &str) -> Result<(), String> {
    let path = path
        .canonicalize()
        .map_err(|error| format!("The downloaded installer is no longer available: {error}"))?;
    let temp_root = std::env::temp_dir()
        .canonicalize()
        .map_err(|error| format!("Could not verify the temporary update directory: {error}"))?;
    let directory = path
        .parent()
        .ok_or_else(|| "The downloaded installer has no parent directory".to_string())?;
    if !directory.starts_with(&temp_root)
        || !directory
            .file_name()
            .and_then(OsStr::to_str)
            .is_some_and(|name| name.starts_with("carambola-update-"))
        || !is_setup_filename(path.file_name().and_then(OsStr::to_str).unwrap_or_default())
    {
        return Err(
            "The update handoff rejected an installer outside its private temporary folder".into(),
        );
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("Could not recheck the downloaded installer: {error}"))?;
    verify_installer(&bytes, checksum)
}

fn is_setup_filename(name: &str) -> bool {
    let Some(version) = name
        .strip_prefix("Carambola-Setup-")
        .and_then(|value| value.strip_suffix(".exe"))
    else {
        return false;
    };
    let mut parts = version.split('.');
    let Some(major) = parts.next() else {
        return false;
    };
    let Some(minor) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && [major, minor].iter().all(|part| {
            !part.is_empty()
                && (part.len() == 1 || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn create_private_update_directory() -> Result<PathBuf, String> {
    let temp = std::env::temp_dir();
    for attempt in 0..10u8 {
        let nonce = format!(
            "{}-{}-{attempt}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|error| format!("System clock is invalid: {error}"))?
                .as_nanos()
        );
        let directory = temp.join(format!("carambola-update-{nonce}"));
        match fs::create_dir(&directory) {
            Ok(()) => {
                return directory.canonicalize().map_err(|error| {
                    format!("Could not verify the private update directory: {error}")
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "Could not create a private update directory: {error}"
                ))
            }
        }
    }
    Err("Could not create a unique private update directory".into())
}

fn validate_release_url(url: &str) -> Result<(&'static str, &str), String> {
    let Some(path) = url.strip_prefix("https://github.com/ahmetfsyn/carambola/releases/download/")
    else {
        return Err("The update release contains an unexpected download address".into());
    };
    if path.is_empty() || path.contains("..") || path.contains(['?', '#', '\\']) {
        return Err("The update release contains an invalid download path".into());
    }
    Ok((
        RELEASE_HOST,
        url.strip_prefix("https://github.com").unwrap(),
    ))
}

fn https_get(host: &str, path: &str, limit: usize) -> Result<Vec<u8>, String> {
    if !matches!(host, API_HOST | RELEASE_HOST) || !path.starts_with('/') {
        return Err("The updater refused an unexpected HTTPS host or path".into());
    }
    let host_wide = wide(host);
    let path_wide = wide(path);
    let headers: Vec<u16> = format!(
        "User-Agent: Carambola/{}\r\nAccept: application/vnd.github+json\r\n",
        env!("CARGO_PKG_VERSION")
    )
    .encode_utf16()
    .collect();

    let session = InternetHandle::new(
        unsafe {
            WinHttpOpen(
                w!("Carambola updater"),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                PCWSTR::null(),
                PCWSTR::null(),
                0,
            )
        },
        "initialize secure HTTP",
    )?;
    unsafe { WinHttpSetTimeouts(session.0, 10_000, 15_000, 15_000, 30_000) }
        .map_err(|error| format!("Could not configure updater network timeouts: {error}"))?;
    let connection = InternetHandle::new(
        unsafe { WinHttpConnect(session.0, PCWSTR::from_raw(host_wide.as_ptr()), 443, 0) },
        "connect to GitHub over HTTPS",
    )?;
    let request = InternetHandle::new(
        unsafe {
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                PCWSTR::from_raw(path_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                ptr::null(),
                WINHTTP_FLAG_SECURE,
            )
        },
        "create secure GitHub request",
    )?;
    unsafe { WinHttpAddRequestHeaders(request.0, &headers, WINHTTP_ADDREQ_FLAG_ADD) }
        .map_err(|error| format!("Could not set updater request headers: {error}"))?;
    unsafe { WinHttpSendRequest(request.0, None, None, 0, 0, 0) }
        .map_err(|error| format!("Could not send the GitHub request: {error}"))?;
    unsafe { WinHttpReceiveResponse(request.0, ptr::null_mut()) }
        .map_err(|error| format!("GitHub did not return an updater response: {error}"))?;

    let mut status_code = 0u32;
    let mut status_bytes = std::mem::size_of::<u32>() as u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status_code as *mut u32).cast()),
            &mut status_bytes,
            ptr::null_mut(),
        )
    }
    .map_err(|error| format!("Could not read the GitHub response status: {error}"))?;
    if status_code != 200 {
        return Err(format!(
            "GitHub returned HTTP {status_code} while checking for updates"
        ));
    }

    let mut body = Vec::new();
    loop {
        let mut available = 0u32;
        unsafe { WinHttpQueryDataAvailable(request.0, &mut available) }
            .map_err(|error| format!("Could not read the GitHub response: {error}"))?;
        if available == 0 {
            break;
        }
        if body.len().saturating_add(available as usize) > limit {
            return Err("The GitHub updater response exceeded its size limit".into());
        }
        let old_length = body.len();
        body.resize(old_length + available as usize, 0);
        let mut bytes_read = 0u32;
        if let Err(error) = unsafe {
            WinHttpReadData(
                request.0,
                body[old_length..].as_mut_ptr().cast(),
                available,
                &mut bytes_read,
            )
        } {
            body.truncate(old_length);
            return Err(format!("Could not download the GitHub response: {error}"));
        }
        body.truncate(old_length + bytes_read as usize);
        if bytes_read == 0 {
            break;
        }
    }
    Ok(body)
}

fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

fn show_error(message: &str) {
    let message = wide(message);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR::from_raw(message.as_ptr()),
            w!("Carambola update"),
            MB_ICONERROR | MB_TASKMODAL,
        );
    }
}
