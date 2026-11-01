pub mod app_state;
pub mod autostart;
pub mod capture;
pub mod clipboard;
pub mod commands;
pub mod config;
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
        // Must be registered before other plugins so a second launch exits
        // immediately instead of creating another tray/overlay instance.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            info!("Carambola is already running; ignoring duplicate launch");
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            // Start silent: the app is background/tray-first, so no overlay is
            // shown until the user presses the capture shortcut.
            Some(vec!["--background"]),
        ))
        .manage(app_state.clone())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        info!("Global shortcut triggered");
                        if let Err(err) = window::activate_overlay(app, &state_for_hotkey) {
                            log::warn!("Screenshot activation failed: {err}");
                        }
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
            commands::copy_crop_image,
            commands::save_crop_image,
            commands::hide_overlay,
            commands::present_overlay,
            commands::extract_ocr,
            commands::get_shortcut,
            commands::set_shortcut,
            commands::get_autostart,
            commands::set_autostart,
        ])
        .setup(move |app| {
            info!("Running application setup...");

            // Tray-only background application on macOS: no Dock icon and no
            // app menu, matching the Windows/Linux tray lifecycle.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Keep the OS startup registration in sync with the persisted
            // preference, enabling it by default on first run.
            autostart::ensure_preference(app.handle());

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
