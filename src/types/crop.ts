export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type HandleType =
  | "nw"
  | "n"
  | "ne"
  | "e"
  | "se"
  | "s"
  | "sw"
  | "w";

export type DragMode =
  | "none"
  | "creating"
  | "moving"
  | "resizing";

export interface DragState {
  mode: DragMode;
  startPoint: Point;
  initialRect: Rect | null;
  activeHandle: HandleType | null;
}

