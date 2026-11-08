//! Screenshot toast notifications for unpackaged Windows desktop builds.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, OnceLock,
    },
};

use crate::core::{frame::CapturedFrame, notification_preview::TempPreviewFile};
use windows::{
    core::{IInspectable, Interface, GUID, HSTRING, PCWSTR, PWSTR},
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    Win32::{
        Foundation::PROPERTYKEY,
        System::{
            Com::{
                CoCreateInstance, CoTaskMemAlloc, IPersistFile,
                StructuredStorage::{PropVariantClear, PROPVARIANT},
                CLSCTX_INPROC_SERVER,
            },
            Variant::VT_LPWSTR,
            WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
        },
        UI::Shell::PropertiesSystem::IPropertyStore,
        UI::Shell::{IShellLinkW, ShellLink},
    },
    UI::Notifications::{
        NotificationSetting, ToastDismissedEventArgs, ToastFailedEventArgs, ToastNotification,
        ToastNotificationManager,
    },
};

const APP_USER_MODEL_ID: &str = "Carambola.Native.Screenshot";
static NEXT_NOTIFICATION_ID: AtomicU64 = AtomicU64::new(0);
static PENDING_NOTIFICATIONS: OnceLock<Mutex<HashMap<u64, PendingNotification>>> = OnceLock::new();

struct PendingNotification {
    // Keep the WinRT event source alive so Dismissed/Failed can release the
    // image only after Explorer has finished using the toast.
    _toast: ToastNotification,
    _preview: TempPreviewFile,
}

const APP_USER_MODEL_ID_KEY: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
    pid: 5,
};

struct ComApartment;

impl ComApartment {
    fn initialize() -> Result<Self, String> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|error| {
            format!("Could not initialize WinRT for the Windows notification: {error}")
        })?;
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

pub fn show_screenshot_notification(
    message: &str,
    _owner_hwnd: isize,
    screenshot: &CapturedFrame,
) -> Result<(), String> {
    super::trace_event("toast.begin");
    let result = show_screenshot_notification_inner(message, screenshot);
    match &result {
        Ok(()) => super::trace_event("toast.submitted"),
        Err(error) => super::trace_event(&format!("toast.failed error={error:?}")),
    }
    result
}

fn show_screenshot_notification_inner(
    message: &str,
    screenshot: &CapturedFrame,
) -> Result<(), String> {
    let _com = ComApartment::initialize()
        .map_err(|error| format!("Toast COM initialization failed: {error}"))?;
    ensure_start_menu_shortcut()
        .map_err(|error| format!("Toast shortcut registration failed: {error}"))?;
    super::trace_event("toast.registration.complete");

    let preview = TempPreviewFile::create(screenshot)
        .map_err(|error| format!("Toast preview creation failed: {error}"))?;
    log::info!("Toast preview created at {}", preview.path().display());
    super::trace_event(&format!("toast.preview.created path={:?}", preview.path()));
    let image_uri = crate::core::notification_preview::local_file_uri(preview.path())
        .map_err(|error| format!("Toast preview URI failed: {error}"))?;
    let xml = format!(
        "<toast duration=\"short\"><visual><binding template=\"ToastGeneric\"><image placement=\"hero\" src=\"{}\" alt=\"Screenshot preview\"/><text>{}</text></binding></visual></toast>",
        escape_xml_attribute(&image_uri),
        escape_xml_text(message),
    );

    let document =
        XmlDocument::new().map_err(|error| format!("Toast XML creation failed: {error}"))?;
    document
        .LoadXml(&HSTRING::from(xml))
        .map_err(|error| format!("Toast XML parsing failed: {error}"))?;
    let toast = ToastNotification::CreateToastNotification(&document)
        .map_err(|error| format!("Toast object creation failed: {error}"))?;
    super::trace_event("toast.xml.created");

    // Keep the preview file alive until Explorer reports that the toast was
    // dismissed or failed. This avoids racing Windows' lazy image loading.
    let notification_id = NEXT_NOTIFICATION_ID.fetch_add(1, Ordering::Relaxed);
    let app_id = HSTRING::from(APP_USER_MODEL_ID);
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&app_id)
        .map_err(|error| format!("Toast notifier creation failed: {error}"))?;
    let setting = notifier
        .Setting()
        .map_err(|error| format!("Could not read the Windows notification setting: {error}"))?;
    super::trace_event(&format!("toast.notification_setting value={setting:?}"));
    if setting != NotificationSetting::Enabled {
        return Err(format!("Windows notifications are disabled ({setting:?})"));
    }
    let dismissed =
        TypedEventHandler::<ToastNotification, ToastDismissedEventArgs>::new(move |_, _| {
            log::info!("Windows toast dismissed id={notification_id}");
            release_notification(notification_id);
            Ok(())
        });
    let failed =
        TypedEventHandler::<ToastNotification, ToastFailedEventArgs>::new(move |_, args| {
            let error_code = args.as_ref().and_then(|args| args.ErrorCode().ok());
            log::error!(
                "Windows toast failed after Show id={notification_id} hresult={error_code:?}"
            );
            super::trace_event(&format!(
                "toast.async_failed id={notification_id} hresult={error_code:?}"
            ));
            release_notification(notification_id);
            Ok(())
        });
    let activated = TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, _| {
        log::info!("Windows toast activated id={notification_id}");
        release_notification(notification_id);
        Ok(())
    });
    toast.Activated(&activated).map_err(format_windows_error)?;
    toast.Dismissed(&dismissed).map_err(format_windows_error)?;
    toast.Failed(&failed).map_err(format_windows_error)?;

    let pending = PENDING_NOTIFICATIONS.get_or_init(|| Mutex::new(HashMap::new()));
    pending
        .lock()
        .map_err(|_| "Screenshot notification cleanup state is unavailable".to_owned())?
        .insert(
            notification_id,
            PendingNotification {
                _toast: toast.clone(),
                _preview: preview,
            },
        );
    if let Err(error) = notifier.Show(&toast) {
        release_notification(notification_id);
        return Err(format!("Windows toast Show failed: {error}"));
    }
    log::info!("Windows toast submitted id={notification_id} app_id={APP_USER_MODEL_ID}");
    Ok(())
}

