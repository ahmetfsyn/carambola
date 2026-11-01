import { Rect } from "../../types/crop";
import { AnnotationElement } from "../../types/annotations";
import { renderElement } from "./drawShapes";

export type CompositeImage = ImageBitmap | HTMLImageElement;

/**
 * Bakes the selected crop and all annotations (including pixelation redaction)
 * into a raw RGBA buffer at full screenshot resolution.
 *
 * The compositor runs on the real captured pixels: the crop region is copied
 * 1:1 from the screenshot, then annotations are drawn on top using the same
 * renderers as the on-screen overlay. Redaction samples the original screenshot,
 * so the underlying content cannot survive in the output.
 *
 * Layout: `[u32 LE width][u32 LE height][RGBA bytes...]`.
 */
export function composeCropRgba(
  crop: Rect,
  elements: AnnotationElement[],
  image: CompositeImage,
  imageScale: number
): Uint8Array {
  const width = Math.max(1, Math.round(crop.width * imageScale));
  const height = Math.max(1, Math.round(crop.height * imageScale));

  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    throw new Error("Could not create export canvas context");
  }

  // 1. Copy the selected screenshot region at native resolution.
  ctx.clearRect(0, 0, width, height);
  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(
    image,
    Math.round(crop.x * imageScale),
    Math.round(crop.y * imageScale),
    width,
    height,
    0,
    0,
    width,
    height
  );

  // 2. Draw annotations in the crop's coordinate space, scaled to screenshot
  //    pixels so the output matches what the user saw in the overlay.
  ctx.setTransform(
    imageScale,
    0,
    0,
    imageScale,
    -crop.x * imageScale,
    -crop.y * imageScale
  );
  for (const el of elements) {
    renderElement(ctx, el, { image, imageScale });
  }
  ctx.setTransform(1, 0, 0, 1, 0, 0);

  // 3. Emit the raw pixels with a small dimension header for the backend.
  const data = ctx.getImageData(0, 0, width, height).data;
  const out = new Uint8Array(8 + data.length);
  const view = new DataView(out.buffer, out.byteOffset, out.byteLength);
  view.setUint32(0, width, true);
  view.setUint32(4, height, true);
  out.set(data, 8);
  return out;
}
