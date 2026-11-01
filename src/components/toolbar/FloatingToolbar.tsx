import React, { useLayoutEffect, useRef, useState } from "react";
import {
  MousePointer,
  ArrowUpRight,
  Square,
  Triangle,
  Circle,
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
import { ShapeMode, ToolType } from "../../types/annotations";

interface FloatingToolbarProps {
  crop: Rect;
  activeTool: ToolType;
  shapeMode: ShapeMode;
  onSelectTool: (tool: ToolType) => void;
  onCycleShape: () => void;
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

/** Lightweight MVP palette. Names are shown in the compact select box. */
const COLOR_OPTIONS = [
  { name: "Black", value: "#000000" },
  { name: "White", value: "#FFFFFF" },
  { name: "Red", value: "#EF4444" },
  { name: "Orange", value: "#F97316" },
  { name: "Yellow", value: "#FBBF24" },
  { name: "Green", value: "#10B981" },
  { name: "Blue", value: "#3B82F6" },
  { name: "Purple", value: "#8B5CF6" },
];

const STROKE_WIDTHS = Array.from({ length: 20 }, (_, i) => i + 1);
const SHAPE_ORDER: ShapeMode[] = ["rect", "triangle", "circle"];
const SHAPE_LABEL: Record<ShapeMode, string> = {
  rect: "Rectangle",
  triangle: "Triangle",
  circle: "Circle",
};

export function nextShapeMode(current: ShapeMode): ShapeMode {
  const index = SHAPE_ORDER.indexOf(current);
  return SHAPE_ORDER[(index + 1) % SHAPE_ORDER.length];
}

const TOOLBAR_MARGIN = 8;
const VIEWPORT_PAD = 12;
const DEFAULT_TOOLBAR_SIZE = { width: 560, height: 44 };

export interface ToolbarSize {
  width: number;
  height: number;
}

/**
 * Anchors the toolbox to the LEFT edge of the selection and prefers the space
 * immediately below it. Falls back to above, then clamps inside the viewport so
 * the toolbar is always fully visible — regardless of selection width, screen
 * edge proximity or DPI scale (all values are CSS pixels).
 */
export function computeToolbarPosition(
  crop: Rect,
  size: ToolbarSize,
  windowWidth: number,
  windowHeight: number
): { left: number; top: number } {
  const width = size.width;
  const height = size.height;

  let left = crop.x;
  left = Math.max(VIEWPORT_PAD, Math.min(left, windowWidth - width - VIEWPORT_PAD));

  let top = crop.y + crop.height + TOOLBAR_MARGIN;
  if (top + height > windowHeight - VIEWPORT_PAD) {
    const above = crop.y - height - TOOLBAR_MARGIN;
    top =
      above >= VIEWPORT_PAD
        ? above
        : Math.max(VIEWPORT_PAD, windowHeight - height - VIEWPORT_PAD);
  }
  top = Math.max(VIEWPORT_PAD, Math.min(top, windowHeight - height - VIEWPORT_PAD));

  return { left, top };
}

export const FloatingToolbar: React.FC<FloatingToolbarProps> = ({
  crop,
  activeTool,
  shapeMode,
  onSelectTool,
  onCycleShape,
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
  const toolbarRef = useRef<HTMLDivElement | null>(null);
  const [toolbarSize, setToolbarSize] = useState(DEFAULT_TOOLBAR_SIZE);

  // Measure the real toolbar so clamping never relies on a guessed width; the
  // select boxes can change the width slightly between platforms.
  useLayoutEffect(() => {
    const el = toolbarRef.current;
    if (!el) return;
    const width = el.offsetWidth;
    const height = el.offsetHeight;
    if (width !== toolbarSize.width || height !== toolbarSize.height) {
      setToolbarSize({ width, height });
    }
  }, [
    toolbarSize.width,
    toolbarSize.height,
    crop.x,
    crop.y,
    crop.width,
    crop.height,
    activeTool,
    shapeMode,
    activeColor,
    activeStrokeWidth,
    windowWidth,
    windowHeight,
  ]);

  if (crop.width <= 0 || crop.height <= 0) return null;

  const width = toolbarSize.width;
  const height = toolbarSize.height;
  const { left, top } = computeToolbarPosition(
    crop,
    { width, height },
    windowWidth,
    windowHeight
  );

  const shapeIcon =
    shapeMode === "triangle" ? (
      <Triangle size={16} />
    ) : shapeMode === "circle" ? (
      <Circle size={16} />
    ) : (
      <Square size={16} />
    );

  return (
    <div
      ref={toolbarRef}
      data-carambola-toolbar
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
          title={`Shape: ${SHAPE_LABEL[shapeMode]} (click to cycle)`}
          aria-label={`Shape tool: ${SHAPE_LABEL[shapeMode]}. Click to cycle to the next shape.`}
          onClick={() => {
            if (activeTool === "rect") {
              onCycleShape();
            } else {
              onSelectTool("rect");
            }
          }}
          className={`p-1.5 rounded-md transition-colors ${
            activeTool === "rect"
              ? "bg-carambola-cyan text-slate-950 font-bold"
              : "hover:bg-slate-700 text-slate-300"
          }`}
        >
          {shapeIcon}
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

      {/* Color & stroke width (visible when a vector tool is active) */}
      {activeTool !== "select" && activeTool !== "mosaic" && (
        <div className="flex items-center gap-1.5 px-1 border-r border-slate-700/60">
          <div className="flex items-center gap-1">
            <span
              className="w-3.5 h-3.5 rounded-full border border-slate-500"
              style={{ backgroundColor: activeColor }}
              aria-hidden="true"
            />
            <select
              aria-label="Annotation color"
              title="Annotation color"
              value={activeColor}
              onChange={(e) => onSelectColor(e.target.value)}
              className="h-7 rounded-md bg-slate-800 border border-slate-700 text-[11px] px-1 text-slate-200 outline-none focus:border-carambola-cyan"
            >
              {COLOR_OPTIONS.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.name}
                </option>
              ))}
            </select>
          </div>

          <select
            aria-label="Stroke width in pixels"
            title="Stroke width (px)"
            value={activeStrokeWidth}
            onChange={(e) => onSelectStrokeWidth(Number(e.target.value))}
            className="h-7 w-[64px] rounded-md bg-slate-800 border border-slate-700 text-[11px] px-1 text-slate-200 outline-none focus:border-carambola-cyan"
          >
            {STROKE_WIDTHS.map((w) => (
              <option key={w} value={w}>
                {w} px
              </option>
            ))}
          </select>
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