use crate::config;
use log::{info, warn};
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

#[derive(Clone, Debug, Serialize)]
pub struct AutostartStatus {
    pub supported: bool,
    pub enabled: bool,
    /// Human-readable explanation when the platform cannot manage startup
    /// entries or when querying/changing them failed.
    pub reason: Option<String>,
}

const SUPPORTED: bool = cfg!(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "linux"
));

pub fn status(app: &AppHandle) -> AutostartStatus {
    if !SUPPORTED {
        return AutostartStatus {
            supported: false,
            enabled: false,
            reason: Some(
                "Start on Boot is not supported on this platform. Use your desktop environment's startup settings."
                    .to_string(),
            ),
        };
    }

    match app.autolaunch().is_enabled() {
        Ok(enabled) => AutostartStatus {
            supported: true,
            enabled,
            reason: None,
        },
        Err(err) => AutostartStatus {
            supported: true,
            enabled: false,
            reason: Some(format!("Could not read the startup registration: {err}")),
        },
    }
}

fn apply(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if !SUPPORTED {
        return Err("Start on Boot is not supported on this platform".to_string());
    }
    let manager = app.autolaunch();
    if enabled {
        manager
            .enable()
            .map_err(|e| format!("Failed to enable Start on Boot: {e}"))
    } else {
        manager
            .disable()
            .map_err(|e| format!("Failed to disable Start on Boot: {e}"))
    }
}

/// Persists the user's choice and updates the OS startup registration.
pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<AutostartStatus, String> {
    apply(app, enabled)?;

    let mut cfg = config::load_or_create();
    cfg.autostart = Some(enabled);
    config::save(&cfg)?;

    info!("Start on Boot set to {enabled}");
    Ok(status(app))
}

/// Called once at application setup.
///
/// - Fresh install / legacy config (`autostart == None`): enable by default and
///   persist the choice, so a later explicit opt-out is respected forever.
/// - Existing choice: re-apply it to keep the OS registration in sync (repairs
///   entries lost during updates or manual cleanup), without silently flipping
///   a user's explicit "off" choice.
pub fn ensure_preference(app: &AppHandle) {
    if !SUPPORTED {
        warn!("Start on Boot is not supported on this platform");
        return;
    }

    let mut cfg = config::load_or_create();
    let desired = cfg.autostart.unwrap_or(true);

    match apply(app, desired) {
        Ok(()) => {
            if cfg.autostart.is_none() {
                cfg.autostart = Some(true);
                if let Err(e) = config::save(&cfg) {
                    warn!("Failed to persist default Start on Boot preference: {e}");
                }
            }
            info!("Start on Boot preference applied: {desired}");
        }
        Err(e) => warn!("Failed to apply Start on Boot preference ({desired}): {e}"),
    }
}
