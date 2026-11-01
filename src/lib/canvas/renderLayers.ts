import { Rect, Point } from "../../types/crop";
import { AnnotationElement } from "../../types/annotations";
import { renderElement } from "./drawShapes";
import { getHandlePositions, HANDLE_SIZE } from "./geometry";

export interface RenderParams {
  ctx: CanvasRenderingContext2D;
  /** Layout width of the canvas in CSS pixels. */
  canvasWidth: number;
  /** Layout height of the canvas in CSS pixels. */
  canvasHeight: number;
  /** Device pixel ratio of the canvas backing store. */
  dpr: number;
  backgroundImage: ImageBitmap | HTMLImageElement | null;
  cropRect: Rect | null;
  elements: AnnotationElement[];
  activeElement: AnnotationElement | null;
  isHovered: boolean;
}

/** Reports the natural pixel width of a canvas image source, if known. */
function imageNaturalWidth(
  image: ImageBitmap | HTMLImageElement | null
): number | null {
  if (!image) return null;
  if ("naturalWidth" in image && image.naturalWidth) return image.naturalWidth;
  if ("width" in image && typeof image.width === "number" && image.width) {
    return image.width;
  }
  return null;
}

/**
 * Screenshot (physical) pixels per CSS pixel for a layout width. Used both for
 * compositing the frozen screenshot and for redaction sampling so overlay and
 * exported output share one coordinate mapping at any DPI scale.
 */
export function getImageScale(
  image: ImageBitmap | HTMLImageElement | null,
  cssWidth: number
): number {
  const natural = imageNaturalWidth(image);
  if (!natural || !cssWidth) return 1;
  return natural / cssWidth;
}

export function renderCanvas(params: RenderParams) {
  const {
    ctx,
    canvasWidth,
    canvasHeight,
    dpr,
    backgroundImage,
    cropRect,
    elements,
    activeElement,
  } = params;

  // The canvas backing store is sized in physical pixels (`css * dpr`) while
  // every coordinate in this module is expressed in CSS pixels. Scaling the
  // context once here keeps a single, consistent coordinate system: the
  // pointer, the selection rectangle, the annotations and the screenshot all
  // share the same CSS-pixel space and the browser maps it 1:1 onto the
  // physical display.
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  // Clear entire canvas (CSS-pixel extents cover the full backing store).
  ctx.clearRect(0, 0, canvasWidth, canvasHeight);

  // 1. Draw background image, mapping the screenshot (natural/physical pixels)
  //    onto the CSS-pixel canvas. `imageScale` is the factor between the two
  //    spaces and must be applied to every source-image rectangle.
  const imageScale = getImageScale(backgroundImage, canvasWidth);

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

    // Redraw the crisp background inside the cutout. The source rectangle is in
    // screenshot (image) pixels, so it must be scaled from the CSS-pixel crop
    // rect by `imageScale`; the destination stays in CSS pixels.
    if (backgroundImage) {
      ctx.drawImage(
        backgroundImage,
        cropRect.x * imageScale,
        cropRect.y * imageScale,
        cropRect.width * imageScale,
        cropRect.height * imageScale,
        cropRect.x,
        cropRect.y,
        cropRect.width,
        cropRect.height
      );
    }
  }
  ctx.restore();

  // 3. Draw committed annotations. Redaction samples the original screenshot,
  //    so it stays non-destructive and never alters other annotations.
  const mosaicSource = backgroundImage ? { image: backgroundImage, imageScale } : undefined;
  for (const el of elements) {
    renderElement(ctx, el, mosaicSource);
  }

  // 4. Draw currently active / in-progress element
  if (activeElement) {
    renderElement(ctx, activeElement, mosaicSource);
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
