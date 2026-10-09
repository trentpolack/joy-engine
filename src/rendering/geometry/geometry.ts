// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Color, ShadedMesh3D } from '../../core/types.ts';

import { TAU } from '../../core/math.ts';

// Shared vertex contract: logical x/y/z position followed by straight-alpha RGBA.
export const FLOATS_PER_VERTEX = 7;

/**
 * Append an equilateral triangle. Radius measures from its center to each corner.
 * Mutates the supplied vertex array; coordinates use the caller's logical units.
 *
 * @param vertices
 * @param x
 * @param y
 * @param radius
 * @param color
 * @param rotation
 * @param z
 */
export function addTriangle(vertices: number[], x: number, y: number, radius: number, color: Color, rotation: number = 0, z: number = 0) {
  for (let index = 0; index < 3; index += 1) {
    const angle = rotation + (index * TAU) / 3;
    vertices.push(
      x + Math.cos(angle) * radius,
      y + Math.sin(angle) * radius, z,
      ...color,
    );
  }
}

/**
 * Append a square as two triangles. Radius measures to a corner, not an edge.
 * Mutates the supplied vertex array; coordinates use the caller's logical units.
 *
 * @param vertices
 * @param x
 * @param y
 * @param radius
 * @param color
 * @param rotation
 * @param z
 */
export function addQuad(vertices: number[], x: number, y: number, radius: number, color: Color, rotation: number = Math.PI / 4, z: number = 0) {
  const corners = [];

  for (let index = 0; index < 4; index += 1) {
    const angle = rotation + (index * TAU) / 4;
    corners.push([
      x + Math.cos(angle) * radius,
      y + Math.sin(angle) * radius,
    ]);
  }

  for (const index of [0, 1, 2, 0, 2, 3]) {
    vertices.push(...corners[index], z, ...color);
  }
}

/**
 * Append a triangle fan approximating a filled circle.
 * Mutates the supplied vertex array; coordinates use the caller's logical units.
 *
 * @param vertices
 * @param x
 * @param y
 * @param radius
 * @param color
 * @param segments
 * @param z
 */
export function addCircle(vertices: number[], x: number, y: number, radius: number, color: Color, segments: number = 12, z: number = 0) {
  for (let index = 0; index < segments; index += 1) {
    const firstAngle = (index/segments)*TAU;
    const secondAngle = ((index + 1)/segments) * TAU;

    vertices.push(x, y, z, ...color,
      x + Math.cos(firstAngle)*radius, y + Math.sin(firstAngle)*radius, z, ...color,
      x + Math.cos(secondAngle)*radius, y + Math.sin(secondAngle)*radius, z, ...color);
  }
}

/**
 * Append one normalized, faceted 3D model to the shared GPU batch.
 * @param vertices
 * @param mesh
 * @param x @param y @param scale
 * @param rotation @param color
 * @param z
 */
export function addMesh(vertices: number[], mesh: ShadedMesh3D, x: number, y: number, scale: number, rotation: number, color: Color, z: number = 0) {
  const cosine = Math.cos(rotation);
  const sine = Math.sin(rotation);
  for (const triangle of mesh.triangles) {
    const shade = triangle[9];
    const shadedColor = (([
      Math.min(1, color[0] * shade), Math.min(1, color[1] * shade),
      Math.min(1, color[2] * shade), color[3],
    ]) as Color);
    for (let index = 0; index < 9; index += 3) {
      const localX = triangle[index] * scale;
      const localY = triangle[index + 1] * scale;
      vertices.push(x + localX * cosine - localY * sine,
        y + localX * sine + localY * cosine,
        z + triangle[index + 2] * scale, ...shadedColor);
    }
  }
}

/**
 * Append a regular polygon using equal radial samples.
 *
 * @param vertices
 * @param x
 * @param y
 * @param radius
 * @param color
 * @param sides
 * @param rotation
 * @param z
 */
export function addPolygon(vertices: number[], x: number, y: number, radius: number, color: Color, sides: number, rotation: number, z: number) {
  addRadialPolygon(vertices, x, y, radius, color, Array(sides).fill(1), rotation, z);
}

/**
 * Append a triangle fan from evenly spaced radial multipliers.
 * A sample of 1 lies at radius; rotation is in radians. Each vertex
 * repeats x/y/z/RGBA because the engine consumes a non-indexed triangle list.
 *
 * @param vertices
 * @param x
 * @param y
 * @param radius
 * @param color
 * @param shape
 * @param rotation
 * @param z
 */
