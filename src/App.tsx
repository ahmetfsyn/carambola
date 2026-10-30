import React, { useState, useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { OverlayContainer } from "./components/overlay/OverlayContainer";
import { getDisplayInfo } from "./lib/ipc/commands";
import { DisplayInfo } from "./types/display";

export const App: React.FC = () => {
  const [displayInfo, setDisplayInfo] = useState<DisplayInfo | null>(null);
  const [backgroundImage, setBackgroundImage] = useState<ImageBitmap | HTMLImageElement | null>(null);

  const loadFrame = async () => {
    try {
      const img = new Image();
      // Cache-busting timestamp ensures we get the freshest screenshot on every hotkey press
      img.src = `carambola://frame/current?t=${Date.now()}`;
      await img.decode();
      setBackgroundImage(img);
    } catch (err) {
      console.warn("Could not load carambola:// frame protocol, trying fallback:", err);
      // Generate a subtle dark grid placeholder if testing outside Tauri
      const canvas = document.createElement("canvas");
      canvas.width = window.innerWidth;
      canvas.height = window.innerHeight;
      const ctx = canvas.getContext("2d");
      if (ctx) {
        ctx.fillStyle = "#1e293b";
        ctx.fillRect(0, 0, canvas.width, canvas.height);
        // Subtle grid
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
    }
  };

  useEffect(() => {
    getDisplayInfo().then(setDisplayInfo);
    loadFrame();

    // Listen for new screenshot capture events from Rust backend
    const unlistenPromise = listen("capture_triggered", () => {
      loadFrame();
    });

    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const scaleFactor = displayInfo?.scaleFactor || window.devicePixelRatio || 1;

  return (
    <div className="w-screen h-screen bg-transparent overflow-hidden select-none">
      <OverlayContainer
        backgroundImage={backgroundImage}
        scaleFactor={scaleFactor}
      />
    </div>
  );
};

export default App;

