import React from "react";
import {
  MousePointer,
  ArrowUpRight,
  Square,
  Pencil,
  Type,
  Grid,
  Undo2,
  Redo2,
  Copy,
  Download,
  ScanText,
  X,
  Check,
  Settings,
} from "lucide-react";
import { Rect } from "../../types/crop";
import { ToolType } from "../../types/annotations";

interface FloatingToolbarProps {
  crop: Rect;
  activeTool: ToolType;
  onSelectTool: (tool: ToolType) => void;
  activeColor: string;
  onSelectColor: (color: string) => void;
  activeStrokeWidth: number;
  onSelectStrokeWidth: (w: number) => void;
  canUndo: boolean;
  canRedo: boolean;
  onUndo: () => void;
  onRedo: () => void;
  onCopy: () => void;
  onSave: () => void;
  onOcr: () => void;
  onOpenSettings: () => void;
  onClose: () => void;
  copiedFeedback: boolean;
  ocrLoading: boolean;
  windowWidth: number;
  windowHeight: number;
}

const COLORS = [
  "#EF4444", // Red
  "#10B981", // Green
  "#06B6D4", // Cyan
  "#FBBF24", // Yellow
  "#FFFFFF", // White
  "#0F172A", // Dark Slate / Black
];

const STROKE_WIDTHS = [
  { label: "S", width: 2 },
  { label: "M", width: 4 },
  { label: "L", width: 8 },
];