fn release_notification(notification_id: u64) {
    if let Some(pending) = PENDING_NOTIFICATIONS.get() {
        if let Ok(mut pending) = pending.lock() {
            pending.remove(&notification_id);
        }
    }
}

fn ensure_start_menu_shortcut() -> Result<(), String> {
    use std::{os::windows::ffi::OsStrExt, path::PathBuf};
    use windows::Win32::System::Com::STGM_READWRITE;

    let app_data = std::env::var_os("APPDATA")
        .ok_or_else(|| "Windows did not provide the current user's AppData directory".to_owned())?;
    let programs = PathBuf::from(app_data)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    std::fs::create_dir_all(&programs)
        .map_err(|error| format!("Could not create the Start Menu program folder: {error}"))?;
    // Keep the toast registration separate from an existing application
    // shortcut so assigning a notification AppUserModelID cannot change the
    // app's normal taskbar identity or grouping.
    let shortcut = programs.join("Carambola Screenshot Notifications.lnk");
    let shortcut_wide = shortcut
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    let link: IShellLinkW = unsafe {
        CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).map_err(format_windows_error)?
    };
    let persist: IPersistFile = link.cast().map_err(format_windows_error)?;
    if shortcut.exists() {
        unsafe { persist.Load(PCWSTR(shortcut_wide.as_ptr()), STGM_READWRITE) }
            .map_err(format_windows_error)?;
    }

    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not locate the Carambola executable: {error}"))?;
    let executable_wide = executable
        .as_os_str()
        .encode_wide()
        .chain([0])
        .collect::<Vec<_>>();
    unsafe { link.SetPath(PCWSTR(executable_wide.as_ptr())) }.map_err(format_windows_error)?;
    if let Some(directory) = executable.parent() {
        let directory_wide = directory
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        unsafe { link.SetWorkingDirectory(PCWSTR(directory_wide.as_ptr())) }
            .map_err(format_windows_error)?;
    }
    let description = HSTRING::from("Carambola screenshot notifications");
    unsafe { link.SetDescription(&description) }.map_err(format_windows_error)?;

    let store: IPropertyStore = link.cast().map_err(format_windows_error)?;
    let mut property = string_property(APP_USER_MODEL_ID)?;
    let set_result = unsafe { store.SetValue(&APP_USER_MODEL_ID_KEY, &property) };
    let clear_result = unsafe { PropVariantClear(&mut property) };
    set_result.map_err(format_windows_error)?;
    clear_result.map_err(format_windows_error)?;
    unsafe { store.Commit() }.map_err(format_windows_error)?;
    unsafe { persist.Save(PCWSTR(shortcut_wide.as_ptr()), true) }.map_err(format_windows_error)?;
    log::info!(
        "Toast shortcut registered app_id={APP_USER_MODEL_ID} shortcut={} target={}",
        shortcut.display(),
        executable.display()
    );
    Ok(())
}

fn string_property(value: &str) -> Result<PROPVARIANT, String> {
    let wide = value.encode_utf16().chain([0]).collect::<Vec<_>>();
    let bytes = wide.len() * std::mem::size_of::<u16>();
    let allocated = unsafe { CoTaskMemAlloc(bytes) }.cast::<u16>();
    if allocated.is_null() {
        return Err("Could not allocate the Windows notification AppUserModelID".into());
    }
    let mut property = PROPVARIANT::default();
    unsafe {
        std::ptr::copy_nonoverlapping(wide.as_ptr(), allocated, wide.len());
        (*property.Anonymous.Anonymous).vt = VT_LPWSTR;
        (*property.Anonymous.Anonymous).Anonymous.pwszVal = PWSTR(allocated);
    }
    Ok(property)
}

fn escape_xml_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_xml_attribute(value: &str) -> String {
    escape_xml_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn format_windows_error(error: windows::core::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::{escape_xml_attribute, escape_xml_text, string_property};
    use windows::Win32::System::{Com::StructuredStorage::PropVariantClear, Variant::VT_LPWSTR};

    #[test]
    fn app_id_property_is_a_utf16_string_with_owned_storage() {
        let mut property = string_property("Carambola.Native.Screenshot").unwrap();
        unsafe {
            assert_eq!((*property.Anonymous.Anonymous).vt, VT_LPWSTR);
            let pointer = (*property.Anonymous.Anonymous).Anonymous.pwszVal.0;
            let expected = "Carambola.Native.Screenshot"
                .encode_utf16()
                .collect::<Vec<_>>();
            assert_eq!(
                std::slice::from_raw_parts(pointer, expected.len()),
                expected
            );
            PropVariantClear(&mut property).unwrap();
        }
    }

    #[test]
    fn toast_content_is_xml_escaped() {
        assert_eq!(
            escape_xml_text("Copy & save <done>"),
            "Copy &amp; save &lt;done&gt;"
        );
        assert_eq!(
            escape_xml_attribute("file:///C:/A & B.png"),
            "file:///C:/A &amp; B.png"
        );
    }
}
