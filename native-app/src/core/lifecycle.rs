//! Small lifecycle decisions shared by the native UI and platform adapters.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    NewScreenshot,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    StartCapture,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosePolicy {
    KeepShown,
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
            Self::Exit => AppAction::Quit,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{overlay_close_policy, AppAction, ClosePolicy, TrayCommand, TrayResourceOwnership};

    #[test]
    fn tray_commands_map_to_the_application_actions() {
        assert_eq!(TrayCommand::NewScreenshot.action(), AppAction::StartCapture);
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
    fn overlay_close_does_not_discard_an_active_capture() {
        assert_eq!(overlay_close_policy(), ClosePolicy::KeepShown);
    }
}
