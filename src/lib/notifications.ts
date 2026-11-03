import {
  isPermissionGranted,
  requestPermission,
  sendNotification,
} from "@tauri-apps/plugin-notification";
import { invoke } from "@tauri-apps/api/core";
import {
  claimNotificationPermissionPrompt,
  getScreenshotNotificationSettings,
  ScreenshotNotificationSettings,
} from "./ipc/commands";

/**
 * Shows one native notification after an exported screenshot. Permission is
 * requested at most once; a denied permission is left to OS Settings.
 */
export async function notifyScreenshotExported(
  settings: ScreenshotNotificationSettings
): Promise<boolean> {
  const isWindows = /Win/i.test(navigator.platform);
  const diagnose = async (stage: string, detail: string) => {
    if (!isWindows) return;
    try {
      await invoke("log_notification_diagnostic", { stage, detail });
    } catch (err) {
      console.error(`[notification] Could not persist ${stage} diagnostic:`, err);
    }
  };

  if (!settings.enabled) {
    await diagnose("frontend_preference", "screenshot_notifications=false; suppressed");
    return false;
  }

  try {
    if (/Mac/i.test(navigator.platform)) {
      await invoke("notify_macos_screenshot");
      return true;
    }
    if (isWindows) {
      await diagnose("delivery_begin", "export succeeded; checking Windows notification settings");
      const latestSettings = await getScreenshotNotificationSettings();
      await diagnose(
        "frontend_preference",
        `screenshot_notifications=${latestSettings.enabled}; permission_prompted=${latestSettings.permission_prompted}`
      );
      if (!latestSettings.enabled) {
        await diagnose("delivery_suppressed", "screenshot preference disabled on recheck");
        return false;
      }
      await diagnose("invoke_begin", "notify_windows_screenshot");
      await invoke("notify_windows_screenshot");
      await diagnose("invoke_resolved", "Rust command completed; inspect windows_api_result for OS acceptance");
      return true;
    }

    let granted = await isPermissionGranted();
    if (!granted) {
      if (settings.permission_prompted) return false;
      if (!(await claimNotificationPermissionPrompt())) return false;
      granted = (await requestPermission()) === "granted";
    }

    if (!granted) return false;
    if (!(await getScreenshotNotificationSettings()).enabled) return false;
    sendNotification({
      title: "Carambola",
      body: "Screenshot captured successfully.",
    });
    return true;
  } catch (err) {
    // Notification permission or delivery must never fail the screenshot flow.
    console.warn("Could not show screenshot notification:", err);
    await diagnose("delivery_failed", String(err));
    return false;
  }
}

/** Sends updater feedback independently of the screenshot notification preference. */
export async function notifyUpdateStatus(title: string, body: string): Promise<boolean> {
  try {
    if (/Win/i.test(navigator.platform)) {
      await invoke("notify_windows_update", { title, body });
      return true;
    }

    // Background update checks must not trigger a new permission prompt.
    if (!(await isPermissionGranted())) {
      console.warn("Update notification was not submitted because notification permission is unavailable.");
      return false;
    }
    sendNotification({ title, body });
    return true;
  } catch (error) {
    // Toast delivery is best-effort; updater outcomes remain available in Settings.
    console.error("Could not submit updater notification:", error);
    return false;
  }
}
