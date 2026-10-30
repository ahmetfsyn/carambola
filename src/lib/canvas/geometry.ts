import { Point, Rect, HandleType } from "../../types/crop";

export const HANDLE_SIZE = 8;
export const HANDLE_HIT_SIZE = 14;

export function clamp(val: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, val));
}

export function pointInRect(p: Point, r: Rect): boolean {
  return (
    p.x >= r.x &&
    p.x <= r.x + r.width &&
    p.y >= r.y &&
    p.y <= r.y + r.height
  );
}

export function normalizeRect(p1: Point, p2: Point): Rect {
  const x = Math.min(p1.x, p2.x);
  const y = Math.min(p1.y, p2.y);
  const width = Math.abs(p1.x - p2.x);
  const height = Math.abs(p1.y - p2.y);
  return { x, y, width, height };
}

export function getHandlePositions(r: Rect): Record<HandleType, Point> {
  const cx = r.x + r.width / 2;
  const cy = r.y + r.height / 2;

  return {
    nw: { x: r.x, y: r.y },
    n: { x: cx, y: r.y },
    ne: { x: r.x + r.width, y: r.y },
    e: { x: r.x + r.width, y: cy },
    se: { x: r.x + r.width, y: r.y + r.height },
    s: { x: cx, y: r.y + r.height },
    sw: { x: r.x, y: r.y + r.height },
    w: { x: r.x, y: cy },
  };
}

export function hitTestHandle(p: Point, r: Rect): HandleType | null {
  const handles = getHandlePositions(r);
  const halfHit = HANDLE_HIT_SIZE / 2;

  for (const [key, pos] of Object.entries(handles) as [HandleType, Point][]) {
    if (
      p.x >= pos.x - halfHit &&
      p.x <= pos.x + halfHit &&
      p.y >= pos.y - halfHit &&
      p.y <= pos.y + halfHit
    ) {
      return key;
    }
  }

  return null;
}

export function resizeRectWithHandle(
  initialRect: Rect,
  handle: HandleType,
  currentPoint: Point,
  maxWidth: number,
  maxHeight: number
): Rect {
  const r = { ...initialRect };
  const curX = clamp(currentPoint.x, 0, maxWidth);
  const curY = clamp(currentPoint.y, 0, maxHeight);

  switch (handle) {
    case "nw":
      r.width = initialRect.x + initialRect.width - curX;
      r.height = initialRect.y + initialRect.height - curY;
      r.x = curX;
      r.y = curY;
      break;
    case "n":
      r.height = initialRect.y + initialRect.height - curY;
      r.y = curY;
      break;
    case "ne":
      r.width = curX - initialRect.x;
      r.height = initialRect.y + initialRect.height - curY;
      r.y = curY;
      break;
    case "e":
      r.width = curX - initialRect.x;
      break;
    case "se":
      r.width = curX - initialRect.x;
      r.height = curY - initialRect.y;
      break;
    case "s":
      r.height = curY - initialRect.y;
      break;
    case "sw":
      r.width = initialRect.x + initialRect.width - curX;
      r.height = curY - initialRect.y;
      r.x = curX;
      break;
    case "w":
      r.width = initialRect.x + initialRect.width - curX;
      r.x = curX;
      break;
  }

  // Handle inverting / flipping
  const finalX = r.width < 0 ? r.x + r.width : r.x;
  const finalY = r.height < 0 ? r.y + r.height : r.y;
  const finalWidth = Math.abs(r.width);
  const finalHeight = Math.abs(r.height);

  return {
    x: clamp(finalX, 0, maxWidth),
    y: clamp(finalY, 0, maxHeight),
    width: Math.min(finalWidth, maxWidth - finalX),
    height: Math.min(finalHeight, maxHeight - finalY),
  };
}

