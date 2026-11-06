//! Small lifecycle decisions shared by the native UI and platform adapters.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SettingsVisibility {
    Hidden = 0,
    Visible = 1,
    Minimized = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsRestoreAction {
    Hide,
    Show,
    ShowMinimized,
}

impl SettingsVisibility {
    pub fn from_window_state(visible: bool, minimized: bool) -> Self {
        if !visible {
            Self::Hidden
        } else if minimized {
            Self::Minimized
        } else {
            Self::Visible
        }
    }

    pub fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Visible,
            2 => Self::Minimized,
            _ => Self::Hidden,
        }
    }

    pub const fn restore_action(self) -> SettingsRestoreAction {
        match self {
            Self::Hidden => SettingsRestoreAction::Hide,
            Self::Visible => SettingsRestoreAction::Show,
            Self::Minimized => SettingsRestoreAction::ShowMinimized,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    NewScreenshot,
    OpenCarambola,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    StartCapture,
    ShowSettings,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosePolicy {
    Hide,
    KeepShown,
}

pub fn settings_close_policy(tray_ready: bool) -> ClosePolicy {
    if tray_ready {
        ClosePolicy::Hide
    } else {
        ClosePolicy::KeepShown
    }
}

pub const fn overlay_close_policy() -> ClosePolicy {
    ClosePolicy::KeepShown
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrayResourceOwnership {
    hotkey_registered: bool,
    icon_added: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrayCleanupPlan {
    pub unregister_hotkey: bool,
    pub remove_icon: bool,
}

impl TrayResourceOwnership {
    pub fn mark_hotkey_registered(&mut self) {
        self.hotkey_registered = true;
    }

    pub fn mark_icon_added(&mut self) {
        self.icon_added = true;
    }

    pub fn release(&mut self) -> TrayCleanupPlan {
        let plan = TrayCleanupPlan {
            unregister_hotkey: self.hotkey_registered,
            remove_icon: self.icon_added,
        };
        self.hotkey_registered = false;
        self.icon_added = false;
        plan
    }
}

impl TrayCommand {
    pub fn action(self) -> AppAction {
        match self {
            Self::NewScreenshot => AppAction::StartCapture,
            Self::OpenCarambola => AppAction::ShowSettings,
            Self::Exit => AppAction::Quit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        overlay_close_policy, settings_close_policy, AppAction, ClosePolicy, SettingsRestoreAction,
        SettingsVisibility, TrayCommand, TrayResourceOwnership,
    };

    #[test]
    fn settings_visibility_round_trips_for_capture_restore() {
        for (visible, minimized, expected) in [
            (false, false, SettingsVisibility::Hidden),
            (true, false, SettingsVisibility::Visible),
            (true, true, SettingsVisibility::Minimized),
        ] {
            let state = SettingsVisibility::from_window_state(visible, minimized);
            assert_eq!(state, expected);
            assert_eq!(SettingsVisibility::from_code(state as u8), state);
        }
    }

    #[test]
    fn capture_restore_action_preserves_hidden_visible_and_minimized_state() {
        assert_eq!(
            SettingsVisibility::Hidden.restore_action(),
            SettingsRestoreAction::Hide
        );
        assert_eq!(
            SettingsVisibility::Visible.restore_action(),
            SettingsRestoreAction::Show
        );
        assert_eq!(
            SettingsVisibility::Minimized.restore_action(),
            SettingsRestoreAction::ShowMinimized
        );
    }

    #[test]
    fn tray_commands_map_to_the_application_actions() {
        assert_eq!(TrayCommand::NewScreenshot.action(), AppAction::StartCapture);
        assert_eq!(TrayCommand::OpenCarambola.action(), AppAction::ShowSettings);
        assert_eq!(TrayCommand::Exit.action(), AppAction::Quit);
    }

    #[test]
    fn tray_cleanup_releases_only_acquired_resources_once() {
        let mut ownership = TrayResourceOwnership::default();
        assert_eq!(
            ownership.release(),
            super::TrayCleanupPlan {
                unregister_hotkey: false,
                remove_icon: false,
            }
        );

        ownership.mark_hotkey_registered();
        ownership.mark_icon_added();
        assert_eq!(
            ownership.release(),
            super::TrayCleanupPlan {
                unregister_hotkey: true,
                remove_icon: true,
            }
        );
        assert_eq!(
            ownership.release(),
            super::TrayCleanupPlan {
                unregister_hotkey: false,
                remove_icon: false,
            }
        );
    }

    #[test]
    fn settings_close_hides_only_when_the_tray_can_restore_the_app() {
        assert_eq!(settings_close_policy(true), ClosePolicy::Hide);
        assert_eq!(settings_close_policy(false), ClosePolicy::KeepShown);
    }

    #[test]
    fn overlay_close_does_not_discard_an_active_capture() {
        assert_eq!(overlay_close_policy(), ClosePolicy::KeepShown);
    }
}
