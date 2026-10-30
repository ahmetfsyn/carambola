import { invoke } from "@tauri-apps/api/core";
import { Rect } from "../../types/crop";
import { AnnotationElement } from "../../types/annotations";
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

export async function getCurrentFrameBytes(): Promise<Uint8Array> {
  const bytes = await invoke<number[] | Uint8Array>("get_current_frame");
  return bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
}

export async function copyCropToClipboard(
  crop: Rect,
  elements: AnnotationElement[],
  scaleFactor: number
): Promise<void> {
  await invoke("copy_crop", {
    crop,
    elements,
    scaleFactor,
  });
}

export async function saveCropToDisk(
  crop: Rect,
  elements: AnnotationElement[],
  scaleFactor: number
): Promise<string> {
  return await invoke<string>("save_crop", {
    crop,
    elements,
    scaleFactor,
  });
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

/// Temporary geometry diagnostics forwarded to the Rust log stream.
export function logDiagnostics(tag: string, data: Record<string, unknown>): void {
  invoke("frontend_log", { message: `${tag} ${JSON.stringify(data)}` }).catch(() => {
    // Diagnostics must never affect runtime behaviour.
  });
}
