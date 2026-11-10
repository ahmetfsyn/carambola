use crate::core::hotkey::HotkeyConfig;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

use super::atomic_file;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppSettings {
    pub save_directory: Option<PathBuf>,
    pub capture_shortcut: HotkeyConfig,
    pub notifications_enabled: bool,
    pub launch_on_startup: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            save_directory: default_save_directory(),
            capture_shortcut: HotkeyConfig::DEFAULT,
            notifications_enabled: true,
            launch_on_startup: true,
        }
    }
}

impl AppSettings {
    pub fn load() -> Result<(Self, PathBuf), String> {
        let path = settings_file_path().ok_or_else(|| {
            "Could not find a per-user settings directory for Carambola".to_string()
        })?;
        let settings = if path.exists() {
            let contents = fs::read_to_string(&path)
                .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
            serde_json::from_str(&contents)
                .map_err(|error| format!("Could not parse {}: {error}", path.display()))?
        } else {
            Self::default()
        };
        Ok((settings, path))
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        let parent = path
            .parent()
            .ok_or_else(|| "Carambola settings path has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create {}: {error}", parent.display()))?;
        let contents = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("Could not encode Carambola settings: {error}"))?;
        atomic_file::write(path, &contents)
            .map_err(|error| format!("Could not save {}: {error}", path.display()))
    }
}

pub fn settings_file_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let root = std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .map(|path| path.join("AppData").join("Roaming"))
    });

    #[cfg(target_os = "macos")]
    let root = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join("Library").join("Application Support"));

    #[cfg(target_os = "linux")]
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|path| path.join(".config"))
        });

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let root: Option<PathBuf> = None;

    root.map(|path| path.join("Carambola").join("settings.json"))
}

pub fn default_save_directory() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);

    #[cfg(not(target_os = "windows"))]
    let home = std::env::var_os("HOME").map(PathBuf::from);

    home.map(|path| path.join("Pictures"))
}

pub fn validate_save_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("The save folder {} is unavailable: {error}", path.display()))?;
    if !metadata.is_dir() {
        return Err(format!(
            "The save location {} is not a folder",
            path.display()
        ));
    }
    fs::read_dir(path).map_err(|error| {
        format!(
            "The save folder {} cannot be accessed: {error}",
            path.display()
        )
    })?;
    Ok(())
}

pub trait ShortcutRegistrar {
    /// Implementations must leave `current` active if they return an error.
    fn replace(&mut self, current: HotkeyConfig, requested: HotkeyConfig) -> Result<(), String>;
}

pub trait StartupRegistrar {
    fn is_enabled(&mut self) -> Result<bool, String>;
    fn set_enabled(&mut self, enabled: bool) -> Result<(), String>;
}

pub fn change_startup_preference(
    settings: &mut AppSettings,
    requested: bool,
    registrar: &mut impl StartupRegistrar,
    persist: impl FnOnce(&AppSettings) -> Result<(), String>,
) -> Result<(), String> {
    let previous_registration = registrar.is_enabled()?;
    registrar.set_enabled(requested)?;

    let mut updated = settings.clone();
    updated.launch_on_startup = requested;
    if let Err(persist_error) = persist(&updated) {
        return match registrar.set_enabled(previous_registration) {
            Ok(()) => Err(persist_error),
            Err(rollback_error) => Err(format!(
                "{persist_error}; the previous Windows startup registration could not be restored: {rollback_error}"
            )),
        };
    }

    settings.launch_on_startup = requested;
    Ok(())
}

