import { invoke } from "@tauri-apps/api/core";
import { Rect } from "../../types/crop";
import { DisplayInfo } from "../../types/display";

export async function getDisplayInfo(): Promise<DisplayInfo> {
  try {
    return await invoke<DisplayInfo>("get_display_info");
  } catch (err) {
    console.warn("Using fallback display info:", err);
    return {
      id: 0,
      name: "Primary Display",
      isPrimary: true,
      scaleFactor: window.devicePixelRatio || 1,
      bounds: {
        x: 0,
        y: 0,
        width: window.innerWidth,
        height: window.innerHeight,
      },
      physicalWidth: window.innerWidth * (window.devicePixelRatio || 1),
      physicalHeight: window.innerHeight * (window.devicePixelRatio || 1),
    };
  }
}

/** Returns the frozen frame PNG as raw bytes (Tauri binary IPC body). */
export async function getCurrentFrameBytes(): Promise<ArrayBuffer> {
  const bytes = await invoke<ArrayBuffer | Uint8Array | number[]>("get_current_frame");
  if (bytes instanceof ArrayBuffer) return bytes;
  if (bytes instanceof Uint8Array) {
    return bytes.buffer.slice(
      bytes.byteOffset,
      bytes.byteOffset + bytes.byteLength
    ) as ArrayBuffer;
  }
  return new Uint8Array(bytes).buffer;
}

/**
 * Second half of an activation: shows the pre-created overlay window only after
 * the frontend has decoded the fresh frame and cleared the previous session.
 */
export interface OverlayRenderMetrics {
  captureId: number;
  viewportWidth: number;
  viewportHeight: number;
  dpr: number;
  canvasWidth: number;
  canvasHeight: number;
  imageWidth: number;
  imageHeight: number;
}

export async function acknowledgeOverlayRender(metrics: OverlayRenderMetrics): Promise<void> {
  await invoke("acknowledge_overlay_render", { ...metrics });
}

export async function presentOverlay(captureId: number): Promise<void> {
  await invoke("present_overlay", { captureId });
}

/** Signals that the hidden overlay has subscribed to capture events. */
export async function markOverlayFrontendReady(): Promise<void> {
  await invoke("overlay_frontend_ready");
}

/**
 * Sends the composited crop (raw RGBA with an 8-byte dimension header) to the
 * backend as a raw binary IPC body. The backend copies it straight to the
 * clipboard, so the redacted, annotated pixels are exactly what the user saw.
 */
export async function copyCropImage(rgbaWithHeader: Uint8Array): Promise<void> {
  await invoke("copy_crop_image", rgbaWithHeader);
}

export async function saveCropImage(rgbaWithHeader: Uint8Array): Promise<string> {
  return await invoke<string>("save_crop_image", rgbaWithHeader);
}

export async function hideOverlay(): Promise<void> {
  await invoke("hide_overlay");
}

export async function extractOcrText(crop: Rect, scaleFactor: number): Promise<string> {
  return await invoke<string>("extract_ocr", {
    crop,
    scaleFactor,
  });
}

export async function getGlobalShortcut(): Promise<string> {
  try {
    return await invoke<string>("get_shortcut");
  } catch (err) {
    console.warn("Failed to get shortcut:", err);
    return "CommandOrControl+Shift+X";
  }
}

export async function setGlobalShortcut(shortcut: string): Promise<void> {
  await invoke("set_shortcut", { shortcut });
}

export interface AutostartStatus {
  supported: boolean;
  enabled: boolean;
  reason: string | null;
}

/** Reads the real OS startup registration, not just the stored preference. */
export async function getAutostartStatus(): Promise<AutostartStatus> {
  return await invoke<AutostartStatus>("get_autostart");
}

export async function setAutostartEnabled(enabled: boolean): Promise<AutostartStatus> {
  return await invoke<AutostartStatus>("set_autostart", { enabled });
}

export interface ScreenshotNotificationSettings {
  enabled: boolean;
  permission_prompted: boolean;
}

export async function getScreenshotNotificationSettings(): Promise<ScreenshotNotificationSettings> {
  return await invoke<ScreenshotNotificationSettings>("get_screenshot_notification_settings");
}

export async function setScreenshotNotifications(
  enabled: boolean
): Promise<ScreenshotNotificationSettings> {
  return await invoke<ScreenshotNotificationSettings>("set_screenshot_notifications", {
    enabled,
  });
}

export async function claimNotificationPermissionPrompt(): Promise<boolean> {
  return await invoke<boolean>("claim_notification_permission_prompt");
}

