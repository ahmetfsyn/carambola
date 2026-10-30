use crate::app_state::AppState;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

#[derive(Serialize, Deserialize, Debug)]
pub struct UserConfig {
    pub shortcut: String,
}

fn get_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("carambola").join("config.json"))
}

pub fn load_or_create_config() -> String {
    let default_shortcut = if cfg!(target_os = "macos") {
        "CommandOrControl+Shift+X".to_string()
    } else {
        "PrintScreen".to_string()
    };

    if let Some(config_path) = get_config_path() {
        if config_path.exists() {
            if let Ok(content) = fs::read_to_string(&config_path) {
                if let Ok(cfg) = serde_json::from_str::<UserConfig>(&content) {
                    if !cfg.shortcut.trim().is_empty() {
                        return cfg.shortcut;
                    }
                }
            }
        } else {
            // Write default config for user convenience
            if let Some(parent) = config_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let initial_cfg = UserConfig {
                shortcut: default_shortcut.clone(),
            };
            if let Ok(json) = serde_json::to_string_pretty(&initial_cfg) {
                let _ = fs::write(&config_path, json);
                info!("Created default config file at {:?}", config_path);
            }
        }
    }

    default_shortcut
}

pub fn save_config(shortcut: &str) -> Result<(), String> {
    if let Some(config_path) = get_config_path() {
        if let Some(parent) = config_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let cfg = UserConfig {
            shortcut: shortcut.to_string(),
        };
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| format!("Failed to serialize config: {e}"))?;
        fs::write(&config_path, json)
            .map_err(|e| format!("Failed to write config file: {e}"))?;
        info!("Saved shortcut '{}' to {:?}", shortcut, config_path);
        Ok(())
    } else {
        Err("Could not determine config directory".into())
    }
}

pub fn setup_hotkey(app_handle: &AppHandle, _state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let shortcut_str = load_or_create_config();

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
            info!("Registered global shortcut: {:?}", shortcut);
        }
        Err(err) => {
            warn!("Failed to register {:?}: {err:?}. Trying fallback 'Alt+A'...", shortcut);
            if let Ok(fallback) = "Alt+A".parse::<Shortcut>() {
                let _ = app_handle.global_shortcut().register(fallback);
            }
        }
    }

    Ok(())
}

pub fn update_global_shortcut(app_handle: &AppHandle, new_shortcut_str: &str) -> Result<(), String> {
    let shortcut: Shortcut = new_shortcut_str
        .parse()
        .map_err(|e| format!("Invalid shortcut combination '{new_shortcut_str}': {e:?}"))?;

    // Unregister existing shortcuts
    let _ = app_handle.global_shortcut().unregister_all();

    // Register the new one
    app_handle
        .global_shortcut()
        .register(shortcut)
        .map_err(|e| format!("Could not register shortcut '{new_shortcut_str}': {e:?}"))?;

    // Persist to disk
    save_config(new_shortcut_str)?;
    info!("Successfully re-bound global shortcut to: {}", new_shortcut_str);

    Ok(())
}
