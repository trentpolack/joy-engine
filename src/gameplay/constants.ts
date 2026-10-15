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

/**
 * Authored component types interpreted by engine runtimes (`LevelScene`, `AssetScene`, `LevelRuntime`).
 * Projects may author additional component types; the engine preserves them as data.
 */
export const COMPONENT_TYPE = Object.freeze({
  RENDER: 'render',
  LIGHT: 'light',
  PHYSICS: 'physics',
  VEHICLE: 'vehicle',
  EFFECT: 'effect'
} as const);

/**
 * Built-in component presets: named starting records offered when an author adds a component.
 */
export const COMPONENT_PRESET = Object.freeze({
  RENDER_PRIMITIVE: 'render.primitive',
  RENDER_MESH: 'render.mesh',
  LIGHT_POINT: 'light.point',
  PHYSICS_BOX: 'physics.box',
  PHYSICS_SPHERE: 'physics.sphere',
  VEHICLE_FOUR_WHEEL: 'vehicle.four-wheel',
  EFFECT_PARTICLES: 'effect.particles'
} as const);

/**
 * Value shapes a component field schema can describe. Authoring tools choose controls from these;
 * validation checks authored JSON against them.
 */
export const COMPONENT_FIELD_KIND = Object.freeze({
  NUMBER: 'number',
  BOOLEAN: 'boolean',
  TEXT: 'text',
  CHOICE: 'choice',
  VECTOR3: 'vector3',
  COLOR: 'color',
  ASSET: 'asset',
  RECORD: 'record',
  LIST: 'list'
} as const);

/**
 * Authoring groups for component schemas; values are display labels.
 */
export const COMPONENT_CATEGORY = Object.freeze({
  RENDERING: 'Rendering',
  LIGHTING: 'Lighting',
  PHYSICS: 'Physics',
  EFFECTS: 'Effects',
  GAMEPLAY: 'Gameplay'
} as const);

/**
 * Workspace asset kinds that component asset fields may reference; each kind is also its file extension.
 */
export const COMPONENT_ASSET_KIND = Object.freeze({
  FORM: 'form',
  FORM_LAB: 'formlab',
  PARTICLE_EFFECT: 'joyfx'
} as const);
