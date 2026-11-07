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

/// A capture may close only after its requested export has completed
/// successfully. Cancelled dialogs never start an export operation.
pub fn export_should_close_overlay<T, E>(result: &Result<T, E>) -> bool {
    result.is_ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Copy,
    Save,
}

pub fn export_notification<T, E>(result: &Result<T, E>, kind: ExportKind) -> Option<&'static str> {
    result.as_ref().ok().map(|_| match kind {
        ExportKind::Copy => "Screenshot copied",
        ExportKind::Save => "Screenshot saved",
    })
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

    #[test]
    fn only_successful_copy_or_save_completion_closes_the_overlay() {
        assert!(super::export_should_close_overlay::<(), ()>(&Ok(())));
        assert!(!super::export_should_close_overlay::<(), _>(&Err(
            "clipboard or file error"
        )));
    }

    #[test]
    fn native_notification_policy_only_returns_messages_for_successful_exports() {
        assert_eq!(
            super::export_notification(&Ok::<_, ()>(()), super::ExportKind::Copy),
            Some("Screenshot copied")
        );
        assert_eq!(
            super::export_notification(&Ok::<_, ()>(()), super::ExportKind::Save),
            Some("Screenshot saved")
        );
        assert_eq!(
            super::export_notification(&Err::<(), _>("clipboard failed"), super::ExportKind::Copy),
            None
        );
        assert_eq!(
            super::export_notification(&Err::<(), _>("save failed"), super::ExportKind::Save),
            None
        );
    }
}
