import React, { useRef, useState, useEffect, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { Settings } from "lucide-react";
import { Point, Rect, DragState, HandleType } from "../../types/crop";
import { ToolType, AnnotationElement } from "../../types/annotations";
import {
  pointInRect,
  normalizeRect,
  hitTestHandle,
  resizeRectWithHandle,
} from "../../lib/canvas/geometry";
import { renderCanvas } from "../../lib/canvas/renderLayers";
import { FloatingToolbar } from "../toolbar/FloatingToolbar";
import { DimensionBadge } from "./DimensionBadge";
import { LoupeMagnifier } from "./LoupeMagnifier";
import { ShortcutModal } from "../settings/ShortcutModal";
import {
  copyCropToClipboard,
  saveCropToDisk,
  hideOverlay,
  extractOcrText,
  logDiagnostics,
} from "../../lib/ipc/commands";

interface OverlayContainerProps {
  backgroundImage: ImageBitmap | HTMLImageElement | null;
  scaleFactor: number;
  captureToken: number;
}

export const OverlayContainer: React.FC<OverlayContainerProps> = ({
  backgroundImage,
  scaleFactor,
  captureToken,
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  // Device pixel ratio for this webview. The canvas backing store is sized in
  // physical pixels (`css * dpr`) so the screenshot is rendered crisply at the
  // display's native resolution, while all interaction coordinates stay in CSS
  // pixels (the canvas context is scaled by `dpr` in `renderCanvas`).
  const dpr = window.devicePixelRatio || 1;

  // Crop & interaction state
  const [crop, setCrop] = useState<Rect | null>(null);
  const [cursorPos, setCursorPos] = useState<Point>({ x: 0, y: 0 });
  const [dragState, setDragState] = useState<DragState>({
    mode: "none",
    startPoint: { x: 0, y: 0 },
    initialRect: null,
    activeHandle: null,
  });

  // Annotation tools & history state
  const [activeTool, setActiveTool] = useState<ToolType>("select");
  const [activeColor, setActiveColor] = useState<string>("#EF4444");
  const [activeStrokeWidth, setActiveStrokeWidth] = useState<number>(4);
  const [elements, setElements] = useState<AnnotationElement[]>([]);
  const [undoStack, setUndoStack] = useState<AnnotationElement[][]>([]);
  const [redoStack, setRedoStack] = useState<AnnotationElement[][]>([]);
  const [currentDrawing, setCurrentDrawing] = useState<AnnotationElement | null>(null);

  // In-place text editing
  const [editingText, setEditingText] = useState<{ position: Point; text: string } | null>(null);

  // Feedback states
  const [copiedFeedback, setCopiedFeedback] = useState<boolean>(false);
  const [ocrLoading, setOcrLoading] = useState<boolean>(false);
  const [toastMessage, setToastMessage] = useState<string | null>(null);
  const [isSettingsOpen, setIsSettingsOpen] = useState<boolean>(false);

  const [windowSize, setWindowSize] = useState<{ width: number; height: number }>({
    width: window.innerWidth,
    height: window.innerHeight,
  });

  // Track window resize
  useEffect(() => {
    const handleResize = () => {
      setWindowSize({ width: window.innerWidth, height: window.innerHeight });
    };
    window.addEventListener("resize", handleResize);

    const unlistenPromise = listen("open_shortcut_settings", () => {
      setIsSettingsOpen(true);
    });

    return () => {
      window.removeEventListener("resize", handleResize);
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  // Every new capture resets the selection/annotations and re-measures the
  // window so the canvas always matches the (possibly relocated) overlay.
  useEffect(() => {
    setWindowSize({ width: window.innerWidth, height: window.innerHeight });
    setCrop(null);
    setElements([]);
    setUndoStack([]);
    setRedoStack([]);
    setCurrentDrawing(null);
    setEditingText(null);
    setDragState({
      mode: "none",
      startPoint: { x: 0, y: 0 },
      initialRect: null,
      activeHandle: null,
    });
  }, [captureToken]);

  // Main canvas render effect
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    renderCanvas({
      ctx,
      canvasWidth: windowSize.width,
      canvasHeight: windowSize.height,
      dpr,
      backgroundImage,
      cropRect: crop,
      elements,
      activeElement: currentDrawing,
      isHovered: false,
    });
  }, [windowSize, backgroundImage, crop, elements, currentDrawing, dpr]);

  // Temporary pipeline diagnostics (canvas / viewport / image relationship).
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const img = backgroundImage as HTMLImageElement | null;
    logDiagnostics("canvas", {
      innerWidth: window.innerWidth,
      innerHeight: window.innerHeight,
      devicePixelRatio: window.devicePixelRatio,
      canvasBackingWidth: canvas.width,
      canvasBackingHeight: canvas.height,
      canvasCssWidth: canvas.clientWidth,
      canvasCssHeight: canvas.clientHeight,
      imageNaturalWidth: img && "naturalWidth" in img ? img.naturalWidth : null,
      imageNaturalHeight: img && "naturalHeight" in img ? img.naturalHeight : null,
      imageScale:
        img && "naturalWidth" in img && img.naturalWidth && canvas.clientWidth
          ? img.naturalWidth / canvas.clientWidth
          : null,
      renderedWidth: windowSize.width,
      renderedHeight: windowSize.height,
    });
  }, [backgroundImage, windowSize, dpr]);

  // Push element to history
  const pushElement = useCallback((element: AnnotationElement) => {
    setUndoStack((prev) => [...prev, elements]);
    setRedoStack([]);
    setElements((prev) => [...prev, element]);
  }, [elements]);

  const handleUndo = useCallback(() => {
    if (undoStack.length === 0) return;
    const previous = undoStack[undoStack.length - 1];
    setRedoStack((prev) => [...prev, elements]);
    setUndoStack((prev) => prev.slice(0, -1));
    setElements(previous);
  }, [elements, undoStack]);

  const handleRedo = useCallback(() => {
    if (redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    setUndoStack((prev) => [...prev, elements]);
    setRedoStack((prev) => prev.slice(0, -1));
    setElements(next);
  }, [elements, redoStack]);

  // Output handlers
  const handleCopy = useCallback(async () => {
    if (!crop || crop.width <= 0 || crop.height <= 0) return;
    try {
      await copyCropToClipboard(crop, elements, scaleFactor);
      setCopiedFeedback(true);
      setTimeout(() => {
        setCopiedFeedback(false);
        hideOverlay();
      }, 350);
    } catch (err) {
      console.error("Failed to copy crop:", err);
    }
  }, [crop, elements, scaleFactor]);

  const handleSave = useCallback(async () => {
    if (!crop || crop.width <= 0 || crop.height <= 0) return;
    try {
      const savedPath = await saveCropToDisk(crop, elements, scaleFactor);
      setToastMessage(`Saved to ${savedPath}`);
      setTimeout(() => {
        setToastMessage(null);
        hideOverlay();
      }, 800);
    } catch (err) {
      console.error("Failed to save crop:", err);
    }
  }, [crop, elements, scaleFactor]);

  const handleOcr = useCallback(async () => {
    if (!crop || crop.width <= 0 || crop.height <= 0) return;
    setOcrLoading(true);
    try {
      const text = await extractOcrText(crop, scaleFactor);
      setOcrLoading(false);
      setToastMessage(`OCR Extracted (${text.length} chars)`);
      setTimeout(() => setToastMessage(null), 2500);
    } catch (err) {
      setOcrLoading(false);
      console.error("Failed to run OCR:", err);
    }
  }, [crop, scaleFactor]);

  const handleClose = useCallback(() => {
    hideOverlay();
  }, []);

  // Global overlay key shortcuts (Escape, Ctrl+Z, Enter, etc.)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (editingText) return; // Allow normal typing in text editor

      if (e.key === "Escape") {
        e.preventDefault();
        handleClose();
      } else if (e.key === "Enter") {
        e.preventDefault();
        handleCopy();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) {
          handleRedo();
        } else {
          handleUndo();
        }
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "y") {
        e.preventDefault();
        handleRedo();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "c") {
        e.preventDefault();
        handleCopy();
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
        e.preventDefault();
        handleSave();
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [editingText, handleClose, handleCopy, handleSave, handleUndo, handleRedo]);

  // Mouse event handlers
  const handleMouseDown = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const pt: Point = { x: e.clientX, y: e.clientY };

    // Commit any active in-place text
    if (editingText && editingText.text.trim()) {
      pushElement({
        id: `text-${Date.now()}`,
        type: "text",
        position: editingText.position,
        text: editingText.text,
        color: activeColor,
        fontSize: activeStrokeWidth * 6,
      });
      setEditingText(null);
      return;
    }

    if (crop) {
      // 1. Check if clicking on an existing resize handle
      const hitHandle = hitTestHandle(pt, crop);
      if (hitHandle) {
        setDragState({
          mode: "resizing",
          startPoint: pt,
          initialRect: crop,
          activeHandle: hitHandle,
        });
        return;
      }

      // 2. If clicking inside crop with selection tool -> move crop
      if (activeTool === "select" && pointInRect(pt, crop)) {
        setDragState({
          mode: "moving",
          startPoint: pt,
          initialRect: crop,
          activeHandle: null,
        });
        return;
      }

      // 3. If clicking inside crop with a vector tool -> start drawing
      if (activeTool !== "select" && pointInRect(pt, crop)) {
        if (activeTool === "text") {
          setEditingText({ position: pt, text: "" });
          return;
        }

        const id = `${activeTool}-${Date.now()}`;
        if (activeTool === "arrow") {
          setCurrentDrawing({
            id,
            type: "arrow",
            start: pt,
            end: pt,
            color: activeColor,
            strokeWidth: activeStrokeWidth,
          });
        } else if (activeTool === "rect") {
          setCurrentDrawing({
            id,
            type: "rect",
            bounds: { x: pt.x, y: pt.y, width: 0, height: 0 },
            color: activeColor,
            strokeWidth: activeStrokeWidth,
          });
        } else if (activeTool === "pen") {
          setCurrentDrawing({
            id,
            type: "pen",
            points: [pt],
            color: activeColor,
            strokeWidth: activeStrokeWidth,
          });
        } else if (activeTool === "mosaic") {
          setCurrentDrawing({
            id,
            type: "mosaic",
            bounds: { x: pt.x, y: pt.y, width: 0, height: 0 },
            blockSize: 10,
          });
        }
        return;
      }
    }

    // 4. Clicking outside crop or starting fresh -> create new crop
    setCrop(null);
    setElements([]);
    setUndoStack([]);
    setRedoStack([]);
    setDragState({
      mode: "creating",
      startPoint: pt,
      initialRect: null,
      activeHandle: null,
    });
  };

  const handleMouseMove = (e: React.MouseEvent<HTMLCanvasElement>) => {
    const pt: Point = { x: e.clientX, y: e.clientY };
    setCursorPos(pt);

    // Dynamic cursor styling
    const canvas = canvasRef.current;
    if (canvas && crop) {
      const hitHandle = hitTestHandle(pt, crop);
      if (hitHandle) {
        const cursorMap: Record<HandleType, string> = {
          nw: "nwse-resize",
          se: "nwse-resize",
          ne: "nesw-resize",
          sw: "nesw-resize",
          n: "ns-resize",
          s: "ns-resize",
          e: "ew-resize",
          w: "ew-resize",
        };
        canvas.style.cursor = cursorMap[hitHandle];
      } else if (activeTool === "select" && pointInRect(pt, crop)) {
        canvas.style.cursor = "move";
      } else if (activeTool !== "select" && pointInRect(pt, crop)) {
        canvas.style.cursor = "crosshair";
      } else {
        canvas.style.cursor = "crosshair";
      }
    }

    // Handle active drawing
    if (currentDrawing) {
      if (currentDrawing.type === "arrow") {
        setCurrentDrawing({ ...currentDrawing, end: pt });
      } else if (currentDrawing.type === "rect") {
        setCurrentDrawing({
          ...currentDrawing,
          bounds: normalizeRect(
            { x: currentDrawing.bounds.x, y: currentDrawing.bounds.y },
            pt
          ),
        });
      } else if (currentDrawing.type === "pen") {
        setCurrentDrawing({
          ...currentDrawing,
          points: [...currentDrawing.points, pt],
        });
      } else if (currentDrawing.type === "mosaic") {
        setCurrentDrawing({
          ...currentDrawing,
          bounds: normalizeRect(
            { x: currentDrawing.bounds.x, y: currentDrawing.bounds.y },
            pt
          ),
        });
      }
      return;
    }

    // Handle crop creation
    if (dragState.mode === "creating") {
      const newRect = normalizeRect(dragState.startPoint, pt);
      setCrop(newRect);
      return;
    }

    // Handle crop move
    if (dragState.mode === "moving" && dragState.initialRect) {
      const dx = pt.x - dragState.startPoint.x;
      const dy = pt.y - dragState.startPoint.y;
      setCrop({
        x: Math.max(0, Math.min(windowSize.width - dragState.initialRect.width, dragState.initialRect.x + dx)),
        y: Math.max(0, Math.min(windowSize.height - dragState.initialRect.height, dragState.initialRect.y + dy)),
        width: dragState.initialRect.width,
        height: dragState.initialRect.height,
      });
      return;
    }

    // Handle crop resize
    if (dragState.mode === "resizing" && dragState.initialRect && dragState.activeHandle) {
      const resized = resizeRectWithHandle(
        dragState.initialRect,
        dragState.activeHandle,
        pt,
        windowSize.width,
        windowSize.height
      );
      setCrop(resized);
    }
  };

  const handleMouseUp = () => {
    if (currentDrawing) {
      pushElement(currentDrawing);
      setCurrentDrawing(null);
    }

    setDragState({
      mode: "none",
      startPoint: { x: 0, y: 0 },
      initialRect: null,
      activeHandle: null,
    });
  };

  return (
    <div className="relative w-screen h-screen overflow-hidden select-none">
      {/* 4-Layer Interactive HTML5 Canvas */}
      <canvas
        ref={canvasRef}
        width={Math.round(windowSize.width * dpr)}
        height={Math.round(windowSize.height * dpr)}
        style={{ width: `${windowSize.width}px`, height: `${windowSize.height}px` }}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
        className="block absolute inset-0 z-10"
      />

      {/* Live Dimension Badge */}
      {crop && (
        <DimensionBadge crop={crop} scaleFactor={scaleFactor} />
      )}

      {/* 4x Magnifier Loupe */}
      <LoupeMagnifier
        cursor={cursorPos}
        visible={dragState.mode === "creating" || (!crop && dragState.mode === "none")}
        backgroundImage={backgroundImage}
        canvasWidth={windowSize.width}
        canvasHeight={windowSize.height}
      />

      {/* Floating Toolbar */}
      {crop && dragState.mode === "none" && !currentDrawing && (
        <FloatingToolbar
          crop={crop}
          activeTool={activeTool}
          onSelectTool={setActiveTool}
          activeColor={activeColor}
          onSelectColor={setActiveColor}
          activeStrokeWidth={activeStrokeWidth}
          onSelectStrokeWidth={setActiveStrokeWidth}
          canUndo={undoStack.length > 0}
          canRedo={redoStack.length > 0}
          onUndo={handleUndo}
          onRedo={handleRedo}
          onCopy={handleCopy}
          onSave={handleSave}
          onOcr={handleOcr}
          onOpenSettings={() => setIsSettingsOpen(true)}
          onClose={handleClose}
          copiedFeedback={copiedFeedback}
          ocrLoading={ocrLoading}
          windowWidth={windowSize.width}
          windowHeight={windowSize.height}
        />
      )}

      {/* In-Place Text Editing Overlay */}
      {editingText && (
        <input
          type="text"
          autoFocus
          value={editingText.text}
          onChange={(e) => setEditingText({ ...editingText, text: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              if (editingText.text.trim()) {
                pushElement({
                  id: `text-${Date.now()}`,
                  type: "text",
                  position: editingText.position,
                  text: editingText.text,
                  color: activeColor,
                  fontSize: activeStrokeWidth * 6,
                });
              }
              setEditingText(null);
            } else if (e.key === "Escape") {
              setEditingText(null);
            }
          }}
          style={{
            position: "absolute",
            left: `${editingText.position.x}px`,
            top: `${editingText.position.y}px`,
            color: activeColor,
            fontSize: `${activeStrokeWidth * 6}px`,
          }}
          className="z-50 bg-slate-900/80 border border-carambola-cyan rounded px-1.5 py-0.5 outline-none font-bold text-shadow shadow-lg"
          placeholder="Type annotation..."
        />
      )}

      {/* Top-right quick shortcut settings when idle */}
      {!crop && dragState.mode === "none" && (
        <div className="absolute top-4 right-4 z-40">
          <button
            onClick={() => setIsSettingsOpen(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-slate-900/85 hover:bg-slate-800 backdrop-blur-md text-slate-300 hover:text-white border border-slate-700/60 shadow-lg text-xs font-medium transition-colors"
          >
            <Settings size={13} className="text-carambola-cyan" />
            <span>Shortcut Settings</span>
          </button>
        </div>
      )}

      {/* Shortcut Configuration Modal */}
      <ShortcutModal
        isOpen={isSettingsOpen}
        onClose={() => setIsSettingsOpen(false)}
        onSaved={(sc) => {
          setToastMessage(`Shortcut updated: ${sc}`);
          setTimeout(() => setToastMessage(null), 3000);
        }}
      />

      {/* Toast Notification */}
      {toastMessage && (
        <div className="absolute top-6 left-1/2 -translate-x-1/2 z-50 px-4 py-2 rounded-xl bg-slate-900/90 text-carambola-yellow border border-slate-700 shadow-2xl text-xs font-mono font-medium toolbar-fade-in flex items-center gap-2">
          <span>{toastMessage}</span>
        </div>
      )}
    </div>
  );
};