export const FloatingToolbar: React.FC<FloatingToolbarProps> = ({
  crop,
  activeTool,
  onSelectTool,
  activeColor,
  onSelectColor,
  activeStrokeWidth,
  onSelectStrokeWidth,
  canUndo,
  canRedo,
  onUndo,
  onRedo,
  onCopy,
  onSave,
  onOcr,
  onOpenSettings,
  onClose,
  copiedFeedback,
  ocrLoading,
  windowWidth,
  windowHeight,
}) => {
  if (crop.width <= 0 || crop.height <= 0) return null;

  // Calculate toolbar position: prefer below the crop box
  const toolbarHeight = 44;
  const margin = 8;
  let top = crop.y + crop.height + margin;
  let left = crop.x + crop.width - 480; // Align to bottom right of selection

  // If outside bottom edge, place above the crop box
  if (top + toolbarHeight > windowHeight - 12) {
    top = Math.max(12, crop.y - toolbarHeight - margin);
  }

  // Clamp within horizontal screen bounds
  left = Math.max(12, Math.min(windowWidth - 490, left));

  return (
    <div
      style={{ top: `${top}px`, left: `${left}px` }}
      className="absolute z-50 flex items-center gap-1.5 p-1.5 rounded-xl bg-slate-900/95 backdrop-blur-md border border-slate-700/80 shadow-2xl text-slate-200 toolbar-fade-in"
      onClick={(e) => e.stopPropagation()}
    >
      {/* Tool Selection Group */}
      <div className="flex items-center gap-0.5 bg-slate-800/80 p-0.5 rounded-lg border border-slate-700/50">
        <button
          title="Select / Move Crop"
          onClick={() => onSelectTool("select")}
          className={`p-1.5 rounded-md transition-colors ${
            activeTool === "select"
              ? "bg-carambola-cyan text-slate-950 font-bold"
              : "hover:bg-slate-700 text-slate-300"
          }`}
        >
          <MousePointer size={16} />
        </button>

        <button
          title="Arrow Tool"
          onClick={() => onSelectTool("arrow")}
          className={`p-1.5 rounded-md transition-colors ${
            activeTool === "arrow"
              ? "bg-carambola-cyan text-slate-950 font-bold"
              : "hover:bg-slate-700 text-slate-300"
          }`}
        >
          <ArrowUpRight size={16} />
        </button>

        <button
          title="Rectangle Tool"
          onClick={() => onSelectTool("rect")}
          className={`p-1.5 rounded-md transition-colors ${
            activeTool === "rect"
              ? "bg-carambola-cyan text-slate-950 font-bold"
              : "hover:bg-slate-700 text-slate-300"
          }`}
        >
          <Square size={16} />
        </button>

        <button
          title="Pen / Freehand"
          onClick={() => onSelectTool("pen")}
          className={`p-1.5 rounded-md transition-colors ${
            activeTool === "pen"
              ? "bg-carambola-cyan text-slate-950 font-bold"
              : "hover:bg-slate-700 text-slate-300"
          }`}
        >
          <Pencil size={16} />
        </button>

        <button
          title="Text Annotation"
          onClick={() => onSelectTool("text")}
          className={`p-1.5 rounded-md transition-colors ${
            activeTool === "text"
              ? "bg-carambola-cyan text-slate-950 font-bold"
              : "hover:bg-slate-700 text-slate-300"
          }`}
        >
          <Type size={16} />
        </button>

        <button
          title="Blur / Mosaic Redaction"
          onClick={() => onSelectTool("mosaic")}
          className={`p-1.5 rounded-md transition-colors ${
            activeTool === "mosaic"
              ? "bg-carambola-cyan text-slate-950 font-bold"
              : "hover:bg-slate-700 text-slate-300"
          }`}
        >
          <Grid size={16} />
        </button>
      </div>

      {/* Colors & Stroke Width (visible when vector tool is active) */}
      {activeTool !== "select" && activeTool !== "mosaic" && (
        <div className="flex items-center gap-1.5 px-1 border-r border-slate-700/60">
          <div className="flex items-center gap-1">
            {COLORS.map((c) => (
              <button
                key={c}
                onClick={() => onSelectColor(c)}
                className={`w-4 h-4 rounded-full border transition-transform ${
                  activeColor === c
                    ? "scale-125 border-white shadow-sm ring-1 ring-carambola-cyan"
                    : "border-slate-600 hover:scale-110"
                }`}
                style={{ backgroundColor: c }}
              />
            ))}
          </div>

          <div className="flex items-center gap-0.5 bg-slate-800/60 rounded p-0.5 text-[10px] font-mono">
            {STROKE_WIDTHS.map((sw) => (
              <button
                key={sw.width}
                onClick={() => onSelectStrokeWidth(sw.width)}
                className={`px-1.5 py-0.5 rounded ${
                  activeStrokeWidth === sw.width
                    ? "bg-slate-700 text-carambola-yellow font-bold"
                    : "text-slate-400 hover:text-white"
                }`}
              >
                {sw.label}
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Undo / Redo */}
      <div className="flex items-center gap-0.5 px-0.5 border-r border-slate-700/60">
        <button
          title="Undo (Ctrl+Z / Cmd+Z)"
          disabled={!canUndo}
          onClick={onUndo}
          className="p-1.5 rounded-md hover:bg-slate-800 disabled:opacity-30 disabled:hover:bg-transparent text-slate-300"
        >
          <Undo2 size={15} />
        </button>
        <button
          title="Redo (Ctrl+Y / Cmd+Shift+Z)"
          disabled={!canRedo}
          onClick={onRedo}
          className="p-1.5 rounded-md hover:bg-slate-800 disabled:opacity-30 disabled:hover:bg-transparent text-slate-300"
        >
          <Redo2 size={15} />
        </button>
      </div>

      {/* Action Buttons */}
      <div className="flex items-center gap-1">
        <button
          title="OCR Text Recognition"
          onClick={onOcr}
          disabled={ocrLoading}
          className="p-1.5 rounded-md bg-slate-800 hover:bg-slate-700 text-carambola-yellow transition-colors flex items-center gap-1 text-xs font-medium px-2"
        >
          <ScanText size={15} className={ocrLoading ? "animate-pulse" : ""} />
          <span>OCR</span>
        </button>

        <button
          title="Save Screenshot to Disk"
          onClick={onSave}
          className="p-1.5 rounded-md bg-slate-800 hover:bg-slate-700 text-slate-200 transition-colors"
        >
          <Download size={15} />
        </button>

        <button
          title="Copy to Clipboard"
          onClick={onCopy}
          className="p-1.5 rounded-md bg-carambola-cyan hover:bg-cyan-400 text-slate-950 font-semibold transition-colors flex items-center gap-1 text-xs px-2.5 shadow-sm"
        >
          {copiedFeedback ? (
            <>
              <Check size={14} className="stroke-[3]" />
              <span>Copied!</span>
            </>
          ) : (
            <>
              <Copy size={14} />
              <span>Copy</span>
            </>
          )}
        </button>

        <button
          title="Change Shortcut / Settings"
          onClick={onOpenSettings}
          className="p-1.5 rounded-md hover:bg-slate-800 text-slate-400 hover:text-carambola-yellow transition-colors"
        >
          <Settings size={15} />
        </button>

        <button
          title="Cancel / Dismiss (Esc)"
          onClick={onClose}
          className="p-1.5 rounded-md hover:bg-red-500/20 text-slate-400 hover:text-red-400 transition-colors ml-0.5"
        >
          <X size={15} />
        </button>
      </div>
    </div>
  );
};

