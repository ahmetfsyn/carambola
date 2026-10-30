import { Rect, Point } from "../../types/crop";
import { AnnotationElement } from "../../types/annotations";
import { renderElement } from "./drawShapes";
import { getHandlePositions, HANDLE_SIZE } from "./geometry";

export interface RenderParams {
  ctx: CanvasRenderingContext2D;
  canvasWidth: number;
  canvasHeight: number;
  backgroundImage: ImageBitmap | HTMLImageElement | null;
  cropRect: Rect | null;
  elements: AnnotationElement[];
  activeElement: AnnotationElement | null;
  isHovered: boolean;
}

export function renderCanvas(params: RenderParams) {
  const {
    ctx,
    canvasWidth,
    canvasHeight,
    backgroundImage,
    cropRect,
    elements,
    activeElement,
  } = params;

  // Clear entire canvas
  ctx.clearRect(0, 0, canvasWidth, canvasHeight);

  // 1. Draw background image
  if (backgroundImage) {
    ctx.drawImage(backgroundImage, 0, 0, canvasWidth, canvasHeight);
  }

  // 2. Draw dimmed mask over unselected areas
  ctx.save();
  ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
  ctx.fillRect(0, 0, canvasWidth, canvasHeight);

  if (cropRect && cropRect.width > 0 && cropRect.height > 0) {
    // Cut out clear hole for crop selection
    ctx.globalCompositeOperation = "destination-out";
    ctx.fillStyle = "#000000";
    ctx.fillRect(cropRect.x, cropRect.y, cropRect.width, cropRect.height);
    ctx.globalCompositeOperation = "source-over";

    // Redraw the crisp background inside the cutout
    if (backgroundImage) {
      ctx.drawImage(
        backgroundImage,
        cropRect.x,
        cropRect.y,
        cropRect.width,
        cropRect.height,
        cropRect.x,
        cropRect.y,
        cropRect.width,
        cropRect.height
      );
    }
  }
  ctx.restore();

  // 3. Draw committed annotations
  for (const el of elements) {
    renderElement(ctx, el);
  }

  // 4. Draw currently active / in-progress element
  if (activeElement) {
    renderElement(ctx, activeElement);
  }

  // 5. Draw selection border & handles
  if (cropRect && cropRect.width > 0 && cropRect.height > 0) {
    drawCropBorderAndHandles(ctx, cropRect);
  }
}

function drawCropBorderAndHandles(ctx: CanvasRenderingContext2D, r: Rect) {
  ctx.save();

  // Subtle drop shadow for crop box
  ctx.strokeStyle = "#06B6D4"; // Carambola vibrant cyan
  ctx.lineWidth = 1.5;
  ctx.strokeRect(r.x, r.y, r.width, r.height);

  // Draw 8 resize handles
  const handles = getHandlePositions(r);
  const half = HANDLE_SIZE / 2;

  ctx.fillStyle = "#FFFFFF";
  ctx.strokeStyle = "#06B6D4";
  ctx.lineWidth = 2;

  for (const pos of Object.values(handles) as Point[]) {
    ctx.fillRect(pos.x - half, pos.y - half, HANDLE_SIZE, HANDLE_SIZE);
    ctx.strokeRect(pos.x - half, pos.y - half, HANDLE_SIZE, HANDLE_SIZE);
  }

  ctx.restore();
}
