use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Persistent user preferences shared by the global shortcut and startup logic.
///
/// `autostart` is optional so configs written by older versions (which only had
/// `shortcut`) migrate cleanly: `None` means "user has not chosen yet", which
/// triggers the default-on behaviour exactly once.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserConfig {
    pub shortcut: String,
    #[serde(default)]
    pub autostart: Option<bool>,
    #[serde(default = "default_screenshot_notifications")]
    pub screenshot_notifications: bool,
    #[serde(default)]
    pub notification_permission_prompted: bool,
}

fn default_screenshot_notifications() -> bool {
    true
}

pub fn default_shortcut() -> &'static str {
    if cfg!(target_os = "macos") {
        "CommandOrControl+Shift+X"
    } else {
        "PrintScreen"
    }
}

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("carambola").join("config.json"))
}

/// Loads the config, creating it with platform defaults on first run.
pub fn load_or_create() -> UserConfig {
    if let Some(path) = config_path() {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(mut cfg) = serde_json::from_str::<UserConfig>(&content) {
                    if cfg.shortcut.trim().is_empty() {
                        cfg.shortcut = default_shortcut().to_string();
                    }
                    return cfg;
                }
            }
        } else if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
            let cfg = UserConfig {
                shortcut: default_shortcut().to_string(),
                autostart: None,
                screenshot_notifications: default_screenshot_notifications(),
                notification_permission_prompted: false,
            };
            if let Ok(json) = serde_json::to_string_pretty(&cfg) {
                let _ = fs::write(&path, json);
            }
            return cfg;
        }
    }

    UserConfig {
        shortcut: default_shortcut().to_string(),
        autostart: None,
        screenshot_notifications: default_screenshot_notifications(),
        notification_permission_prompted: false,
    }
}

pub fn save(config: &UserConfig) -> Result<(), String> {
    let path = config_path().ok_or_else(|| "Could not determine config directory".to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {e}"))?;
    }
    let json = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {e}"))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write config file: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_without_autostart_deserializes() {
        let cfg: UserConfig = serde_json::from_str(r#"{"shortcut":"Alt+A"}"#).unwrap();
        assert_eq!(cfg.shortcut, "Alt+A");
        assert_eq!(cfg.autostart, None);
        assert!(cfg.screenshot_notifications);
        assert!(!cfg.notification_permission_prompted);
    }

    #[test]
    fn autostart_preference_round_trips() {
        let cfg: UserConfig =
            serde_json::from_str(r#"{"shortcut":"Alt+A","autostart":false}"#).unwrap();
        assert_eq!(cfg.autostart, Some(false));
        assert!(cfg.screenshot_notifications);
    }

    #[test]
    fn notification_preference_round_trips_without_resetting_other_settings() {
        let mut cfg: UserConfig = serde_json::from_str(
            r#"{"shortcut":"Alt+A","autostart":false,"screenshot_notifications":false,"notification_permission_prompted":true}"#,
        )
        .unwrap();
        assert!(!cfg.screenshot_notifications);
        assert!(cfg.notification_permission_prompted);

        cfg.screenshot_notifications = true;
        let saved = serde_json::to_string(&cfg).unwrap();
        let restored: UserConfig = serde_json::from_str(&saved).unwrap();
        assert_eq!(restored.shortcut, "Alt+A");
        assert_eq!(restored.autostart, Some(false));
        assert!(restored.screenshot_notifications);
        assert!(restored.notification_permission_prompted);
    }
}
