use crate::app_state::AppState;
use crate::window::activate_overlay;
use log::info;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter};

pub fn setup_tray(app_handle: &AppHandle, state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let capture_item = MenuItem::with_id(app_handle, "capture", "Capture Screen", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app_handle, "settings", "Change Shortcut...", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app_handle)?;
    let quit_item = MenuItem::with_id(app_handle, "quit", "Quit Carambola", true, None::<&str>)?;

    let menu = Menu::with_items(app_handle, &[&capture_item, &settings_item, &separator, &quit_item])?;

    let state_clone = state.clone();

    let _tray = TrayIconBuilder::new()
        .icon(app_handle.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "capture" => {
                info!("Tray menu 'Capture' clicked");
                let _ = activate_overlay(app, &state_clone);
            }
            "settings" => {
                info!("Tray menu 'Change Shortcut' clicked");
                let _ = activate_overlay(app, &state_clone);
                let _ = app.emit("open_shortcut_settings", ());
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
        })
        .build(app_handle)?;

    info!("System tray initialized successfully with shortcut settings menu");
    Ok(())
}
