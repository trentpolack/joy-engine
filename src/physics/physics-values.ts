// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Quaternion } from 'cannon-es';

import {Vec3} from 'cannon-es';

/**
 * Validate finite scalar configuration once at the public boundary.
 * @param value
 * @param name
 * @param [minimum]
 * @param [maximum]
 */
export function finiteNumber(value: number, name: string, minimum: number = -Infinity, maximum: number = Infinity) {
  if(!Number.isFinite(value) || value < minimum || value > maximum) {
    throw new RangeError(`${name} must be finite and between ${minimum} and ${maximum}.`);
  }

  return value;
}

/**
 * Copy a world-space XYZ array into a solver vector.
 * @param value
 * @param name
 */
export function vector(value: readonly number[], name: string) {
  if(!Array.isArray(value) || value.length !== 3) {
    throw new TypeError(`${name} must contain three finite numbers.`);
  }
  // Indexed reads reject sparse holes instead of allowing Vec3 defaults to hide them.
  const x = finiteNumber(value[0], name);
  const y = finiteNumber(value[1], name);
  const z = finiteNumber(value[2], name);
  return new Vec3(x, y, z);
}

/**
 * Return XYZ Euler radians applied X, then Y, then Z (matrix Rz*Ry*Rx).
 * At gimbal lock the equivalent canonical rotation fixes X to zero.
 * @param quaternion
 */
export function eulerRotation(quaternion: Quaternion): [
    number,
    number,
    number
] {
  const {x, y, z, w} = quaternion;
  const matrix11 = 1 - 2*(y*y + z*z);
  const matrix21 = 2*(x*y + z*w);
  const matrix31 = 2*(x*z - y*w);
  const cosineY = Math.hypot(matrix11, matrix21);
  const rotationY = Math.atan2(-matrix31, cosineY);
  if(cosineY > 1e-12) {
    const rotationX = Math.atan2(2*(y*z + x*w), 1 - 2*(x*x + y*y));
    const rotationZ = Math.atan2(matrix21, matrix11);
    return [rotationX, rotationY, rotationZ];
  }

  const rotationZ = Math.atan2(2*(z*w - x*y), 1 - 2*(x*x + z*z));
  return([0, rotationY, rotationZ]);
}
