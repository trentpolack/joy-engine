// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { FORM_ATTRIBUTE_DOMAIN } from './constants.ts';
import type { GeometryValue, GeometryStore } from './procedural-geometry.ts';
import { integerRange } from './geometry.ts';
export type { GeometryValue };
export type { GeometryStore };

/** Revolve ordered (radius, height, 0) points about Y. Bottom-to-top profiles face outward.
 * Returns owned geometry, open at nonzero-radius ends. Input topology/attributes are not copied.
 * @param store @param profile @param segments
 */
export function revolveGeometry(store: GeometryStore, profile: GeometryValue, segments: number) {
  integerRange(segments, 3, 96);
  const points = orderedPoints(profile);
  if(points.some(point => point[0] < 0 || point[2] !== 0)) {
    throw new Error('Revolve profiles use (nonnegative radius, height, 0).');
  }
  const result = store.create();

  const rings: number[][] = [];
  for(const [radius, height] of points) {
    if(radius === 0) {
      const pole = store.addPoint(result, [0, height, 0]);
      rings.push(Array(segments).fill(pole));
    } else {
      const ring = [];
      for(let side = 0; side < segments; side++) {
        const angle = side/segments*Math.PI*2;
        ring.push(store.addPoint(result, [Math.cos(angle)*radius, height, Math.sin(angle)*radius]));
      }
      rings.push(ring);
    }
  }
  connectRings(store, result, rings);
  writeSmoothNormals(store, result);
  return result;
}

/** Sweep a circular section along ordered XYZ points. Projected frames minimize twisting.
 * Open ends; sharp bends may self-intersect. Exact reversals are rejected.
 * @param store @param path @param radius @param sides
 */
export function tubeGeometry(store: GeometryStore, path: GeometryValue, radius: number, sides: number) {
  integerRange(sides, 3, 64);
  if(!Number.isFinite(radius) || radius <= 0 || radius > 1e6) {
    throw new Error('Tube radius must be positive and at most 1,000,000.');
  }
  const points = orderedPoints(path);
  const result = store.create();

  const rings: number[][] = [];

  let radial: number[] | null = null;
  for(let row = 0; row < points.length; row++) {
    const before = normalize(subtract(points[row], points[Math.max(0, row - 1)]));
    const after = normalize(subtract(points[Math.min(points.length - 1, row + 1)], points[row]));
    const tangent = normalize(before.map((value, axis) => value + after[axis]));
    if(Math.hypot(...tangent) < 0.5) {
      throw new Error('Tube paths cannot reverse direction at a point.');
    }
    const previous = radial ?? (Math.abs(tangent[0]) < 0.8 ? [1, 0, 0] : [0, 0, 1]);
    const alignment = dot(previous, tangent);
    radial = normalize(previous.map((value, axis) => value - tangent[axis]*alignment));
    if(Math.hypot(...radial) < 0.5) {
      radial = normalize(cross(tangent, Math.abs(tangent[0]) < 0.8 ? [1, 0, 0] : [0, 0, 1]));
    }
    const frameRadial = radial;
    const binormal = cross(frameRadial, tangent);
    const ring = [];
    for(let side = 0; side < sides; side++) {
      const angle = side/sides*Math.PI*2;
      ring.push(store.addPoint(result, points[row].map((value, axis) => value + radius*(frameRadial[axis]*Math.cos(angle) + binormal[axis]*Math.sin(angle)))));
    }
    rings.push(ring);
  }
  connectRings(store, result, rings);
  writeSmoothNormals(store, result);
  return result;
}

/** Area-weighted point normals; preserves topology and non-normal attributes.
 * Degenerate faces contribute nothing; isolated points receive a zero normal.
 * @param store @param geometry
 */
export function writeSmoothNormals(store: GeometryStore, geometry: GeometryValue) {
  const points = ((geometry.attributes.points.position.values) as number[][]);
  const normals = points.map(() => [0, 0, 0]);
  for(const face of geometry.faces) {
    const normal = cross(subtract(points[face[1]], points[face[0]]), subtract(points[face[2]], points[face[0]]));
    for(const index of face) {
      for(let axis = 0; axis < 3; axis++) {
        normals[index][axis]+= normal[axis];
      }
    }
  }
  delete geometry.attributes.faces.normal;
  delete geometry.attributes.corners.normal;
  for(let index = 0; index < normals.length; index++) {
    store.set(geometry, FORM_ATTRIBUTE_DOMAIN.POINTS, 'normal', index, normalize(normals[index]));
  }
}

/** Bound expansion before generating rings; never weld the UV seam into authoring topology.
 * @param geometry */
function orderedPoints(geometry: GeometryValue): number[][] {
  const points = ((geometry.attributes.points.position.values) as number[][]);
  if(points.length < 2 || points.length > 512) {
    throw new Error('Profiles and paths require 2–512 ordered points.');
  }
  for(let index = 1; index < points.length; index++) {
    if(Math.hypot(...subtract(points[index], points[index - 1])) < 1e-8) {
      throw new Error('Adjacent profile/path points must be distinct.');
    }
  }
  return points;
}
/** @param store @param geometry @param rings */
function connectRings(store: GeometryStore, geometry: GeometryValue, rings: number[][]) {
  const sides = rings[0].length;
  // Pole rings repeat one point; skip their collapsed triangles while retaining the UV seam.
  /** @param indices @param uv */
  const face = (indices: number[], uv: number[][]) => {
    if(new Set(indices).size < 3) {
      return;
    }
    const index = store.addFace(geometry, indices);
    for(let corner = 0; corner < 3; corner++) {
      store.set(geometry, FORM_ATTRIBUTE_DOMAIN.CORNERS, 'uv', index*3 + corner, uv[corner]);
    }
  };
  for(let row = 0; row < rings.length - 1; row++) {
    const v = row/(rings.length - 1),
      w = (row + 1)/(rings.length - 1);
    for(let side = 0; side < sides; side++) {
      const next = (side + 1)%sides,
        u = side/sides,
        t = (side + 1)/sides;
      const a = rings[row][side],
        b = rings[row][next],
        c = rings[row + 1][side],
        d = rings[row + 1][next];
      face([a, c, b], [[u,v,0], [u,w,0], [t,v,0]]);
      face([b, c, d], [[t,v,0], [u,w,0], [t,w,0]]);
    }
  }
}
/** @param a @param b */
function subtract(a: number[], b: number[]) {
  return a.map((value, axis) => value - b[axis]);
}
/** @param a @param b */
function dot(a: number[], b: number[]) {
  return a[0]*b[0] + a[1]*b[1] + a[2]*b[2];
}
/** @param a @param b */
function cross(a: number[], b: number[]) {
  return [a[1]*b[2] - a[2]*b[1], a[2]*b[0] - a[0]*b[2], a[0]*b[1] - a[1]*b[0]];
}
/** @param value */
function normalize(value: number[]) {
  const length = Math.hypot(...value);
  return length > 1e-12 ? value.map(component => component/length) : [0, 0, 0];
}
