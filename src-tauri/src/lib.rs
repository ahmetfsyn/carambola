pub mod app_state;
pub mod capture;
pub mod clipboard;
pub mod commands;
pub mod hotkey;
pub mod ocr;
pub mod protocol;
pub mod tray;
pub mod window;

use app_state::AppState;
use log::info;
use tauri_plugin_global_shortcut::ShortcutState;

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    info!("Starting Carambola application...");

    let app_state = AppState::new();
    let state_for_hotkey = app_state.clone();
    let state_for_setup = app_state.clone();

    tauri::Builder::default()
        .manage(app_state.clone())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        info!("Global shortcut triggered");
                        let _ = window::activate_overlay(app, &state_for_hotkey);
                    }
                })
                .build(),
        )
        .register_uri_scheme_protocol("carambola", |ctx, req| {
            protocol::handle_carambola_protocol(ctx.app_handle(), &req)
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_display_info,
            commands::get_current_frame,
            commands::copy_crop,
            commands::save_crop,
            commands::hide_overlay,
            commands::extract_ocr,
            commands::get_shortcut,
            commands::set_shortcut,
            commands::frontend_log,
        ])
        .setup(move |app| {
            info!("Running application setup...");

            // Initialize System Tray
            if let Err(e) = tray::setup_tray(app.handle(), state_for_setup.clone()) {
                log::error!("Failed to initialize tray: {e}");
            }

            // Register global hotkey
            if let Err(e) = hotkey::setup_hotkey(app.handle(), state_for_setup.clone()) {
                log::error!("Failed to register global hotkey: {e}");
            }

            // Prime background capture
            if let Ok(initial_frame) = capture::capture_screen() {
                state_for_setup.set_frame(initial_frame);
                info!("Initial screen frame primed in background");
            }

            // Size the hidden overlay to the target monitor up front so it is
            // never shown at the default window size.
            window::prime_overlay(app.handle(), &state_for_setup);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Error while running Carambola application");
}
