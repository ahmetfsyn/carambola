//! Platform-independent description and validation for the Windows hotkey.

pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
pub const MOD_ALT: u32 = 0x0001;
const ALLOWED_MODIFIERS: u32 = MOD_ALT | MOD_CONTROL | MOD_SHIFT;
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyConfig {
    pub modifiers: u32,
    pub virtual_key: u32,
    /// Unicode character recorded by Slint for layout-dependent keys. Windows
    /// resolves this to the active layout's virtual key when registering.
    #[serde(default)]
    pub key_character: u32,
}

impl HotkeyConfig {
    pub const DEFAULT: Self = Self {
        modifiers: MOD_CONTROL | MOD_SHIFT,
        virtual_key: b'S' as u32,
        key_character: 0,
    };

    pub fn validate(self) -> Result<(), &'static str> {
        if self.modifiers & !ALLOWED_MODIFIERS != 0 {
            return Err("the shortcut contains unsupported modifiers");
        }
        if self.modifiers == 0 {
            return Err("the shortcut must include Ctrl, Shift, or Alt");
        }
        if self.key_character != 0 {
            let Some(character) = char::from_u32(self.key_character) else {
                return Err("the shortcut key is not a valid Unicode character");
            };
            if character.is_control()
                || character.len_utf16() != 1
                || (self.virtual_key != 0 && !is_supported_virtual_key(self.virtual_key))
            {
                return Err("the shortcut character cannot be registered by Windows");
            }
        } else if !is_supported_virtual_key(self.virtual_key) {
            return Err("the shortcut key is unsupported or is a modifier key");
        }
        Ok(())
    }

    /// Upgrade the old persisted ASCII virtual-key representation to a
    /// layout-dependent logical letter without changing its displayed key.
    pub fn normalize_legacy_letter(mut self) -> Self {
        if self.key_character == 0 && (b'A' as u32..=b'Z' as u32).contains(&self.virtual_key) {
            self.key_character = char::from_u32(self.virtual_key)
                .expect("ASCII letter is valid Unicode")
                .to_ascii_lowercase() as u32;
            self.virtual_key = 0;
        }
        self
    }

    pub fn from_recorded_key(
        text: &str,
        control: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    ) -> Result<Self, &'static str> {
        if meta {
            return Err("The Windows key is not supported for the capture shortcut");
        }
        let mut chars = text.chars();
        let Some(character) = chars.next() else {
            return Err("Press a key with Ctrl, Shift, or Alt");
        };
        if chars.next().is_some() {
            return Err("The capture shortcut must contain exactly one key");
        }
        let (virtual_key, key_character) = match slint_named_key(character) {
            Some(virtual_key) => (virtual_key, 0),
            None if !character.is_control() && character.len_utf16() == 1 => (0, character as u32),
            None => return Err("This key cannot be registered as a Windows global shortcut"),
        };
        let config = Self {
            modifiers: (if control { MOD_CONTROL } else { 0 })
                | (if shift { MOD_SHIFT } else { 0 })
                | (if alt { MOD_ALT } else { 0 }),
            virtual_key,
            key_character,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn display(self) -> String {
        if self.validate().is_err() {
            return "Unsupported shortcut".to_owned();
        }
        let key = if self.key_character != 0 {
            match char::from_u32(self.key_character) {
                Some(' ') => "Space".to_owned(),
                Some(character) if character.is_ascii_alphabetic() => {
                    character.to_ascii_uppercase().to_string()
                }
                Some(character) => character.to_string(),
                None => "?".to_owned(),
            }
        } else {
            virtual_key_name(self.virtual_key)
                .map(str::to_owned)
                .unwrap_or_else(|| char::from_u32(self.virtual_key).unwrap_or('?').to_string())
        };
        let mut label = String::new();
        if self.modifiers & MOD_CONTROL != 0 {
            label.push_str("Ctrl+");
        }
        if self.modifiers & MOD_SHIFT != 0 {
            label.push_str("Shift+");
        }
        if self.modifiers & MOD_ALT != 0 {
            label.push_str("Alt+");
        }
        label.push_str(&key);
        label
    }
}

