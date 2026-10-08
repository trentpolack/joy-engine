// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Built-in entity factories; projects may register additional names.
 */
export const ENTITY_TYPE = Object.freeze({
  ENTITY: 'entity',
  CHARACTER: 'character',
  PLAYER_CHARACTER: 'player-character'
} as const);

/**
 * Transform component keys used by level authoring and validation.
 */
export const TRANSFORM_PROPERTY = Object.freeze({
  POSITION: 'position',
  ROTATION: 'rotation',
  SCALE: 'scale'
} as const);
