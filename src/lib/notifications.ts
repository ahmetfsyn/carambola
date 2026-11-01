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
  if (!settings.enabled) return false;

  try {
    if (/Mac/i.test(navigator.platform)) {
      await invoke("notify_macos_screenshot");
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
    return false;
  }
}