export function addRadialPolygon(vertices: number[], x: number, y: number, radius: number, color: Color, shape: readonly number[], rotation: number, z: number) {
  for (let index = 0; index < shape.length; index += 1) {
    const firstAngle = rotation + index / shape.length * Math.PI * 2;
    const secondAngle = rotation + (index + 1) / shape.length * Math.PI * 2;
    const firstSample = shape[index];
    const secondSample = shape[(index + 1) % shape.length];
    vertices.push(
      x, y, z, ...color,
      x + Math.cos(firstAngle) * radius * firstSample,
      y + Math.sin(firstAngle) * radius * firstSample, z, ...color,
      x + Math.cos(secondAngle) * radius * secondSample,
      y + Math.sin(secondAngle) * radius * secondSample, z, ...color,
    );
  }
}

/**
 * Append an irregular triangle fan with independent center and edge colors.
 * Interpolation creates a portable radial falloff without textures or a
 * particle-specific shader, making the primitive useful for light blooms,
 * smoke, and other soft particles on every renderer backend.
 *
 * @param vertices
 * @param x
 * @param y
 * @param radius
 * @param centerColor
 * @param edgeColor
 * @param shape
 * @param rotation
 * @param z
 */
export function addGradientRadialPolygon(
  vertices: number[], x: number, y: number, radius: number, centerColor: Color, edgeColor: Color, shape: readonly number[], rotation: number, z: number,
) {
  for (let index = 0; index < shape.length; index += 1) {
    const firstAngle = rotation + index / shape.length * Math.PI * 2;
    const secondAngle = rotation + (index + 1) / shape.length * Math.PI * 2;
    vertices.push(
      x, y, z, ...centerColor,
      x + Math.cos(firstAngle) * radius * shape[index],
      y + Math.sin(firstAngle) * radius * shape[index], z, ...edgeColor,
      x + Math.cos(secondAngle) * radius * shape[(index + 1) % shape.length],
      y + Math.sin(secondAngle) * radius * shape[(index + 1) % shape.length], z, ...edgeColor,
    );
  }
}

/**
 * Append an annulus as two triangles per segment.
 * Width is the HALF thickness: the radii are radius - width and radius + width.
 * Clamp the inner radius to zero while the ring is smaller than its thickness.
 *
 * @param vertices
 * @param x
 * @param y
 * @param radius
 * @param width
 * @param color
 * @param segments
 * @param z
 */
export function addRing(vertices: number[], x: number, y: number, radius: number, width: number, color: Color, segments: number, z: number) {
  for (let index = 0; index < segments; index += 1) {
    const firstAngle = index / segments * Math.PI * 2;
    const secondAngle = (index + 1) / segments * Math.PI * 2;
    const corners = [
      { radius: Math.max(0, radius - width), angle: firstAngle },
      { radius: radius + width, angle: firstAngle },
      { radius: radius + width, angle: secondAngle },
      { radius: Math.max(0, radius - width), angle: secondAngle },
    ];
    for (const cornerIndex of [0, 1, 2, 0, 2, 3]) {
      const corner = corners[cornerIndex];
      vertices.push(
        x + Math.cos(corner.angle) * corner.radius,
        y + Math.sin(corner.angle) * corner.radius, z, ...color,
      );
    }
  }
}

/**
 * Append a butt-ended thick segment as two triangles.
 * Width is the full thickness in the caller's logical units; z is constant along the segment.
 *
 * @param vertices
 * @param ax
 * @param ay
 * @param bx
 * @param by
 * @param width
 * @param color
 * @param z
 */
export function addLine(vertices: number[], ax: number, ay: number, bx: number, by: number, width: number, color: Color, z: number) {
  const angle = Math.atan2(by - ay, bx - ax) + Math.PI / 2;
  const offsetX = Math.cos(angle) * width / 2;
  const offsetY = Math.sin(angle) * width / 2;
  const corners = [
    [ax - offsetX, ay - offsetY],
    [ax + offsetX, ay + offsetY],
    [bx + offsetX, by + offsetY],
    [bx - offsetX, by - offsetY],
  ];
  for (const cornerIndex of [0, 1, 2, 0, 2, 3]) {
    const [x, y] = corners[cornerIndex];
    vertices.push(x, y, z, ...color);
  }
}
