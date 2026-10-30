import React, { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { OverlayContainer } from "./components/overlay/OverlayContainer";
import { getDisplayInfo, getCurrentFrameBytes, logDiagnostics } from "./lib/ipc/commands";
import { DisplayInfo } from "./types/display";

export const App: React.FC = () => {
  const [displayInfo, setDisplayInfo] = useState<DisplayInfo | null>(null);
  const [backgroundImage, setBackgroundImage] = useState<ImageBitmap | HTMLImageElement | null>(null);
  // Incremented on every capture so the overlay can reset its selection and
  // re-measure the (possibly resized/repositioned) window.
  const [captureToken, setCaptureToken] = useState<number>(0);

  const loadFrame = async () => {
    // 1. Try direct high-speed binary IPC transfer (100% reliable across all platforms/modes)
    try {
      const pngBytes = await getCurrentFrameBytes();
      if (pngBytes && pngBytes.length > 0) {
        const blob = new Blob([pngBytes.buffer as ArrayBuffer], { type: "image/png" });
        const objectUrl = URL.createObjectURL(blob);
        const img = new Image();
        img.src = objectUrl;
        await img.decode();
        setBackgroundImage(img);
        return;
      }
    } catch (ipcErr) {
      console.warn("Direct IPC frame fetch not available, trying custom protocol:", ipcErr);
    }

    // 2. Fallback to custom protocol
    try {
      const img = new Image();
      // Cache-busting timestamp ensures fresh screenshot
      img.src = `http://carambola.localhost/frame/current?t=${Date.now()}`;
      await img.decode();
      setBackgroundImage(img);
      return;
    } catch (protoErr) {
      console.warn("carambola.localhost protocol failed, trying carambola:// scheme:", protoErr);
      try {
        const img = new Image();
        img.src = `carambola://frame/current?t=${Date.now()}`;
        await img.decode();
        setBackgroundImage(img);
        return;
      } catch (err) {
        console.warn("Could not load frame protocol, generating fallback:", err);
      }
    }

    // 3. Fallback when testing in isolated browser environment
    const canvas = document.createElement("canvas");
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    const ctx = canvas.getContext("2d");
    if (ctx) {
      ctx.fillStyle = "#1e293b";
      ctx.fillRect(0, 0, canvas.width, canvas.height);
      ctx.strokeStyle = "#334155";
      ctx.lineWidth = 1;
      for (let x = 0; x < canvas.width; x += 40) {
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, canvas.height);
        ctx.stroke();
      }
      for (let y = 0; y < canvas.height; y += 40) {
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(canvas.width, y);
        ctx.stroke();
      }
      const fallbackImg = new Image();
      fallbackImg.src = canvas.toDataURL();
      await fallbackImg.decode();
      setBackgroundImage(fallbackImg);
    }
  };

  useEffect(() => {
    getDisplayInfo().then(setDisplayInfo);
    loadFrame();

    // Listen for new screenshot capture events from Rust backend
    const unlistenPromise = listen("capture_triggered", () => {
      // The overlay may have been moved to another monitor or DPI context:
      // refresh the display metadata and bump the capture token so the overlay
      // resets its selection and re-measures the window.
      getDisplayInfo().then(setDisplayInfo);
      setCaptureToken((token) => token + 1);
      loadFrame();
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
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
