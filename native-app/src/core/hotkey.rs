//! Platform-independent description and validation for the Windows hotkey.

pub const MOD_CONTROL: u32 = 0x0002;
pub const MOD_SHIFT: u32 = 0x0004;
const ALLOWED_MODIFIERS: u32 = MOD_CONTROL | MOD_SHIFT;
const ERROR_HOTKEY_ALREADY_REGISTERED: u32 = 1409;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HotkeyConfig {
    pub modifiers: u32,
    pub virtual_key: u32,
}

impl HotkeyConfig {
    pub const DEFAULT: Self = Self {
        modifiers: MOD_CONTROL | MOD_SHIFT,
        virtual_key: b'S' as u32,
    };

    pub fn validate(self) -> Result<(), &'static str> {
        if self.modifiers & !ALLOWED_MODIFIERS != 0 {
            return Err("the shortcut contains unsupported modifiers");
        }
        if self.modifiers & MOD_CONTROL == 0 || self.modifiers & MOD_SHIFT == 0 {
            return Err("the shortcut must include Ctrl and Shift");
        }
        if !(b'A' as u32..=b'Z' as u32).contains(&self.virtual_key) {
            return Err("the shortcut key must be an A-Z letter");
        }
        Ok(())
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
    use super::{HotkeyConfig, RegistrationState, MOD_CONTROL, MOD_SHIFT};

    #[test]
    fn default_shortcut_is_valid() {
        assert_eq!(HotkeyConfig::DEFAULT.validate(), Ok(()));
        assert_eq!(HotkeyConfig::DEFAULT.modifiers, MOD_CONTROL | MOD_SHIFT);
        assert_eq!(HotkeyConfig::DEFAULT.virtual_key, b'S' as u32);
    }

    #[test]
    fn shortcut_validation_requires_ctrl_shift_and_a_letter() {
        assert!(HotkeyConfig {
            modifiers: MOD_SHIFT,
            virtual_key: b'S' as u32,
        }
        .validate()
        .is_err());
        assert!(HotkeyConfig {
            modifiers: MOD_CONTROL,
            virtual_key: b'S' as u32,
        }
        .validate()
        .is_err());
        assert!(HotkeyConfig {
            modifiers: MOD_CONTROL | MOD_SHIFT,
            virtual_key: b'1' as u32,
        }
        .validate()
        .is_err());
        assert!(HotkeyConfig {
            modifiers: MOD_CONTROL | MOD_SHIFT | 0x0001,
            virtual_key: b'S' as u32,
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
}
