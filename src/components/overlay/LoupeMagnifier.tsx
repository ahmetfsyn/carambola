import React, { useRef, useEffect } from "react";
import { Point } from "../../types/crop";

interface LoupeMagnifierProps {
  cursor: Point;
  visible: boolean;
  backgroundImage: ImageBitmap | HTMLImageElement | null;
  canvasWidth: number;
  canvasHeight: number;
}

export const LoupeMagnifier: React.FC<LoupeMagnifierProps> = ({
  cursor,
  visible,
  backgroundImage,
  canvasWidth,
  canvasHeight,
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [hexColor, setHexColor] = React.useState<string>("#000000");

  useEffect(() => {
    if (!visible || !backgroundImage || !canvasRef.current) return;

    const canvas = canvasRef.current;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const loupeSize = 100;
    const sampleRadius = 10;
    const zoom = 5;

    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, loupeSize, loupeSize);

    // Draw magnified center region
    const sx = cursor.x - sampleRadius;
    const sy = cursor.y - sampleRadius;
    const sWidth = sampleRadius * 2;
    const sHeight = sampleRadius * 2;

    ctx.drawImage(
      backgroundImage,
      sx,
      sy,
      sWidth,
      sHeight,
      0,
      0,
      loupeSize,
      loupeSize
    );

    // Sample center pixel color
    try {
      const centerPixel = ctx.getImageData(loupeSize / 2, loupeSize / 2, 1, 1).data;
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
    ctx.moveTo(0, loupeSize / 2);
    ctx.lineTo(loupeSize, loupeSize / 2);
    ctx.stroke();

    // Vertical crosshair
    ctx.beginPath();
    ctx.moveTo(loupeSize / 2, 0);
    ctx.lineTo(loupeSize / 2, loupeSize);
    ctx.stroke();

    // Center pixel highlight box
    ctx.strokeRect(loupeSize / 2 - zoom / 2, loupeSize / 2 - zoom / 2, zoom, zoom);
  }, [cursor, visible, backgroundImage]);

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
        width={100}
        height={100}
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
          {cursor.x}, {cursor.y}
        </span>
      </div>
    </div>
  );
};

