import React, { useState, useEffect, useRef } from "react";
import { flushSync } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import { OverlayContainer } from "./components/overlay/OverlayContainer";
import {
  getDisplayInfo,
  getCurrentFrameBytes,
  presentOverlay,
  hideOverlay,
} from "./lib/ipc/commands";
import { DisplayInfo } from "./types/display";

/** Payload emitted by the backend once a fresh frame is frozen in memory. */
interface CaptureReadyPayload {
  triggeredAt: number;
  display: DisplayInfo;
  captureMs: number;
  encodeMs: number;
  pngBytes: number;
}

type FrameImage = ImageBitmap | HTMLImageElement;

/**
 * Turns the raw PNG payload into a decoded image. `createImageBitmap` decodes
 * off the main thread; the DOM `Image` path is a fallback for older webviews.
 */
async function decodeFrame(bytes: ArrayBuffer): Promise<FrameImage> {
  const blob = new Blob([bytes], { type: "image/png" });
  if (typeof createImageBitmap === "function") {
    try {
      return await createImageBitmap(blob);
    } catch {
      // fall through to the <img> decoder
    }
  }
  const objectUrl = URL.createObjectURL(blob);
  try {
    const img = new Image();
    img.src = objectUrl;
    await img.decode();
    return img;
  } finally {
    URL.revokeObjectURL(objectUrl);
  }
}

/**
 * Fetches the latest frozen frame. Returns `null` when no trustworthy frame is
 * available; callers must not present the overlay in that case.
 */
async function loadFrameImage(): Promise<FrameImage | null> {
  try {
    const bytes = await getCurrentFrameBytes();
    if (bytes && bytes.byteLength > 0) {
      return await decodeFrame(bytes);
    }
  } catch (ipcErr) {
    console.warn("Direct IPC frame fetch not available, trying custom protocol:", ipcErr);
  }

  for (const src of [
    `http://carambola.localhost/frame/current?t=${Date.now()}`,
    `carambola://frame/current?t=${Date.now()}`,
  ]) {
    try {
      const img = new Image();
      img.src = src;
      await img.decode();
      return img;
    } catch (err) {
      console.warn(`Frame protocol failed (${src}):`, err);
    }
  }

  return null;
}

export const App: React.FC = () => {
  const [displayInfo, setDisplayInfo] = useState<DisplayInfo | null>(null);
  const [backgroundImage, setBackgroundImage] = useState<FrameImage | null>(null);
  // Incremented together with each new frame so the overlay resets its session
  // in the very same React commit that installs the fresh screenshot.
  const [captureToken, setCaptureToken] = useState<number>(0);
  // Guards against overlapping activations (e.g. a double shortcut press).
  const activationSeq = useRef<number>(0);

  useEffect(() => {
    getDisplayInfo().then(setDisplayInfo);
    loadFrameImage().then((img) => {
      if (img) setBackgroundImage(img);
    });

    // Listen for new screenshot capture events from Rust backend. The cleanup
    // below handles the case where the async registration resolves after an
    // effect teardown (React StrictMode / fast reload), preventing a duplicate
    // listener from surviving into the next mount.
    let disposed = false;
    let unlistenCapture: (() => void) | null = null;
    const unlistenPromise = listen<CaptureReadyPayload>("capture_triggered", async (event) => {
      const seq = ++activationSeq.current;
      const payload = event.payload;

      const image = await loadFrameImage();

      // A newer activation superseded this one while the frame was loading.
      if (seq !== activationSeq.current) return;
      if (!image) {
        console.error("No fresh screenshot available; keeping the overlay hidden");
        // Release the backend session lock so a later activation can retry.
        void hideOverlay();
        return;
      }

      // Reset the previous session and install the new frame in one synchronous
      // commit. All reset/redraw effects are layout effects, so the canvas is
      // already clean when the window becomes visible.
      flushSync(() => {
        setDisplayInfo(payload.display);
        setBackgroundImage(image);
        setCaptureToken((token) => token + 1);
      });

      if (seq !== activationSeq.current) return;

      try {
        await presentOverlay();
      } catch (err) {
        console.error("Failed to present overlay:", err);
        // Release the backend session lock so the app is not stuck.
        void hideOverlay();
      }
    });

    unlistenPromise
      .then((unlisten) => {
        if (disposed) {
          unlisten();
        } else {
          unlistenCapture = unlisten;
        }
      })
      .catch((err) => console.error("Failed to register capture listener:", err));

    return () => {
      disposed = true;
      unlistenCapture?.();
      unlistenCapture = null;
    };
  }, []);

  // The overlay canvas is laid out in CSS pixels, while the frozen screenshot
  // is stored in physical pixels. `devicePixelRatio` is the authoritative
  // CSS-to-device-pixel ratio for this webview and therefore the correct factor
  // for mapping a selection onto the screenshot, even when the capture backend
  // reports a different DPI scale.
  const scaleFactor = window.devicePixelRatio || displayInfo?.scaleFactor || 1;

  return (
    <div className="w-screen h-screen overflow-hidden select-none">
      <OverlayContainer
        backgroundImage={backgroundImage}
        scaleFactor={scaleFactor}
        captureToken={captureToken}
      />
    </div>
  );
};

export default App;
