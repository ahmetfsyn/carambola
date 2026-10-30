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

export function drawMosaicShape(
  ctx: CanvasRenderingContext2D,
  bounds: Rect,
  blockSize: number
) {
  if (bounds.width <= 0 || bounds.height <= 0) return;

  ctx.save();
  try {
    const imgData = ctx.getImageData(bounds.x, bounds.y, bounds.width, bounds.height);
    const data = imgData.data;
    const w = imgData.width;
    const h = imgData.height;

    for (let y = 0; y < h; y += blockSize) {
      for (let x = 0; x < w; x += blockSize) {
        // Sample center pixel of current block
        const sampleX = Math.min(x + Math.floor(blockSize / 2), w - 1);
        const sampleY = Math.min(y + Math.floor(blockSize / 2), h - 1);
        const sampleIdx = (sampleY * w + sampleX) * 4;

        const r = data[sampleIdx];
        const g = data[sampleIdx + 1];
        const b = data[sampleIdx + 2];
        const a = data[sampleIdx + 3];

        // Fill all pixels in block with sample color
        for (let dy = 0; dy < blockSize && y + dy < h; dy++) {
          for (let dx = 0; dx < blockSize && x + dx < w; dx++) {
            const pixelIdx = ((y + dy) * w + (x + dx)) * 4;
            data[pixelIdx] = r;
            data[pixelIdx + 1] = g;
            data[pixelIdx + 2] = b;
            data[pixelIdx + 3] = a;
          }
        }
      }
    }

    ctx.putImageData(imgData, bounds.x, bounds.y);
  } catch (err) {
    // If getImageData fails due to security or out of bounds, draw pixel pattern
    ctx.fillStyle = "rgba(128, 128, 128, 0.8)";
    ctx.fillRect(bounds.x, bounds.y, bounds.width, bounds.height);
  }
  ctx.restore();
}

export function renderElement(ctx: CanvasRenderingContext2D, el: AnnotationElement) {
  switch (el.type) {
    case "arrow":
      drawArrow(ctx, el.start, el.end, el.color, el.strokeWidth);
      break;
    case "rect":
      drawRectShape(ctx, el.bounds, el.color, el.strokeWidth);
      break;
    case "pen":
      drawSmoothStroke(ctx, el.points, el.color, el.strokeWidth);
      break;
    case "text":
      drawTextShape(ctx, el.position, el.text, el.color, el.fontSize);
      break;
    case "mosaic":
      drawMosaicShape(ctx, el.bounds, el.blockSize);
      break;
  }
}

