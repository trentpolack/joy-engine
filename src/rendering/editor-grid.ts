// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export interface EditorGridView {
  /** Borrowed Y-up camera target and backward direction, in world coordinates. */
  target: readonly number[];
  backward: readonly number[];
  distance: number;
  viewportHeightPixels: number;
  /** Height of the editor's XZ reference plane, in the caller's unchanged world units. */
  height?: number;
}

export interface EditorGridFrame {
  /** Borrowed cached XYZ/RGBA triangles. Submit as unlit, depth-tested transparency; never mutate. */
  triangles: Float32Array;
  /** Minor spacing in world units; major spacing is five times this value. */
  spacing: number;
}

const HALF_LINE_COUNT = 24;
const MINOR_COLOR = [0.20, 0.25, 0.32];
const MAJOR_COLOR = [0.31, 0.38, 0.47];
const X_AXIS_COLOR = [0.60, 0.32, 0.20];
const Z_AXIS_COLOR = [0.24, 0.43, 0.60];

/**
 * Own a small cached editor aid, without DOM, GPU resources or frame instrumentation.
 * Camera-facing ribbons keep lines readable across zoom and orbit. World units are
 * never rescaled: minor spacing follows a 1/2/5 sequence and major lines mark five
 * divisions. The origin axes stay at world zero while the finite grid follows pan.
 * All colors are linear and all triangles are transparent, so grids never become
 * lit surfaces or write depth over authored geometry. Discard this owner on teardown.
 */
export class EditorGrid {
  private declare key: string;
  private declare cached: EditorGridFrame;

  constructor() {
    this.key = '';
    this.cached = {triangles: new Float32Array(0), spacing: 1};
  }

  /** Return a borrowed frame; unchanged cameras reuse the same bounded array. */
  frame({target, backward, distance, viewportHeightPixels, height = 0}: EditorGridView): EditorGridFrame {
    if(!Number.isFinite(distance) || distance <= 0 || !Number.isFinite(viewportHeightPixels) || viewportHeightPixels <= 0
      || !Number.isFinite(height) || !validVector(target) || !validVector(backward) || Math.hypot(...backward) === 0) {
      throw new RangeError('Editor grid requires a finite camera, positive distance and viewport height.');
    }
    const worldPerPixel = 2*distance*Math.tan(Math.PI/8)/viewportHeightPixels;
    const desiredSpacing = Math.max(1e-6, worldPerPixel*32);
    const power = 10**Math.floor(Math.log10(desiredSpacing));
    const ratio = desiredSpacing/power;
    const spacing = power*(ratio >= 5 ? 5 : ratio >= 2 ? 2 : 1);
    const centerX = Math.round(target[0]/spacing);
    const centerZ = Math.round(target[2]/spacing);
    const key = [spacing, centerX, centerZ, worldPerPixel, height, ...backward].join(',');
    if(key === this.key) {
      return this.cached;
    }

    const triangles: number[] = [];
    const extent = HALF_LINE_COUNT*spacing;
    for(let line = -HALF_LINE_COUNT; line <= HALF_LINE_COUNT; line++) {
      const fade = 1 - 0.7*Math.abs(line)/HALF_LINE_COUNT;
      for(const alongX of [true, false]) {
        const division = (alongX ? centerZ : centerX) + line;
        const position = division*spacing;
        const axis = division === 0;
        const major = division%5 === 0;
        const color = axis ? alongX ? X_AXIS_COLOR : Z_AXIS_COLOR : major ? MAJOR_COLOR : MINOR_COLOR;
        const halfWidth = worldPerPixel*(axis ? 0.9 : major ? 0.65 : 0.4);
        const alpha = (axis ? 0.9 : major ? 0.75 : 0.55)*fade;
        const start = alongX ? [centerX*spacing - extent, height, position] : [position, height, centerZ*spacing - extent];
        const end = alongX ? [centerX*spacing + extent, height, position] : [position, height, centerZ*spacing + extent];
        appendLine(triangles, start, end, backward, halfWidth, color, alpha);
      }
    }
    const packed = new Float32Array(triangles);
    if(!packed.every(Number.isFinite)) {
      throw new RangeError('Editor grid coordinates must fit finite float32 values.');
    }
    this.key = key;
    this.cached = {triangles: packed, spacing};
    return this.cached;
  }
}

/** Create two camera-facing triangles for a segment; edge-on lines can project to a point. */
function appendLine(out: number[], start: number[], end: number[], backward: readonly number[], halfWidth: number, color: number[], alpha: number) {
  const direction = end.map((value, axis) => value - start[axis]);
  const side = [
    direction[1]*backward[2] - direction[2]*backward[1],
    direction[2]*backward[0] - direction[0]*backward[2],
    direction[0]*backward[1] - direction[1]*backward[0]
  ];
  const length = Math.hypot(...side);
  if(length < 1e-10) {
    return;
  }
  const offset = side.map(value => value*halfWidth/length);
  const corners = [
    start.map((value, axis) => value - offset[axis]),
    start.map((value, axis) => value + offset[axis]),
    end.map((value, axis) => value + offset[axis]),
    end.map((value, axis) => value - offset[axis])
  ];
  for(const corner of [0, 1, 2, 0, 2, 3]) {
    out.push(...corners[corner], ...color, alpha);
  }
}

/** Camera vectors are finite triples; holes are invalid too. */
function validVector(vector: readonly number[]) {
  return vector.length === 3 && [0, 1, 2].every(axis => Number.isFinite(vector[axis]));
}