fn is_supported_virtual_key(key: u32) -> bool {
    if !(1..=0xfe).contains(&key) {
        return false;
    }
    // Modifier-only keys and Windows' synthetic packet/process keys cannot be
    // used as a reliable RegisterHotKey primary key.
    !matches!(key, 0x10..=0x12 | 0x5b..=0x5c | 0xa0..=0xa5 | 0xe5 | 0xe7)
}

fn slint_named_key(character: char) -> Option<u32> {
    Some(match character as u32 {
        0x08 => 0x08,                                          // Backspace
        0x09 => 0x09,                                          // Tab
        0x0a => 0x0d,                                          // Return
        0x1b => 0x1b,                                          // Escape
        0x7f => 0x2e,                                          // Delete
        0x14 => 0x14,                                          // Caps Lock
        0x20 => 0x20,                                          // Space
        0xf700 => 0x26,                                        // Up
        0xf701 => 0x28,                                        // Down
        0xf702 => 0x25,                                        // Left
        0xf703 => 0x27,                                        // Right
        0xf704..=0xf71b => 0x70 + (character as u32 - 0xf704), // F1-F24
        0xf727 => 0x2d,                                        // Insert
        0xf729 => 0x24,                                        // Home
        0xf72b => 0x23,                                        // End
        0xf72c => 0x21,                                        // Page Up
        0xf72d => 0x22,                                        // Page Down
        0xf72f => 0x91,                                        // Scroll Lock
        0xf730 => 0x13,                                        // Pause
        0xf731 => 0x2c,                                        // Print Screen
        0xf735 => 0x5d,                                        // Context Menu
        0xf748 => 0xa6,                                        // Browser Back
        _ => return None,
    })
}

fn virtual_key_name(key: u32) -> Option<&'static str> {
    Some(match key {
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0d => "Enter",
        0x13 => "Pause",
        0x14 => "Caps Lock",
        0x1b => "Escape",
        0x20 => "Space",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2c => "Print Screen",
        0x2d => "Insert",
        0x2e => "Delete",
        0x5d => "Menu",
        0x70..=0x87 => return Some(FUNCTION_KEY_NAMES[(key - 0x70) as usize]),
        0x91 => "Scroll Lock",
        0xa6 => "Browser Back",
        _ => return None,
    })
}

const FUNCTION_KEY_NAMES: [&str; 24] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "F13", "F14", "F15",
    "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24",
];

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationState {
    Registered,
    Conflict,
    Failed(u32),
}