pub fn change_shortcut(
    settings: &mut AppSettings,
    requested: HotkeyConfig,
    registrar: &mut impl ShortcutRegistrar,
    persist: impl FnOnce(&AppSettings) -> Result<(), String>,
) -> Result<(), String> {
    requested
        .validate()
        .map_err(|error| format!("Invalid capture shortcut: {error}"))?;
    let current = settings.capture_shortcut;
    if requested == current {
        // A stored shortcut may have failed registration at startup. Let the
        // registrar retry and report a conflict instead of claiming success.
        return registrar.replace(current, requested);
    }
    registrar.replace(current, requested)?;
    let mut updated = settings.clone();
    updated.capture_shortcut = requested;
    if let Err(persist_error) = persist(&updated) {
        return match registrar.replace(requested, current) {
            Ok(()) => Err(persist_error),
            Err(rollback_error) => Err(format!(
                "{persist_error}; the previous shortcut could not be restored: {rollback_error}"
            )),
        };
    }
    settings.capture_shortcut = requested;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        change_shortcut, change_startup_preference, validate_save_directory, AppSettings,
        ShortcutRegistrar, StartupRegistrar,
    };
    use crate::core::hotkey::HotkeyConfig;
    use std::{
        cell::RefCell,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    fn temporary_path() -> PathBuf {
        let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "carambola-settings-test-{}-{id}",
            std::process::id()
        ))
    }

    struct MockRegistrar {
        results: Vec<Result<(), String>>,
        calls: Vec<(HotkeyConfig, HotkeyConfig)>,
    }

    struct MockStartupRegistrar {
        enabled: bool,
        set_results: Vec<Result<(), String>>,
        calls: Vec<bool>,
    }

    impl StartupRegistrar for MockStartupRegistrar {
        fn is_enabled(&mut self) -> Result<bool, String> {
            Ok(self.enabled)
        }

        fn set_enabled(&mut self, enabled: bool) -> Result<(), String> {
            self.calls.push(enabled);
            if !self.set_results.is_empty() {
                self.set_results.remove(0)?;
            }
            self.enabled = enabled;
            Ok(())
        }
    }

    impl ShortcutRegistrar for MockRegistrar {
        fn replace(
            &mut self,
            current: HotkeyConfig,
            requested: HotkeyConfig,
        ) -> Result<(), String> {
            self.calls.push((current, requested));
            if self.results.is_empty() {
                Ok(())
            } else {
                self.results.remove(0)
            }
        }
    }

    fn alternate_shortcut() -> HotkeyConfig {
        HotkeyConfig {
            virtual_key: b'X' as u32,
            ..HotkeyConfig::DEFAULT
        }
    }

    #[test]
    fn settings_round_trip_and_legacy_shortcut_only_file_load() {
        let path = temporary_path().join("settings.json");
        let mut settings = AppSettings::default();
        settings.capture_shortcut = alternate_shortcut();
        settings.save_to(&path).unwrap();
        let loaded: AppSettings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(loaded, settings);
        settings.capture_shortcut =
            HotkeyConfig::from_recorded_key("\"", false, false, true, false)
                .expect("quoted character is a valid logical shortcut");
        settings.save_to(&path).unwrap();
        let layout_dependent: AppSettings =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(layout_dependent, settings);
        assert_eq!(layout_dependent.capture_shortcut.key_character, '"' as u32);
        settings.save_to(&path).unwrap();
        let replaced: AppSettings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(replaced, settings);

        fs::write(
            &path,
            r#"{"capture_shortcut":{"modifiers":6,"virtual_key":83}}"#,
        )
        .unwrap();
        let legacy: AppSettings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(legacy.capture_shortcut, HotkeyConfig::DEFAULT);
        assert_eq!(legacy.save_directory, AppSettings::default().save_directory);
        assert!(legacy.notifications_enabled);
        assert!(legacy.launch_on_startup);
        settings.notifications_enabled = false;
        settings.save_to(&path).unwrap();
        let disabled: AppSettings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(!disabled.notifications_enabled);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn inaccessible_or_non_directory_save_location_is_rejected() {
        let file = temporary_path();
        fs::write(&file, "not a directory").unwrap();
        assert!(validate_save_directory(&file).is_err());
        let missing = file.with_extension("missing");
        assert!(validate_save_directory(&missing).is_err());
        let _ = fs::remove_file(file);
    }

    #[test]
    fn failed_registration_preserves_current_shortcut_and_does_not_persist() {
        let mut settings = AppSettings::default();
        let mut registrar = MockRegistrar {
            results: vec![Err("shortcut is taken".into())],
            calls: Vec::new(),
        };
        let persisted = RefCell::new(false);
        let result = change_shortcut(&mut settings, alternate_shortcut(), &mut registrar, |_| {
            *persisted.borrow_mut() = true;
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(settings.capture_shortcut, HotkeyConfig::DEFAULT);
        assert!(!*persisted.borrow());
        assert_eq!(registrar.calls.len(), 1);
    }

    #[test]
    fn retrying_the_saved_shortcut_reports_an_existing_conflict() {
        let mut settings = AppSettings::default();
        let mut registrar = MockRegistrar {
            results: vec![Err("shortcut is taken".into())],
            calls: Vec::new(),
        };
        let result = change_shortcut(&mut settings, HotkeyConfig::DEFAULT, &mut registrar, |_| {
            panic!("an unchanged shortcut must not be saved again")
        });
        assert_eq!(result, Err("shortcut is taken".into()));
        assert_eq!(
            registrar.calls,
            vec![(HotkeyConfig::DEFAULT, HotkeyConfig::DEFAULT)]
        );
        assert_eq!(settings.capture_shortcut, HotkeyConfig::DEFAULT);
    }

    #[test]
    fn invalid_shortcut_is_rejected_before_registration_or_persistence() {
        let mut settings = AppSettings::default();
        let invalid = HotkeyConfig {
            modifiers: 0,
            virtual_key: b'X' as u32,
            key_character: 0,
        };
        let mut registrar = MockRegistrar {
            results: Vec::new(),
            calls: Vec::new(),
        };
        let persisted = RefCell::new(false);
        let result = change_shortcut(&mut settings, invalid, &mut registrar, |_| {
            *persisted.borrow_mut() = true;
            Ok(())
        });
        assert!(result.is_err());
        assert!(registrar.calls.is_empty());
        assert!(!*persisted.borrow());
        assert_eq!(settings.capture_shortcut, HotkeyConfig::DEFAULT);
    }

    #[test]
    fn failed_persistence_rolls_back_the_registered_shortcut() {
        let mut settings = AppSettings::default();
        let mut registrar = MockRegistrar {
            results: vec![Ok(()), Ok(())],
            calls: Vec::new(),
        };
        let result = change_shortcut(&mut settings, alternate_shortcut(), &mut registrar, |_| {
            Err("settings file is read-only".into())
        });
        assert!(result.is_err());
        assert_eq!(settings.capture_shortcut, HotkeyConfig::DEFAULT);
        assert_eq!(
            registrar.calls,
            vec![
                (HotkeyConfig::DEFAULT, alternate_shortcut()),
                (alternate_shortcut(), HotkeyConfig::DEFAULT),
            ]
        );
    }

    #[test]
    fn startup_is_enabled_by_default_and_saved_off_preference_survives_upgrade() {
        assert!(AppSettings::default().launch_on_startup);

        let existing: AppSettings = serde_json::from_str(r#"{"launch_on_startup":false}"#).unwrap();
        assert!(!existing.launch_on_startup);
    }

    #[test]
    fn explicit_off_startup_preference_persists_across_settings_reload() {
        let path = temporary_path().join("settings.json");
        let settings = AppSettings {
            launch_on_startup: false,
            ..AppSettings::default()
        };
        settings.save_to(&path).unwrap();

        let reloaded: AppSettings = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(!reloaded.launch_on_startup);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn enabling_and_disabling_startup_updates_registration_and_persists_choice() {
        let mut settings = AppSettings::default();
        let mut registrar = MockStartupRegistrar {
            enabled: false,
            set_results: Vec::new(),
            calls: Vec::new(),
        };
        let persisted = RefCell::new(Vec::new());

        change_startup_preference(&mut settings, true, &mut registrar, |updated| {
            persisted.borrow_mut().push(updated.launch_on_startup);
            Ok(())
        })
        .unwrap();
        assert!(settings.launch_on_startup);
        assert!(registrar.enabled);

        change_startup_preference(&mut settings, false, &mut registrar, |updated| {
            persisted.borrow_mut().push(updated.launch_on_startup);
            Ok(())
        })
        .unwrap();
        assert!(!settings.launch_on_startup);
        assert!(!registrar.enabled);
        assert_eq!(*persisted.borrow(), vec![true, false]);
        assert_eq!(registrar.calls, vec![true, false]);
    }

    #[test]
    fn failed_startup_registration_does_not_change_or_persist_preference() {
        let mut settings = AppSettings::default();
        let mut registrar = MockStartupRegistrar {
            enabled: false,
            set_results: vec![Err("registry is not writable".into())],
            calls: Vec::new(),
        };
        let persisted = RefCell::new(false);

        let result = change_startup_preference(&mut settings, true, &mut registrar, |_| {
            *persisted.borrow_mut() = true;
            Ok(())
        });

        assert_eq!(result, Err("registry is not writable".into()));
        assert!(settings.launch_on_startup);
        assert!(!registrar.enabled);
        assert!(!*persisted.borrow());
    }

    #[test]
    fn startup_preference_persistence_failure_restores_previous_registration() {
        let mut settings = AppSettings {
            launch_on_startup: false,
            ..AppSettings::default()
        };
        let mut registrar = MockStartupRegistrar {
            enabled: false,
            set_results: vec![Ok(()), Ok(())],
            calls: Vec::new(),
        };

        let result = change_startup_preference(&mut settings, true, &mut registrar, |_| {
            Err("settings file is read-only".into())
        });

        assert_eq!(result, Err("settings file is read-only".into()));
        assert!(!settings.launch_on_startup);
        assert!(!registrar.enabled);
        assert_eq!(registrar.calls, vec![true, false]);
    }
}
