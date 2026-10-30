export interface DisplayBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface DisplayInfo {
  id: number;
  name: string;
  isPrimary: boolean;
  scaleFactor: number;
  bounds: DisplayBounds;
  physicalWidth: number;
  physicalHeight: number;
}

