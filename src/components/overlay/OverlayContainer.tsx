import React, { useRef, useState, useEffect, useLayoutEffect, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { Point, Rect, DragState, HandleType } from "../../types/crop";
import { ToolType, ShapeMode, AnnotationElement } from "../../types/annotations";
import {
  pointInRect,
  normalizeRect,
  hitTestHandle,
  resizeRectWithHandle,
} from "../../lib/canvas/geometry";
import { renderCanvas, getImageScale } from "../../lib/canvas/renderLayers";
import { composeCropRgba } from "../../lib/canvas/composeCrop";
import { FloatingToolbar, nextShapeMode } from "../toolbar/FloatingToolbar";
import { DimensionBadge } from "./DimensionBadge";
import { ShortcutModal } from "../settings/ShortcutModal";
import {
  copyCropImage,
  saveCropImage,
  hideOverlay,
  extractOcrText,
  getScreenshotNotificationSettings,
} from "../../lib/ipc/commands";
import { notifyScreenshotExported } from "../../lib/notifications";

/** Duration of the close (fade-out) transition, matching the CSS transition. */
const CLOSE_FADE_MS = 180;
const isMac = /Mac/i.test(navigator.platform);

function prefersReducedMotion(): boolean {
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

interface OverlayContainerProps {
  backgroundImage: ImageBitmap | HTMLImageElement | null;
  scaleFactor: number;
  captureToken: number;
  presentationReady: boolean;
}

export const OverlayContainer: React.FC<OverlayContainerProps> = ({
  backgroundImage,
  scaleFactor,
  captureToken,
  presentationReady,
}) => {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);

  // Device pixel ratio for this webview. The canvas backing store is sized in
  // physical pixels (`css * dpr`) so the screenshot is rendered crisply at the
  // display's native resolution, while all interaction coordinates stay in CSS
  // pixels (the canvas context is scaled by `dpr` in `renderCanvas`).
  const dpr = window.devicePixelRatio || 1;

  // Crop & interaction state
  const [crop, setCrop] = useState<Rect | null>(null);
  const [dragState, setDragState] = useState<DragState>({
    mode: "none",
    startPoint: { x: 0, y: 0 },
    initialRect: null,
    activeHandle: null,
  });

  // Annotation tools & history state
  const [activeTool, setActiveTool] = useState<ToolType>("select");
  const [shapeMode, setShapeMode] = useState<ShapeMode>("rect");
  const [activeColor, setActiveColor] = useState<string>("#EF4444");
  const [activeStrokeWidth, setActiveStrokeWidth] = useState<number>(2);
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

  // Close (fade-out) lifecycle. The root layer fades to transparent, then the
  // native window is hidden once the transition has finished.
  const [closing, setClosing] = useState<boolean>(false);
  const closingRef = useRef<boolean>(false);
  const closeTimer = useRef<number | null>(null);
  const notifyAfterClose = useRef<(() => void) | null>(null);
  const notifiedCaptureToken = useRef<number | null>(null);
  // Fixed origin of the shape/mosaic currently being drawn. Bounds must always
  // be normalized against this point; normalizing against the previous (already
  // normalized) bounds made reverse drags collapse the shape on every move.
  const drawingOrigin = useRef<Point | null>(null);
  const textInputRef = useRef<HTMLInputElement | null>(null);
  const textEditing = editingText !== null;

  // `autoFocus` alone did not focus the annotation input inside WebView2, so a
  // user who placed a text box could type and nothing would happen until they
  // clicked the input itself. Focus it explicitly once it is mounted, and again
  // on the next task: the native mouse press finishes its own focus handling
  // after React's effects run and would otherwise blur the input again.
  useEffect(() => {
    if (!textEditing) return;
    textInputRef.current?.focus();
    const timer = window.setTimeout(() => textInputRef.current?.focus(), 0);
    return () => window.clearTimeout(timer);
  }, [textEditing]);

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

    // Tear down safely even if registration resolves after unmount (StrictMode).
    let disposed = false;
    let unlistenSettings: (() => void) | null = null;
    listen("open_shortcut_settings", () => {
      setIsSettingsOpen(true);
    })
      .then((unlisten) => {
        if (disposed) {
          unlisten();
        } else {
          unlistenSettings = unlisten;
        }
      })
      .catch(() => {});

    return () => {
      disposed = true;
      window.removeEventListener("resize", handleResize);
      unlistenSettings?.();
      unlistenSettings = null;
    };
  }, []);

  // Every new capture starts a completely new session. This runs as a layout
  // effect so the reset is committed (and the canvas redrawn) synchronously,
  // before the window is shown, rather than after a visible stale frame.
  useLayoutEffect(() => {
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
    // A new session always starts with the selection tool.
    setActiveTool("select");
    setCopiedFeedback(false);
    setOcrLoading(false);
    setToastMessage(null);
    // Screenshot sessions must never restore a previous Settings view. Settings
    // is reopened explicitly through the tray/toolbar only, after presentation.
    setIsSettingsOpen(false);
    // Cancel any in-flight fade from the previous session before presenting.
    if (closeTimer.current !== null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
    closingRef.current = false;
    notifyAfterClose.current = null;
    setClosing(false);
    drawingOrigin.current = null;
  }, [captureToken]);

  // Smoothly fade the whole overlay (screenshot + dimming + toolbar) to
  // transparent, then hide the native window. Only opacity is animated: no
  // scaling, sliding or OS minimize animation is involved.
  const requestClose = useCallback(() => {
    // Guard in a ref: React StrictMode may invoke state updater functions more
    // than once in development, and scheduling the hide timer inside an updater
    // would call hide_overlay twice. Closing also dismisses Settings right away
    // so no navigation state can outlive the cancelled screenshot session.
    if (closingRef.current) return;
    closingRef.current = true;
    setIsSettingsOpen(false);
    setClosing(true);
    const duration = prefersReducedMotion() ? 0 : CLOSE_FADE_MS;
    closeTimer.current = window.setTimeout(() => {
      closeTimer.current = null;
      const notify = notifyAfterClose.current;
      notifyAfterClose.current = null;
      void hideOverlay()
        .catch((err) => console.error("Failed to hide overlay:", err))
        .finally(() => notify?.());
    }, duration);
  }, []);

  // Clear the pending fade timer if the component unmounts.
  useEffect(() => {
    return () => {
      if (closeTimer.current !== null) {
        window.clearTimeout(closeTimer.current);
        closeTimer.current = null;
      }
    };
  }, []);

  // Main canvas render. Layout effect so a fresh frame + reset session are
  // painted before the overlay window is presented.
  useLayoutEffect(() => {
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
  }, [windowSize, backgroundImage, crop, elements, currentDrawing, dpr, captureToken]);

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

  const notifySuccessfulExport = useCallback(async () => {
    if (notifiedCaptureToken.current === captureToken) return;
    try {
      const settings = await getScreenshotNotificationSettings();
      if (!settings.enabled || notifiedCaptureToken.current === captureToken) return;
      notifiedCaptureToken.current = captureToken;
      await notifyScreenshotExported(settings);
    } catch (err) {
      console.warn("Could not read screenshot notification settings:", err);
    }
  }, [captureToken]);

  const handleRedo = useCallback(() => {
    if (redoStack.length === 0) return;
    const next = redoStack[redoStack.length - 1];
    setUndoStack((prev) => [...prev, elements]);
    setRedoStack((prev) => prev.slice(0, -1));
    setElements(next);
  }, [elements, redoStack]);

  // Output handlers
  const handleCopy = useCallback(async () => {
    if (!crop || crop.width <= 0 || crop.height <= 0 || !backgroundImage) return;
    try {
      const imageScale = getImageScale(backgroundImage, windowSize.width);
      const bytes = composeCropRgba(crop, elements, backgroundImage, imageScale);
      await copyCropImage(bytes);
      if (isMac) {
        notifyAfterClose.current = () => void notifySuccessfulExport();
      } else {
        void notifySuccessfulExport();
      }
      setCopiedFeedback(true);
      setTimeout(() => {
        setCopiedFeedback(false);
        requestClose();
      }, 350);
    } catch (err) {
      console.error("Failed to copy crop:", err);
    }
  }, [crop, elements, backgroundImage, windowSize.width, requestClose, notifySuccessfulExport]);

  const handleSave = useCallback(async () => {
    if (!crop || crop.width <= 0 || crop.height <= 0 || !backgroundImage) return;
    try {
      const imageScale = getImageScale(backgroundImage, windowSize.width);
      const bytes = composeCropRgba(crop, elements, backgroundImage, imageScale);
      const savedPath = await saveCropImage(bytes);
      if (isMac) {
        notifyAfterClose.current = () => void notifySuccessfulExport();
      } else {
        void notifySuccessfulExport();
      }
      setToastMessage(`Saved to ${savedPath}`);
      setTimeout(() => {
        setToastMessage(null);
        requestClose();
      }, 800);
    } catch (err) {
      console.error("Failed to save crop:", err);
    }
  }, [crop, elements, backgroundImage, windowSize.width, requestClose, notifySuccessfulExport]);

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
      const message = typeof err === "string" ? err : "OCR is not available on this platform";
      setToastMessage(message);
      setTimeout(() => setToastMessage(null), 3500);
    }
  }, [crop, scaleFactor]);

  const handleClose = useCallback(() => {
    requestClose();
  }, [requestClose]);

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
        fontSize: Math.max(16, activeStrokeWidth * 6),
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
          drawingOrigin.current = pt;
          setCurrentDrawing({
            id,
            type: shapeMode,
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
          drawingOrigin.current = pt;
          setCurrentDrawing({
            id,
            type: "mosaic",
            bounds: { x: pt.x, y: pt.y, width: 0, height: 0 },
            // Redaction must destroy readable detail, not merely soften it.
            // 32 CSS px blocks average away the strokes of normal UI text and
            // remain irreversible in the exported pixels.
            blockSize: 32,
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
      } else if (
        currentDrawing.type === "rect" ||
        currentDrawing.type === "triangle" ||
        currentDrawing.type === "circle"
      ) {
        setCurrentDrawing({
          ...currentDrawing,
          bounds: normalizeRect(drawingOrigin.current ?? pt, pt),
        });
      } else if (currentDrawing.type === "pen") {
        setCurrentDrawing({
          ...currentDrawing,
          points: [...currentDrawing.points, pt],
        });
      } else if (currentDrawing.type === "mosaic") {
        setCurrentDrawing({
          ...currentDrawing,
          bounds: normalizeRect(drawingOrigin.current ?? pt, pt),
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
    drawingOrigin.current = null;

    setDragState({
      mode: "none",
      startPoint: { x: 0, y: 0 },
      initialRect: null,
      activeHandle: null,
    });
  };

  return (
    <div
      className="relative w-screen h-screen overflow-hidden select-none"
      style={{
        opacity: closing || (isMac && !presentationReady) ? 0 : 1,
        transition: !prefersReducedMotion() && (isMac || closing)
          ? `opacity ${CLOSE_FADE_MS}ms ease-out`
          : "none",
        pointerEvents: closing ? "none" : "auto",
      }}
    >
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

      {/* Floating Toolbar */}
      {crop && dragState.mode === "none" && !currentDrawing && (
        <FloatingToolbar
          crop={crop}
          activeTool={activeTool}
          shapeMode={shapeMode}
          onSelectTool={setActiveTool}
          onCycleShape={() => setShapeMode((mode) => nextShapeMode(mode))}
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
          ref={textInputRef}
          autoFocus
          value={editingText.text}
          onChange={(e) => setEditingText({ ...editingText, text: e.target.value })}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              // React flushes this state update before the native window
              // keydown listener runs. Stop the same Enter event reaching the
              // global Copy shortcut after the editor closes.
              e.preventDefault();
              e.stopPropagation();
              if (editingText.text.trim()) {
                pushElement({
                  id: `text-${Date.now()}`,
                  type: "text",
                  position: editingText.position,
                  text: editingText.text,
                  color: activeColor,
                  fontSize: Math.max(16, activeStrokeWidth * 6),
                });
              }
              setEditingText(null);
            } else if (e.key === "Escape") {
              e.preventDefault();
              e.stopPropagation();
              setEditingText(null);
            }
          }}
          style={{
            position: "absolute",
            left: `${editingText.position.x}px`,
            top: `${editingText.position.y}px`,
            color: activeColor,
            fontSize: `${Math.max(16, activeStrokeWidth * 6)}px`,
          }}
          className="z-50 bg-slate-900/80 border border-carambola-cyan rounded px-1.5 py-0.5 outline-none font-bold text-shadow shadow-lg"
          placeholder="Type annotation..."
        />
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

