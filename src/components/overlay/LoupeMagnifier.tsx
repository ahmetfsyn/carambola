import React, { useRef, useEffect } from "react";
import { Point } from "../../types/crop";

interface LoupeMagnifierProps {
  cursor: Point;
  visible: boolean;
  backgroundImage: ImageBitmap | HTMLImageElement | null;
  canvasWidth: number;
  canvasHeight: number;
}

const LOUPE_SIZE = 100;
const SAMPLE_RADIUS = 10; // CSS pixels sampled around the cursor

export const LoupeMagnifier: React.FC<LoupeMagnifierProps> = ({
  cursor,
  visible,
  backgroundImage,
  canvasWidth,
  canvasHeight,
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [hexColor, setHexColor] = React.useState<string>("#000000");

  const dpr = window.devicePixelRatio || 1;

  // The cursor is reported in CSS pixels, but the screenshot is stored in
  // natural (physical) pixels. `imageScale` converts between the two so the
  // loupe always shows the exact pixel under the pointer.
  const imageScale =
    backgroundImage &&
    "naturalWidth" in backgroundImage &&
    backgroundImage.naturalWidth &&
    canvasWidth
      ? backgroundImage.naturalWidth / canvasWidth
      : 1;

  // Screenshot-pixel position under the cursor (what the user is actually
  // hovering in the original capture).
  const screenshotX = Math.round(cursor.x * imageScale);
  const screenshotY = Math.round(cursor.y * imageScale);

  useEffect(() => {
    if (!visible || !backgroundImage || !canvasRef.current) return;

    const canvas = canvasRef.current;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Work in CSS-pixel loupe units; the backing store is `LOUPE_SIZE * dpr`.
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, LOUPE_SIZE, LOUPE_SIZE);

    // Source rectangle in screenshot pixels corresponding to the CSS-pixel
    // sample window around the cursor.
    const srcRadius = SAMPLE_RADIUS * imageScale;
    const sx = cursor.x * imageScale - srcRadius;
    const sy = cursor.y * imageScale - srcRadius;
    const sWidth = srcRadius * 2;
    const sHeight = srcRadius * 2;

    // Draw magnified center region
    ctx.drawImage(
      backgroundImage,
      sx,
      sy,
      sWidth,
      sHeight,
      0,
      0,
      LOUPE_SIZE,
      LOUPE_SIZE
    );

    // Sample center pixel color (backing-store pixels).
    try {
      const center = Math.floor((LOUPE_SIZE * dpr) / 2);
      const centerPixel = ctx.getImageData(center, center, 1, 1).data;
      const r = centerPixel[0].toString(16).padStart(2, "0");
      const g = centerPixel[1].toString(16).padStart(2, "0");
      const b = centerPixel[2].toString(16).padStart(2, "0");
      setHexColor(`#${r}${g}${b}`.toUpperCase());
    } catch {
      // Ignored if outside boundary
    }

    // Draw crosshair
    ctx.strokeStyle = "rgba(6, 182, 212, 0.85)"; // Cyan crosshair
    ctx.lineWidth = 1;

    // Horizontal crosshair
    ctx.beginPath();
    ctx.moveTo(0, LOUPE_SIZE / 2);
    ctx.lineTo(LOUPE_SIZE, LOUPE_SIZE / 2);
    ctx.stroke();

    // Vertical crosshair
    ctx.beginPath();
    ctx.moveTo(LOUPE_SIZE / 2, 0);
    ctx.lineTo(LOUPE_SIZE / 2, LOUPE_SIZE);
    ctx.stroke();

    // Highlight a single screenshot pixel at the center of the loupe.
    const pixelZoom = LOUPE_SIZE / sWidth; // loupe px per screenshot px
    ctx.strokeRect(
      LOUPE_SIZE / 2 - pixelZoom / 2,
      LOUPE_SIZE / 2 - pixelZoom / 2,
      pixelZoom,
      pixelZoom
    );
  }, [cursor, visible, backgroundImage, imageScale, dpr]);

  if (!visible) return null;

  // Offset loupe away from cursor so it doesn't block view
  const offset = 24;
  const size = 110;
  let posX = cursor.x + offset;
  let posY = cursor.y + offset;

  if (posX + size > canvasWidth) posX = cursor.x - size - offset;
  if (posY + size > canvasHeight) posY = cursor.y - size - offset;

  return (
    <div
      style={{
        left: `${posX}px`,
        top: `${posY}px`,
      }}
      className="absolute pointer-events-none z-40 rounded-xl overflow-hidden bg-slate-900/90 border-2 border-carambola-cyan/80 shadow-2xl flex flex-col items-center p-1.5"
    >
      <canvas
        ref={canvasRef}
        width={LOUPE_SIZE * dpr}
        height={LOUPE_SIZE * dpr}
        style={{ width: `${LOUPE_SIZE}px`, height: `${LOUPE_SIZE}px` }}
        className="rounded-lg border border-slate-700/60"
      />
      <div className="mt-1 flex items-center justify-between w-full px-1 text-[11px] font-mono text-slate-300">
        <span className="flex items-center gap-1">
          <span
            className="w-2.5 h-2.5 rounded-full border border-slate-600 inline-block"
            style={{ backgroundColor: hexColor }}
          />
          <span className="font-semibold text-white">{hexColor}</span>
        </span>
        <span className="text-[10px] text-slate-400">
          {screenshotX}, {screenshotY}
        </span>
      </div>
    </div>
  );
};
