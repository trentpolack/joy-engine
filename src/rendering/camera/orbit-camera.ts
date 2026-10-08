// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/** Y-up orbit camera. Distances are world units; angles are radians. */
export class OrbitCamera {
  declare target: number[];
  declare distance: number;
  declare radius: number;
  declare yaw: number;
  declare pitch: number;
  /** Vertical perspective field of view in radians. */
  declare verticalFovRadians: number;
  /** Optional authored clip distances in world units; null retains automatic framing. */
  declare nearClip: number | null;
  declare farClip: number | null;

  constructor() {
    this.target = [0, 0, 0];
    this.distance = 32;
    this.radius = 10;
    this.yaw = 0.75;
    this.pitch = 0.55;
    this.verticalFovRadians = Math.PI/4;
    this.nearClip = null;
    this.farClip = null;
  }
  /** Fit packed XYZ positions in world units; empty input uses a default centered volume.
   * Mutates target, radius and camera distance.
   * @param positions Borrowed flat position array.
   */
  frame(positions: number[]) {
    const min = [Infinity, Infinity, Infinity],
      max = [-Infinity, -Infinity, -Infinity];
    for(let i = 0; i < positions.length; i++) {
      const axis = i % 3;
      min[axis] = Math.min(min[axis], positions[i]);
      max[axis] = Math.max(max[axis], positions[i]);
    }
    if(!positions.length) {
      min.fill(-5);
      max.fill(5);
    }
    this.target = min.map((value, i) => (value + max[i]) / 2);
    this.radius = Math.max(0.1, Math.hypot(...max.map((value, i) => (value - min[i]) / 2)));
    this.distance = this.radius * 2.8;
  }
  /** Return fresh world-space basis vectors and eye position for the current orbit. */
  basis() {
    const backward = [
      Math.cos(this.pitch) * Math.sin(this.yaw),
      Math.sin(this.pitch),
      Math.cos(this.pitch) * Math.cos(this.yaw),
    ];
    const right = [Math.cos(this.yaw), 0, -Math.sin(this.yaw)];
    const up = crossVectors3(backward, right);
    const eye = this.target.map((value, i) => value + backward[i] * this.distance);
    return { backward, right, up, eye };
  }
  /** Build a caller-owned column-major perspective view/projection matrix.
   * @param aspect Logical viewport width divided by height.
   * @param webgpu Use zero-to-one clip depth instead of WebGL minus-one-to-one.
   */
  matrix(aspect: number, webgpu: boolean) {
    return multiply(this.projectionMatrix(aspect, webgpu), this.viewMatrix());
  }

  /** Caller-owned world-to-view matrix, right handed with camera forward along -Z. */
  viewMatrix(): Float32Array<ArrayBuffer> {
    const {right, up, backward, eye} = this.basis();
    return new Float32Array([
      right[0], up[0], backward[0], 0,
      right[1], up[1], backward[1], 0,
      right[2], up[2], backward[2], 0,
      -dot(right, eye), -dot(up, eye), -dot(backward, eye), 1
    ]);
  }

  get nearPlane(): number {
    return this.nearClip ?? Math.max(0.001, this.distance/1000);
  }

  get farPlane(): number {
    return this.farClip ?? (this.distance + this.radius*100);
  }

  /** Caller-owned view-to-clip matrix using the same clip planes as matrix(). */
  projectionMatrix(aspect: number, webgpu: boolean): Float32Array<ArrayBuffer> {
    const near = this.nearPlane;
    const far = this.farPlane;
    if(!Number.isFinite(aspect) || aspect <= 0 || !Number.isFinite(near) || near <= 0 || !Number.isFinite(far) || far <= near || !Number.isFinite(this.verticalFovRadians) || this.verticalFovRadians <= 0 || this.verticalFovRadians >= Math.PI) {
      throw new RangeError('Camera needs a positive aspect, ordered positive clip planes and field of view between zero and pi.');
    }
    const scale = 1/Math.tan(this.verticalFovRadians/2);
    const depth = webgpu ? far/(near - far) : (far + near)/(near - far);
    const offset = webgpu ? (far*near)/(near - far) : (2*far*near)/(near - far);
    return new Float32Array([scale/aspect, 0, 0, 0, 0, scale, 0, 0, 0, 0, depth, -1, 0, 0, offset, 0]);
  }
}
/** @param a @param b */
export function crossVectors3(a: number[], b: number[]) {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}
/** @param a @param b */
function dot(a: number[], b: number[]) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}
/** @param a @param b */
function multiply(a: Float32Array, b: Float32Array) {
  const out = new Float32Array(16);
  for(let column = 0; column < 4; column++) {
    for(let row = 0; row < 4; row++) {
      for(let k = 0; k < 4; k++) {
        out[column * 4 + row] += a[k * 4 + row] * b[column * 4 + k];
      }
    }
  }
  return out;
}

/** Back-to-front centroid sorting for any camera orientation; leaves input untouched.
 * Intersecting transparent surfaces remain an approximation, not order-independent transparency.
 * @param vertices Interleaved XYZ/RGBA triangles.
 * @param backward Unit vector from target toward camera.
 */
export function sortCameraTransparentTriangles(vertices: readonly number[], backward: readonly number[]): number[] {
  if(vertices.length % 21 !== 0) {
    throw new RangeError('Triangle data must contain 21 values per triangle.');
  }
  const triangles = [];
  for(let offset = 0; offset < vertices.length; offset += 21) {
    let depth = 0;
    for(let vertex = 0; vertex < 3; vertex++) {
      for(let axis = 0; axis < 3; axis++) {
        depth += vertices[offset + vertex * 7 + axis] * backward[axis];
      }
    }
    triangles.push({ offset, depth });
  }
  triangles.sort((a, b) => a.depth - b.depth);
  const sorted = [];
  for(const { offset } of triangles) {
    for(let component = 0; component < 21; component++) {
      sorted.push(vertices[offset + component]);
    }
  }
  return sorted;
}
