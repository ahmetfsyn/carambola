import { Point, Rect } from "./crop";

export type ToolType =
  | "select"
  | "arrow"
  | "rect"
  | "pen"
  | "text"
  | "mosaic";

export interface ArrowElement {
  id: string;
  type: "arrow";
  start: Point;
  end: Point;
  color: string;
  strokeWidth: number;
}

export interface RectElement {
  id: string;
  type: "rect";
  bounds: Rect;
  color: string;
  strokeWidth: number;
}

export interface PenElement {
  id: string;
  type: "pen";
  points: Point[];
  color: string;
  strokeWidth: number;
}

export interface TextElement {
  id: string;
  type: "text";
  position: Point;
  text: string;
  color: string;
  fontSize: number;
}

export interface MosaicElement {
  id: string;
  type: "mosaic";
  bounds: Rect;
  blockSize: number;
}

export type AnnotationElement =
  | ArrowElement
  | RectElement
  | PenElement
  | TextElement
  | MosaicElement;

export interface AnnotationState {
  elements: AnnotationElement[];
  undoStack: AnnotationElement[][];
  redoStack: AnnotationElement[][];
}

