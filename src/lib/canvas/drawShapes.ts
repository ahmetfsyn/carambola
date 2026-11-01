import { Point, Rect } from "../../types/crop";
import { AnnotationElement } from "../../types/annotations";

export function drawArrow(
  ctx: CanvasRenderingContext2D,
  start: Point,
  end: Point,
  color: string,
  strokeWidth: number
) {
  const headLength = Math.max(14, strokeWidth * 3.5);
  const angle = Math.atan2(end.y - start.y, end.x - start.x);

  ctx.save();
  ctx.strokeStyle = color;
  ctx.fillStyle = color;
  ctx.lineWidth = strokeWidth;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  // Draw main shaft
  ctx.beginPath();
  ctx.moveTo(start.x, start.y);
  // Shorten shaft slightly so head sits crisply at the endpoint
  const shaftEndX = end.x - Math.cos(angle) * (headLength * 0.4);
  const shaftEndY = end.y - Math.sin(angle) * (headLength * 0.4);
  ctx.lineTo(shaftEndX, shaftEndY);
  ctx.stroke();

  // Draw arrow head triangle
  ctx.beginPath();
  ctx.moveTo(end.x, end.y);
  ctx.lineTo(
    end.x - headLength * Math.cos(angle - Math.PI / 6),
    end.y - headLength * Math.sin(angle - Math.PI / 6)
  );
  ctx.lineTo(
    end.x - (headLength * 0.6) * Math.cos(angle),
    end.y - (headLength * 0.6) * Math.sin(angle)
  );
  ctx.lineTo(
    end.x - headLength * Math.cos(angle + Math.PI / 6),
    end.y - headLength * Math.sin(angle + Math.PI / 6)
  );
  ctx.closePath();
  ctx.fill();

  ctx.restore();
}

export function drawRectShape(
  ctx: CanvasRenderingContext2D,
  bounds: Rect,
  color: string,
  strokeWidth: number
) {
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = strokeWidth;
  ctx.lineJoin = "round";
  ctx.strokeRect(bounds.x, bounds.y, bounds.width, bounds.height);
  ctx.restore();
}

export function drawTriangleShape(
  ctx: CanvasRenderingContext2D,
  bounds: Rect,
  color: string,
  strokeWidth: number
) {
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = strokeWidth;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";

  // Bounds are already normalized, so the apex is always top-center and the
  // base is the bottom edge regardless of the drag direction.
  ctx.beginPath();
  ctx.moveTo(bounds.x + bounds.width / 2, bounds.y);
  ctx.lineTo(bounds.x + bounds.width, bounds.y + bounds.height);
  ctx.lineTo(bounds.x, bounds.y + bounds.height);
  ctx.closePath();
  ctx.stroke();
  ctx.restore();
}

export function drawCircleShape(
  ctx: CanvasRenderingContext2D,
  bounds: Rect,
  color: string,
  strokeWidth: number
) {
  if (bounds.width <= 0 || bounds.height <= 0) return;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = strokeWidth;
  ctx.lineJoin = "round";

  // Ellipse inscribed in the (normalized) drag bounds.
  ctx.beginPath();
  ctx.ellipse(
    bounds.x + bounds.width / 2,
    bounds.y + bounds.height / 2,
    bounds.width / 2,
    bounds.height / 2,
    0,
    0,
    Math.PI * 2
  );
  ctx.stroke();
  ctx.restore();
}

export function drawSmoothStroke(
  ctx: CanvasRenderingContext2D,
  points: Point[],
  color: string,
  strokeWidth: number
) {
  if (points.length < 2) return;

  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = strokeWidth;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";

  ctx.beginPath();
  ctx.moveTo(points[0].x, points[0].y);

  if (points.length === 2) {
    ctx.lineTo(points[1].x, points[1].y);
  } else {
    // Quadratic bezier curve interpolation through midpoints for ultra-smooth strokes
    for (let i = 1; i < points.length - 1; i++) {
      const xc = (points[i].x + points[i + 1].x) / 2;
      const yc = (points[i].y + points[i + 1].y) / 2;
      ctx.quadraticCurveTo(points[i].x, points[i].y, xc, yc);
    }
    const last = points[points.length - 1];
    ctx.lineTo(last.x, last.y);
  }

  ctx.stroke();
  ctx.restore();
}

