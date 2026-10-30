import React from "react";
import { Rect } from "../../types/crop";

interface DimensionBadgeProps {
  crop: Rect;
  scaleFactor: number;
}

export const DimensionBadge: React.FC<DimensionBadgeProps> = ({ crop, scaleFactor }) => {
  if (crop.width <= 0 || crop.height <= 0) return null;

  // Position badge above top-left of crop if space allows, otherwise inside
  const top = crop.y > 28 ? crop.y - 26 : crop.y + 6;
  const left = Math.max(6, crop.x);

  const physicalW = Math.round(crop.width * scaleFactor);
  const physicalH = Math.round(crop.height * scaleFactor);

  return (
    <div
      style={{ top: `${top}px`, left: `${left}px` }}
      className="absolute pointer-events-none z-30 px-2 py-0.5 rounded text-xs font-mono font-medium text-white bg-slate-900/85 backdrop-blur-sm border border-slate-700 shadow-md select-none flex items-center gap-1.5"
    >
      <span className="text-carambola-cyan font-bold">{physicalW}</span>
      <span className="text-slate-400">×</span>
      <span className="text-carambola-cyan font-bold">{physicalH}</span>
      {scaleFactor > 1 && (
        <span className="text-[10px] text-slate-400">({scaleFactor}x)</span>
      )}
    </div>
  );
};

