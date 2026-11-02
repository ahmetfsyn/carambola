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
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn carambola_install_notification_delegate();
        }
        unsafe { carambola_install_notification_delegate() };
        capture::log_macos_startup_diagnostics();
    }

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
        .plugin(tauri_plugin_notification::init())
        .manage(app_state.clone())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, shortcut, event| {
                    log::debug!(
                        "[hotkey] Event shortcut={shortcut:?} state={:?}",
                        event.state()
                    );
                    if event.state() == ShortcutState::Pressed {
                        info!("[hotkey] Global shortcut activated");
                        if let Err(err) = window::activate_overlay(app, &state_for_hotkey) {
                            window::report_activation_error(app, &err);
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
            commands::overlay_frontend_ready,
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
            commands::get_screenshot_notification_settings,
            commands::notify_macos_screenshot,
            commands::set_screenshot_notifications,
            commands::claim_notification_permission_prompt,
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

            // Configure and hide the existing overlay before making capture
            // activations available through the tray or global shortcut.
            window::prime_overlay(app.handle(), &state_for_setup);

            // Initialize System Tray
            if let Err(e) = tray::setup_tray(app.handle(), state_for_setup.clone()) {
                log::error!("Failed to initialize tray: {e}");
            }

            // Register global hotkey
            if let Err(e) = hotkey::setup_hotkey(app.handle(), state_for_setup.clone()) {
                log::error!("Failed to register global hotkey: {e}");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Error while running Carambola application");
}