impl RegistrationState {
    pub fn from_result(success: bool, error_code: u32) -> Self {
        if success {
            Self::Registered
        } else if error_code == ERROR_HOTKEY_ALREADY_REGISTERED {
            Self::Conflict
        } else {
            Self::Failed(error_code)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HotkeyConfig, RegistrationState, MOD_ALT, MOD_CONTROL, MOD_SHIFT};

    #[test]
    fn default_shortcut_is_valid() {
        assert_eq!(HotkeyConfig::DEFAULT.validate(), Ok(()));
        assert_eq!(HotkeyConfig::DEFAULT.modifiers, MOD_CONTROL | MOD_SHIFT);
        assert_eq!(HotkeyConfig::DEFAULT.virtual_key, b'S' as u32);
    }

    #[test]
    fn legacy_letters_convert_to_layout_dependent_characters() {
        let legacy = HotkeyConfig {
            modifiers: MOD_CONTROL,
            virtual_key: b'Q' as u32,
            key_character: 0,
        }
        .normalize_legacy_letter();
        assert_eq!(legacy.virtual_key, 0);
        assert_eq!(legacy.key_character, 'q' as u32);
        assert_eq!(legacy.display(), "Ctrl+Q");
    }

    #[test]
    fn shortcut_validation_accepts_all_supported_modifiers_and_virtual_keys() {
        for modifiers in [
            MOD_SHIFT,
            MOD_CONTROL,
            MOD_ALT,
            MOD_CONTROL | MOD_ALT,
            MOD_ALT | MOD_SHIFT,
            MOD_CONTROL | MOD_SHIFT | MOD_ALT,
        ] {
            assert_eq!(
                HotkeyConfig {
                    modifiers,
                    virtual_key: b'S' as u32,
                    key_character: 0,
                }
                .validate(),
                Ok(())
            );
        }
        assert!(HotkeyConfig {
            modifiers: 0,
            virtual_key: b'S' as u32,
            key_character: 0,
        }
        .validate()
        .is_err());
        assert!(HotkeyConfig {
            modifiers: MOD_CONTROL | MOD_SHIFT | 0x0008,
            virtual_key: b'S' as u32,
            key_character: 0,
        }
        .validate()
        .is_err());
    }

    #[test]
    fn hotkey_registration_outcomes_are_distinguished() {
        assert_eq!(
            RegistrationState::from_result(true, 0),
            RegistrationState::Registered
        );
        assert_eq!(
            RegistrationState::from_result(false, 1409),
            RegistrationState::Conflict
        );
        assert_eq!(
            RegistrationState::from_result(false, 5),
            RegistrationState::Failed(5)
        );
    }

    #[test]
    fn recorded_shortcuts_accept_punctuation_digits_letters_and_function_keys() {
        let cases = [
            ("\"", false, false, true, MOD_ALT, "Alt+\""),
            ("\"", false, true, true, MOD_SHIFT | MOD_ALT, "Shift+Alt+\""),
            (
                "\"",
                true,
                false,
                true,
                MOD_CONTROL | MOD_ALT,
                "Ctrl+Alt+\"",
            ),
            ("7", true, false, false, MOD_CONTROL, "Ctrl+7"),
            (";", false, true, true, MOD_SHIFT | MOD_ALT, "Shift+Alt+;"),
            ("a", true, false, false, MOD_CONTROL, "Ctrl+A"),
            (
                "Z",
                true,
                true,
                true,
                MOD_CONTROL | MOD_SHIFT | MOD_ALT,
                "Ctrl+Shift+Alt+Z",
            ),
        ];
        for (key, control, shift, alt, modifiers, display) in cases {
            let shortcut =
                HotkeyConfig::from_recorded_key(key, control, shift, alt, false).unwrap();
            assert_eq!(shortcut.modifiers, modifiers);
            assert_eq!(shortcut.key_character, key.chars().next().unwrap() as u32);
            assert_eq!(shortcut.virtual_key, 0);
            assert_eq!(shortcut.display(), display);
        }

        for (key, vk, label) in [("\u{f704}", 0x70, "F1"), ("\u{f71b}", 0x87, "F24")] {
            let shortcut = HotkeyConfig::from_recorded_key(key, false, false, true, false).unwrap();
            assert_eq!(shortcut.virtual_key, vk);
            assert_eq!(shortcut.display(), format!("Alt+{label}"));
        }
        assert_eq!(HotkeyConfig::DEFAULT.display(), "Ctrl+Shift+S");
    }

    #[test]
    fn recorded_shortcuts_reject_modifier_only_and_unmappable_unicode_keys() {
        assert!(HotkeyConfig::from_recorded_key("x", false, false, false, false).is_err());
        assert!(HotkeyConfig::from_recorded_key("\u{10}", true, false, false, false).is_err());
        assert!(HotkeyConfig::from_recorded_key("x", false, false, true, true).is_err());
        assert!(HotkeyConfig::from_recorded_key("😀", true, false, false, false).is_err());
        assert!(HotkeyConfig {
            modifiers: MOD_ALT,
            virtual_key: 0x12,
            key_character: 0,
        }
        .validate()
        .is_err());
    }
}
