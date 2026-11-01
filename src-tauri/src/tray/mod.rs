use crate::app_state::AppState;
use crate::window::activate_overlay;
use log::info;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter};

pub fn setup_tray(
    app_handle: &AppHandle,
    state: AppState,
) -> Result<(), Box<dyn std::error::Error>> {
    let capture_item =
        MenuItem::with_id(app_handle, "capture", "Capture Screen", true, None::<&str>)?;
    let settings_item =
        MenuItem::with_id(app_handle, "settings", "Settings...", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app_handle)?;
    let quit_item = MenuItem::with_id(app_handle, "quit", "Quit Carambola", true, None::<&str>)?;

    let menu = Menu::with_items(
        app_handle,
        &[&capture_item, &settings_item, &separator, &quit_item],
    )?;

    let state_clone = state.clone();

    let mut tray_builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "capture" => {
                info!("Tray menu 'Capture' clicked");
                let _ = activate_overlay(app, &state_clone);
            }
            "settings" => {
                info!("Tray menu 'Settings' clicked");
                if state_clone.is_overlay_visible() {
                    // A finished capture is already on screen; open Settings on
                    // it directly instead of starting a second session.
                    let _ = app.emit("open_shortcut_settings", ());
                } else {
                    // Open Settings only after the fresh capture is presented,
                    // so the per-session reset cannot clear it. If a session is
                    // already preparing, present_overlay will consume the
                    // request once that session is on screen.
                    state_clone.request_settings_on_present();
                    if !state_clone.is_session_active() {
                        let _ = activate_overlay(app, &state_clone);
                    }
                    if !state_clone.is_session_active() {
                        // Activation failed or was ignored: do not leave a
                        // pending Settings request behind.
                        let _ = state_clone.take_settings_on_present();
                    }
                }
            }
            "quit" => {
                info!("Quitting Carambola from tray");
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                let _ = activate_overlay(app, &state);
            }
        });

    // Use the bundled application icon when available. Building the tray
    // without an icon is not useful, but it must not panic on platforms or
    // bundles where the default window icon is unavailable.
    if let Some(icon) = app_handle.default_window_icon() {
        tray_builder = tray_builder.icon(icon.clone());
    } else {
        log::warn!("No default window icon available for the tray icon");
    }

    let _tray = tray_builder.build(app_handle)?;

    info!("System tray initialized successfully with shortcut settings menu");
    Ok(())
}
