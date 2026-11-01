use crate::app_state::AppState;
use crate::config;
use log::{info, warn};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

pub fn load_or_create_config() -> String {
    config::load_or_create().shortcut
}

pub fn setup_hotkey(
    app_handle: &AppHandle,
    _state: AppState,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        warn!(
            "Wayland session detected: the global shortcut backend uses X11. \
             It works through XWayland when available; otherwise bind the shortcut \
             in your desktop's keyboard settings or run an X11 session."
        );
    }

    let shortcut_str = load_or_create_config();
    info!("[hotkey] Attempting to register configured shortcut: {shortcut_str}");

    let shortcut: Shortcut = match shortcut_str.parse() {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to parse configured shortcut '{shortcut_str}': {e:?}. Falling back to default.");
            if cfg!(target_os = "macos") {
                "CommandOrControl+Shift+X".parse()?
            } else {
                "PrintScreen".parse()?
            }
        }
    };

    // Register shortcut
    match app_handle.global_shortcut().register(shortcut) {
        Ok(_) => {
            let registered = app_handle.global_shortcut().is_registered(shortcut);
            info!(
                "[hotkey] Registration succeeded for {:?}; active={registered}",
                shortcut
            );
            #[cfg(target_os = "windows")]
            if shortcut_str.eq_ignore_ascii_case("PrintScreen") {
                // Windows 11 routes PrintScreen to Snipping Tool with a
                // low-level hook by default. The hook wins over RegisterHotKey,
                // so the registration above can succeed while the key never
                // reaches the app. Surface it in the log instead of failing
                // silently.
                warn!(
                    "PrintScreen is the configured shortcut. If pressing it does nothing, \
                     disable 'Use the Print Screen key to open Snipping Tool' in Windows \
                     Settings > Accessibility > Keyboard, or choose another shortcut in \
                     Carambola Settings."
                );
            }
        }
        Err(err) => {
            warn!(
                "[hotkey] Failed to register configured shortcut {:?}: {err:?}. Trying fallback 'Alt+A'...",
                shortcut
            );
            let fallback = "Alt+A".parse::<Shortcut>()?;
            match app_handle.global_shortcut().register(fallback) {
                Ok(()) => {
                    info!(
                        "[hotkey] Fallback shortcut Alt+A registered; configured shortcut remains saved"
                    );
                    crate::window::report_hotkey_fallback(
                        app_handle,
                        &shortcut_str,
                        &err.to_string(),
                    );
                }
                Err(fallback_err) => {
                    return Err(format!(
                        "Configured shortcut {shortcut_str:?} failed ({err:?}); fallback Alt+A failed ({fallback_err:?})"
                    )
                    .into());
                }
            }
        }
    }

    Ok(())
}

pub fn update_global_shortcut(
    app_handle: &AppHandle,
    new_shortcut_str: &str,
) -> Result<(), String> {
    let shortcut: Shortcut = new_shortcut_str
        .parse()
        .map_err(|e| format!("Invalid shortcut combination '{new_shortcut_str}': {e:?}"))?;

    let previous_str = config::load_or_create().shortcut;
    let previous: Option<Shortcut> = previous_str.parse().ok();

    // Register the new shortcut first. If another application owns it, the
    // registration fails and the currently working shortcut stays bound, so the
    // user never loses capture access while editing Settings.
    info!("[hotkey] Attempting to rebind shortcut to {new_shortcut_str}");
    app_handle
        .global_shortcut()
        .register(shortcut)
        .map_err(|e| {
            warn!("[hotkey] Failed to register {new_shortcut_str}: {e:?}");
            format!("Could not register shortcut '{new_shortcut_str}': {e:?}")
        })?;

    // Only drop the old binding once the new one is confirmed. Registering the
    // same combination twice is a no-op for the plugin, so skip it explicitly.
    if let Some(old) = previous {
        if old != shortcut {
            let _ = app_handle.global_shortcut().unregister(old);
        }
    }

    // Persist to disk while preserving other preferences (e.g. autostart).
    let mut cfg = config::load_or_create();
    cfg.shortcut = new_shortcut_str.to_string();
    if let Err(err) = config::save(&cfg) {
        // Keep the running app consistent with the persisted config.
        let _ = app_handle.global_shortcut().unregister(shortcut);
        if let Ok(old) = previous_str.parse::<Shortcut>() {
            let _ = app_handle.global_shortcut().register(old);
        }
        return Err(err);
    }

    info!("[hotkey] Successfully re-bound global shortcut to: {new_shortcut_str}");

    Ok(())
}

#[cfg(test)]
mod tests {
    use tauri_plugin_global_shortcut::Shortcut;

    #[test]
    fn native_parser_preserves_command_control_and_combined_modifiers() {
        let command_a: Shortcut = "Command+KeyA".parse().unwrap();
        let control_a: Shortcut = "Control+KeyA".parse().unwrap();
        let command_control_a: Shortcut = "Command+Control+KeyA".parse().unwrap();
        assert_ne!(command_a, control_a);
        assert_ne!(command_a, command_control_a);
        assert_ne!(control_a, command_control_a);
    }

    #[test]
    fn modifier_order_is_canonical_in_hotkey_identity_and_physical_punctuation_parses() {
        let first: Shortcut = "Control+Alt+Shift+KeyA".parse().unwrap();
        let reordered: Shortcut = "Shift+Alt+Control+KeyA".parse().unwrap();
        assert_eq!(first, reordered);

        assert!("Command+Quote".parse::<Shortcut>().is_ok());
        assert!("Command+Backquote".parse::<Shortcut>().is_ok());
    }
}
