// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Authored primitive shape names shared by level rendering and physics.
 * Foundation-owned so simulation, content and rendering can all name shapes without upward imports.
 */
export const PRIMITIVE_SHAPE = Object.freeze({
  BOX: 'box',
  SPHERE: 'sphere',
  PLANE: 'plane'
} as const);
