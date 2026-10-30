import React, { useState, useEffect, useRef } from "react";
import { flushSync } from "react-dom";
import { listen } from "@tauri-apps/api/event";
import { OverlayContainer } from "./components/overlay/OverlayContainer";
import {
  getDisplayInfo,
  getCurrentFrameBytes,
  presentOverlay,
  logDiagnostics,
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

interface FrameLoadTiming {
  ipcMs?: number;
  decodeMs?: number;
}

/**
 * Fetches the latest frozen frame. Returns `null` when no trustworthy frame is
 * available; callers must not present the overlay in that case.
 */
async function loadFrameImage(timing?: FrameLoadTiming): Promise<FrameImage | null> {
  try {
    const ipcStarted = performance.now();
    const bytes = await getCurrentFrameBytes();
    if (timing) timing.ipcMs = performance.now() - ipcStarted;
    if (bytes && bytes.byteLength > 0) {
      const decodeStarted = performance.now();
      const image = await decodeFrame(bytes);
      if (timing) timing.decodeMs = performance.now() - decodeStarted;
      return image;
    }
    logDiagnostics("frame_load", { stage: "empty_payload" });
  } catch (ipcErr) {
    logDiagnostics("frame_load", { stage: "ipc_failed", error: String(ipcErr) });
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
      logDiagnostics("frame_load", { stage: "protocol_ok", src });
      return img;
    } catch (err) {
      logDiagnostics("frame_load", { stage: "protocol_failed", src, error: String(err) });
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

    // Listen for new screenshot capture events from Rust backend.
    const unlistenPromise = listen<CaptureReadyPayload>("capture_triggered", async (event) => {
      const seq = ++activationSeq.current;
      const payload = event.payload;
      const receivedAt = Date.now();
      const timelineStart = performance.now();

      logDiagnostics("perf", {
        stage: "event_received",
        receivedAfterTriggerMs: receivedAt - payload.triggeredAt,
        captureMs: payload.captureMs,
        encodeMs: payload.encodeMs,
        pngBytes: payload.pngBytes,
        display: payload.display.name,
      });

      const frameTiming: FrameLoadTiming = {};
      const image = await loadFrameImage(frameTiming);
      const fetchDecodeMs = performance.now() - timelineStart;

      // A newer activation superseded this one while the frame was loading.
      if (seq !== activationSeq.current) return;
      if (!image) {
        console.error("No fresh screenshot available; keeping the overlay hidden");
        logDiagnostics("perf", { stage: "aborted", reason: "no_frame", fetchDecodeMs });
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
      const commitMs = performance.now() - timelineStart;

      if (seq !== activationSeq.current) return;

      try {
        await presentOverlay();
      } catch (err) {
        console.error("Failed to present overlay:", err);
        logDiagnostics("perf", { stage: "aborted", reason: "present_failed" });
        return;
      }

      const presentedMs = performance.now() - timelineStart;
      logDiagnostics("perf", {
        stage: "presented",
        ipcMs: frameTiming.ipcMs,
        decodeMs: frameTiming.decodeMs,
        fetchDecodeMs,
        commitMs,
        presentedMs,
        totalAfterTriggerMs: Date.now() - payload.triggeredAt,
      });

      requestAnimationFrame(() => {
        logDiagnostics("perf", {
          stage: "first_frame",
          firstFrameMs: performance.now() - timelineStart,
          totalAfterTriggerMs: Date.now() - payload.triggeredAt,
        });
      });
    });

    unlistenPromise.catch((err) =>
      logDiagnostics("listener", { stage: "error", error: String(err) })
    );

    return () => {
      unlistenPromise.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  // The overlay canvas is laid out in CSS pixels, while the frozen screenshot
  // is stored in physical pixels. `devicePixelRatio` is the authoritative
  // CSS-to-device-pixel ratio for this webview and therefore the correct factor
  // for mapping a selection onto the screenshot, even when the capture backend
  // reports a different DPI scale.
  const scaleFactor = window.devicePixelRatio || displayInfo?.scaleFactor || 1;

  // Temporary pipeline diagnostics (frontend view of the display).
  useEffect(() => {
    if (!displayInfo) return;
    logDiagnostics("display", {
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      devicePixelRatio: window.devicePixelRatio,
      screenWidth: window.screen.width,
      screenHeight: window.screen.height,
      availWidth: window.screen.availWidth,
      availHeight: window.screen.availHeight,
      physicalWidth: displayInfo.physicalWidth,
      physicalHeight: displayInfo.physicalHeight,
      reportedScaleFactor: displayInfo.scaleFactor,
      bounds: displayInfo.bounds,
      isPrimary: displayInfo.isPrimary,
    });
  }, [displayInfo]);

  return (
    <div className="w-screen h-screen bg-black overflow-hidden select-none">
      <OverlayContainer
        backgroundImage={backgroundImage}
        scaleFactor={scaleFactor}
        captureToken={captureToken}
      />
    </div>
  );
};

export default App;