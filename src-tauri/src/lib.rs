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
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use tauri_plugin_global_shortcut::ShortcutState;

struct DiagnosticWriter {
    file: File,
}

impl Write for DiagnosticWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(bytes);
        self.file.write_all(bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        self.file.flush()
    }
}

fn initialize_logging() -> Option<PathBuf> {
    let log_path = dirs::data_local_dir()?
        .join("Carambola")
        .join("logs")
        .join("carambola.log");
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .ok()?;
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        // Notification stages are needed to diagnose installed GUI builds even
        // when the launching shell sets RUST_LOG=warn.
        .filter_level(log::LevelFilter::Info)
        .format_timestamp_millis()
        .target(env_logger::Target::Pipe(Box::new(DiagnosticWriter {
            file,
        })))
        .init();
    Some(log_path)
}

pub fn run() {
    let log_path = initialize_logging();
    if log_path.is_none() {
        env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
            .format_timestamp_millis()
            .init();
        eprintln!("Could not open the Carambola diagnostic log file");
    }
    info!("Starting Carambola application...");
    if let Some(path) = log_path {
        info!("[diagnostics] log_file={}", path.display());
    }
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
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
            commands::notify_windows_screenshot,
            commands::notify_windows_update,
            commands::log_notification_diagnostic,
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
