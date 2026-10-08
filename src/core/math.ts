// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Vector2 } from './types.ts';

export const TAU = Math.PI*2;

/**
 * Constrain a number to an inclusive range.
 * @param value
 * @param minimum
 * @param maximum
 */
export function clamp(value: number, minimum: number, maximum: number): number {
  return Math.max(minimum, Math.min(maximum, value));
}

/**
 * Measure Euclidean distance between two points in the caller's coordinate space.
 * @param first
 * @param second
 */
export function distanceBetween(first: Vector2, second: Vector2): number {
  return Math.hypot(second.x - first.x, second.y - first.y);
}

/**
 * Return a unit direction and distance between two x/y positions.
 * Coincident positions return a zero direction and a fallback length of 1,
 * preserving a safe divisor for callers that normalize using the returned length.
 *
 * @param first
 * @param second
 */
export function directionBetween(first: Vector2, second: Vector2): {x: number; y: number; length: number} {
  const x = second.x - first.x;
  const y = second.y - first.y;
  const length = Math.hypot(x, y) || 1;

  return { x: x/length, y: y/length, length };
}

/**
 * Sample a floating-point number from the half-open requested range.
 * @param minimum
 * @param maximum
 * @param [random] Injected source; defaults to gameplay Math.random.
 */
export function randomRange(minimum: number, maximum: number, random: () => number = Math.random): number {
  return minimum + random()*(maximum - minimum);
}

/**
 * Return an integer in [minimum, maximumExclusive); bounds should be integers.
 * @param minimum
 * @param maximumExclusive
 * @param [random] Injected source; defaults to gameplay Math.random.
 */
export function randomInteger(minimum: number, maximumExclusive: number, random: () => number = Math.random): number {
  return Math.floor(randomRange(minimum, maximumExclusive, random));
}

/**
 * Return the distance from a rectangle's center to its edge along a ray.
 * @param width Positive rectangle width in logical units.
 * @param height Positive rectangle height in logical units.
 * @param angle Ray direction in radians from +X.
 * @returns Distance along the ray in the same logical units.
 */
export function distanceToRectangleEdge(width: number, height: number, angle: number): number {
  return Math.min(
    width/2/Math.abs(Math.cos(angle)),
    height/2/Math.abs(Math.sin(angle)),
  );
}