export function drawTextShape(
  ctx: CanvasRenderingContext2D,
  position: Point,
  text: string,
  color: string,
  fontSize: number
) {
  if (!text) return;
  ctx.save();
  ctx.font = `bold ${fontSize}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
  ctx.textBaseline = "top";

  const lines = text.split("\n");
  const lineHeight = fontSize * 1.25;

  lines.forEach((line, index) => {
    const y = position.y + index * lineHeight;
    // Draw subtle dark shadow/outline for readability against any background
    ctx.strokeStyle = "rgba(0, 0, 0, 0.75)";
    ctx.lineWidth = 3;
    ctx.lineJoin = "round";
    ctx.strokeText(line, position.x, y);

    // Draw main text fill
    ctx.fillStyle = color;
    ctx.fillText(line, position.x, y);
  });

  ctx.restore();
}

/**
 * Pixelation/redaction source. The mosaic is always sampled from the original
 * screenshot pixels (never from the already-composited canvas), so the effect
 * obscures the real content and never compounds with other annotations.
 */
export interface MosaicSource {
  image: CanvasImageSource;
  /** Screenshot pixels per CSS pixel. */
  imageScale: number;
}

// Reused scratch canvas for the pixelation pass. Kept module-level so the
// redaction of a committed element does not allocate on every redraw.
let mosaicCanvas: HTMLCanvasElement | null = null;

function getMosaicCanvas(width: number, height: number): HTMLCanvasElement {
  if (!mosaicCanvas) {
    mosaicCanvas = document.createElement("canvas");
  }
  if (mosaicCanvas.width !== width) mosaicCanvas.width = width;
  if (mosaicCanvas.height !== height) mosaicCanvas.height = height;
  return mosaicCanvas;
}

export function drawMosaicShape(
  ctx: CanvasRenderingContext2D,
  bounds: Rect,
  blockSize: number,
  source: MosaicSource
) {
  if (bounds.width <= 0 || bounds.height <= 0) return;

  const scale = source.imageScale || 1;
  const width = Math.max(1, Math.round(bounds.width * scale));
  const height = Math.max(1, Math.round(bounds.height * scale));
  const block = Math.max(1, Math.round(blockSize * scale));

  // 1. Copy the pristine screenshot region into the scratch canvas at full
  //    resolution, 1:1 with the captured pixels.
  const scratch = getMosaicCanvas(width, height);
  const sctx = scratch.getContext("2d");
  if (!sctx) return;
  sctx.setTransform(1, 0, 0, 1, 0, 0);
  sctx.clearRect(0, 0, width, height);
  sctx.imageSmoothingEnabled = false;
  sctx.drawImage(
    source.image,
    bounds.x * scale,
    bounds.y * scale,
    bounds.width * scale,
    bounds.height * scale,
    0,
    0,
    width,
    height
  );

  // 2. Replace each pixel inside a block by the block's average colour. Using
  //    the average (instead of a single sampled pixel) keeps the mosaic stable
  //    while still destroying all readable detail.
  let imageData: ImageData;
  try {
    imageData = sctx.getImageData(0, 0, width, height);
  } catch {
    // Tainted/unavailable canvas: fall back to a solid opaque fill so the area
    // is still redacted rather than left readable.
    ctx.save();
    ctx.imageSmoothingEnabled = false;
    ctx.fillStyle = "#3f3f46";
    ctx.fillRect(bounds.x, bounds.y, bounds.width, bounds.height);
    ctx.restore();
    return;
  }

  const data = imageData.data;
  for (let by = 0; by < height; by += block) {
    const maxY = Math.min(by + block, height);
    for (let bx = 0; bx < width; bx += block) {
      const maxX = Math.min(bx + block, width);

      let r = 0;
      let g = 0;
      let b = 0;
      let a = 0;
      let count = 0;
      for (let y = by; y < maxY; y++) {
        for (let x = bx; x < maxX; x++) {
          const idx = (y * width + x) * 4;
          r += data[idx];
          g += data[idx + 1];
          b += data[idx + 2];
          a += data[idx + 3];
          count++;
        }
      }
      if (count === 0) continue;
      r = Math.round(r / count);
      g = Math.round(g / count);
      b = Math.round(b / count);
      a = Math.round(a / count);

      for (let y = by; y < maxY; y++) {
        for (let x = bx; x < maxX; x++) {
          const idx = (y * width + x) * 4;
          data[idx] = r;
          data[idx + 1] = g;
          data[idx + 2] = b;
          data[idx + 3] = a;
        }
      }
    }
  }
  sctx.putImageData(imageData, 0, 0);

  // 3. Draw the pixelated region back over the target rectangle. Smoothing is
  //    disabled so the blocks stay crisp instead of blurring into a gradient.
  ctx.save();
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(scratch, 0, 0, width, height, bounds.x, bounds.y, bounds.width, bounds.height);
  ctx.restore();
}

export function renderElement(
  ctx: CanvasRenderingContext2D,
  el: AnnotationElement,
  mosaicSource?: MosaicSource
) {
  switch (el.type) {
    case "arrow":
      drawArrow(ctx, el.start, el.end, el.color, el.strokeWidth);
      break;
    case "rect":
      drawRectShape(ctx, el.bounds, el.color, el.strokeWidth);
      break;
    case "triangle":
      drawTriangleShape(ctx, el.bounds, el.color, el.strokeWidth);
      break;
    case "circle":
      drawCircleShape(ctx, el.bounds, el.color, el.strokeWidth);
      break;
    case "pen":
      drawSmoothStroke(ctx, el.points, el.color, el.strokeWidth);
      break;
    case "text":
      drawTextShape(ctx, el.position, el.text, el.color, el.fontSize);
      break;
    case "mosaic":
      if (mosaicSource) {
        drawMosaicShape(ctx, el.bounds, el.blockSize, mosaicSource);
      }
      break;
  }
}

