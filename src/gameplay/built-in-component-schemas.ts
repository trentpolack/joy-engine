// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PRIMITIVE_SHAPE } from '../core/constants.ts';
import { COMPONENT_ASSET_KIND, COMPONENT_CATEGORY, COMPONENT_FIELD_KIND, COMPONENT_PRESET, COMPONENT_TYPE } from './constants.ts';
import type { ComponentFieldSchema, ComponentSchema } from './component-schema.ts';
import { PHYSICS_BODY_DEFAULTS } from '../physics/physics-body.ts';
import { PHYSICS_VEHICLE_DEFAULTS, PHYSICS_WHEEL_DEFAULTS } from '../physics/physics-vehicle.ts';

/**
 * Values `LevelScene` assumes for omitted `render` fields. Size is full dimensions in meters;
 * colors are linear RGB. FORM meshes keep their authored colors unless a tint is supplied.
 */
export const RENDER_COMPONENT_DEFAULTS = Object.freeze({
  size: Object.freeze([1, 1, 1]),
  primitiveColor: Object.freeze([0.95, 0.36, 0.12]),
  meshTint: Object.freeze([1, 1, 1])
});

/**
 * Values `LevelScene` assumes for omitted `light` fields. Range is meters; intensity scales linear color.
 */
export const LIGHT_COMPONENT_DEFAULTS = Object.freeze({
  enabled: true,
  color: Object.freeze([1, 1, 1]),
  intensity: 1,
  range: 10,
  castsShadow: true
});

/**
 * Values `AssetScene` assumes for omitted `effect` fields. The seed selects a deterministic cosmetic sequence.
 */
export const EFFECT_COMPONENT_DEFAULTS = Object.freeze({
  enabled: true,
  loop: false,
  seed: 7777
});

// Shared choice lists; render primitives cannot be planes, colliders can.
const RENDER_SHAPES = [PRIMITIVE_SHAPE.BOX, PRIMITIVE_SHAPE.SPHERE];
const PHYSICS_SHAPES = [PRIMITIVE_SHAPE.BOX, PRIMITIVE_SHAPE.SPHERE, PRIMITIVE_SHAPE.PLANE];

/**
 * Primitive or FORM mesh presentation for `LevelScene` and `AssetScene`.
 */
const RENDER_SCHEMA: ComponentSchema = {
  type: COMPONENT_TYPE.RENDER,
  label: 'Render',
  description: 'Draws a primitive shape, or a FORM mesh when an asset is assigned. Entity scale multiplies size.',
  category: COMPONENT_CATEGORY.RENDERING,
  fields: [
    {
      name: 'asset', kind: COMPONENT_FIELD_KIND.ASSET, label: 'FORM mesh',
      description: 'Replaces the primitive shape with a compiled FORM or FORM LAB mesh.',
      assetKinds: [COMPONENT_ASSET_KIND.FORM, COMPONENT_ASSET_KIND.FORM_LAB]
    },
    {
      name: 'parameters', kind: COMPONENT_FIELD_KIND.RECORD,
      description: 'FORM parameter overrides; omitted parameters use the script defaults.'
    },
    {
      name: 'shape', kind: COMPONENT_FIELD_KIND.CHOICE, choices: RENDER_SHAPES,
      description: 'Primitive drawn when no FORM mesh is assigned.'
    },
    {
      name: 'size', kind: COMPONENT_FIELD_KIND.VECTOR3, unit: 'm', step: 0.25,
      default: [...RENDER_COMPONENT_DEFAULTS.size],
      description: 'Full primitive dimensions before entity scale.'
    },
    {
      name: 'color', kind: COMPONENT_FIELD_KIND.COLOR, min: 0,
      description: 'Linear RGB tint. Omitted, primitives draw orange and FORM meshes keep their own colors.'
    }
  ],
  presets: [
    {id: COMPONENT_PRESET.RENDER_PRIMITIVE, label: 'Primitive', data: {shape: PRIMITIVE_SHAPE.BOX, size: [1, 1, 1], color: [0.93, 0.42, 0.22]}},
    {id: COMPONENT_PRESET.RENDER_MESH, label: 'FORM Mesh', data: {asset: '', parameters: {}}}
  ]
};

/**
 * World-space point light derived by `LevelScene` from the entity position.
 */
const LIGHT_SCHEMA: ComponentSchema = {
  type: COMPONENT_TYPE.LIGHT,
  label: 'Point Light',
  description: 'Illuminates the scene from the entity position. Rotation and scale are ignored.',
  category: COMPONENT_CATEGORY.LIGHTING,
  fields: [
    {name: 'enabled', kind: COMPONENT_FIELD_KIND.BOOLEAN, default: LIGHT_COMPONENT_DEFAULTS.enabled},
    {name: 'color', kind: COMPONENT_FIELD_KIND.COLOR, min: 0, default: [...LIGHT_COMPONENT_DEFAULTS.color], description: 'Linear RGB before intensity.'},
    {name: 'intensity', kind: COMPONENT_FIELD_KIND.NUMBER, unit: 'linear', min: 0, step: 0.25, default: LIGHT_COMPONENT_DEFAULTS.intensity},
    {name: 'range', kind: COMPONENT_FIELD_KIND.NUMBER, unit: 'm', min: 0, minExclusive: true, step: 0.5, default: LIGHT_COMPONENT_DEFAULTS.range},
    {name: 'castsShadow', kind: COMPONENT_FIELD_KIND.BOOLEAN, label: 'casts shadow', default: LIGHT_COMPONENT_DEFAULTS.castsShadow}
  ],
  presets: [
    {id: COMPONENT_PRESET.LIGHT_POINT, label: 'Point Light', data: {enabled: true, color: [1, 0.88, 0.7], intensity: 4, range: 12, castsShadow: true}}
  ]
};

/**
 * Rigid body created by `LevelRuntime`. Collider dimensions are local; entity scale is applied once at spawn.
 */
const PHYSICS_SCHEMA: ComponentSchema = {
  type: COMPONENT_TYPE.PHYSICS,
  label: 'Physics',
  description: 'Creates a rigid body. Mass 0 is static; colliders are authored separately from render meshes.',
  category: COMPONENT_CATEGORY.PHYSICS,
  fields: [
    {name: 'shape', kind: COMPONENT_FIELD_KIND.CHOICE, choices: PHYSICS_SHAPES, required: true},
    {name: 'mass', kind: COMPONENT_FIELD_KIND.NUMBER, unit: 'kg', min: 0, step: 1, default: PHYSICS_BODY_DEFAULTS.mass, description: '0 makes the body static; planes must be static.'},
    {
      name: 'halfExtents', kind: COMPONENT_FIELD_KIND.VECTOR3, label: 'half extents', unit: 'm', min: 0, minExclusive: true, step: 0.25,
      default: [...PHYSICS_BODY_DEFAULTS.halfExtents], when: {field: 'shape', equals: [PRIMITIVE_SHAPE.BOX]}
    },
    {
      name: 'radius', kind: COMPONENT_FIELD_KIND.NUMBER, unit: 'm', min: 0, minExclusive: true, step: 0.25,
      default: PHYSICS_BODY_DEFAULTS.radius, when: {field: 'shape', equals: [PRIMITIVE_SHAPE.SPHERE]},
      description: 'Spheres require uniform entity scale.'
    },
    {name: 'friction', kind: COMPONENT_FIELD_KIND.NUMBER, min: 0, step: 0.05, default: PHYSICS_BODY_DEFAULTS.friction},
    {name: 'restitution', kind: COMPONENT_FIELD_KIND.NUMBER, min: 0, max: 1, step: 0.05, default: PHYSICS_BODY_DEFAULTS.restitution},
    {name: 'linearDamping', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'linear damping', min: 0, max: 1, step: 0.01, default: PHYSICS_BODY_DEFAULTS.linearDamping},
    {name: 'angularDamping', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'angular damping', min: 0, max: 1, step: 0.01, default: PHYSICS_BODY_DEFAULTS.angularDamping}
  ],
  presets: [
    {id: COMPONENT_PRESET.PHYSICS_BOX, label: 'Box Collider', data: {shape: PRIMITIVE_SHAPE.BOX, mass: 0, halfExtents: [0.5, 0.5, 0.5]}},
    {id: COMPONENT_PRESET.PHYSICS_SPHERE, label: 'Sphere Collider', data: {shape: PRIMITIVE_SHAPE.SPHERE, mass: 0, radius: 0.5}}
  ]
};

/**
 * One raycast wheel, in chassis-local meters.
 */
const WHEEL_FIELDS: readonly ComponentFieldSchema[] = [
  {name: 'position', kind: COMPONENT_FIELD_KIND.VECTOR3, unit: 'm', step: 0.05, required: true},
  {name: 'radius', kind: COMPONENT_FIELD_KIND.NUMBER, unit: 'm', min: 0, minExclusive: true, step: 0.05, default: PHYSICS_WHEEL_DEFAULTS.radius},
  {name: 'steering', kind: COMPONENT_FIELD_KIND.BOOLEAN, default: PHYSICS_WHEEL_DEFAULTS.steering},
  {name: 'driven', kind: COMPONENT_FIELD_KIND.BOOLEAN, default: PHYSICS_WHEEL_DEFAULTS.driven},
  {name: 'brake', kind: COMPONENT_FIELD_KIND.BOOLEAN, default: PHYSICS_WHEEL_DEFAULTS.brake},
  {name: 'suspensionRestLength', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'suspension rest length', unit: 'm', min: 0, step: 0.05, default: PHYSICS_WHEEL_DEFAULTS.suspensionRestLength},
  {name: 'suspensionStiffness', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'suspension stiffness', min: 0, step: 1, default: PHYSICS_WHEEL_DEFAULTS.suspensionStiffness},
  {name: 'dampingCompression', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'damping compression', min: 0, step: 0.1, default: PHYSICS_WHEEL_DEFAULTS.dampingCompression},
  {name: 'dampingRelaxation', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'damping relaxation', min: 0, step: 0.1, default: PHYSICS_WHEEL_DEFAULTS.dampingRelaxation},
  {name: 'maxSuspensionTravel', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'max suspension travel', unit: 'm', min: 0, step: 0.05, default: PHYSICS_WHEEL_DEFAULTS.maxSuspensionTravel},
  {name: 'maxSuspensionForce', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'max suspension force', unit: 'N', min: 0, step: 100, default: PHYSICS_WHEEL_DEFAULTS.maxSuspensionForce},
  {name: 'frictionSlip', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'friction slip', min: 0, step: 0.1, description: 'Defaults to the vehicle friction slip.'},
  {name: 'rollInfluence', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'roll influence', min: 0, max: 1, step: 0.01, default: PHYSICS_WHEEL_DEFAULTS.rollInfluence}
];

/**
 * Raycast vehicle attached by `LevelRuntime` to the entity's physics body.
 */
const VEHICLE_SCHEMA: ComponentSchema = {
  type: COMPONENT_TYPE.VEHICLE,
  label: 'Vehicle',
  description: 'Raycast suspension and drive on the physics body. Requires uniform entity scale.',
  category: COMPONENT_CATEGORY.PHYSICS,
  requires: [COMPONENT_TYPE.PHYSICS],
  fields: [
    {name: 'wheels', kind: COMPONENT_FIELD_KIND.LIST, required: true, items: WHEEL_FIELDS},
    {name: 'engineForce', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'engine force', unit: 'N', min: 0, step: 10, default: PHYSICS_VEHICLE_DEFAULTS.engineForce},
    {name: 'maxSteer', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'max steer', unit: 'rad', min: 0, max: Math.PI/2, step: 0.05, default: PHYSICS_VEHICLE_DEFAULTS.maxSteer},
    {name: 'brakeForce', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'brake force', unit: 'N', min: 0, step: 1, default: PHYSICS_VEHICLE_DEFAULTS.brakeForce},
    {name: 'frictionSlip', kind: COMPONENT_FIELD_KIND.NUMBER, label: 'friction slip', min: 0, step: 0.1, default: PHYSICS_VEHICLE_DEFAULTS.frictionSlip}
  ],
  presets: [
    {
      id: COMPONENT_PRESET.VEHICLE_FOUR_WHEEL, label: 'Four-Wheel Vehicle',
      data: {
        wheels: [
          {position: [-0.75, -0.05, 0.75], steering: true, brake: true},
          {position: [0.75, -0.05, 0.75], steering: true, brake: true},
          {position: [-0.75, -0.05, -0.75], driven: true, brake: true},
          {position: [0.75, -0.05, -0.75], driven: true, brake: true}
        ]
      }
    }
  ]
};

/**
 * Particle effect simulated per entity by `AssetScene`.
 */
const EFFECT_SCHEMA: ComponentSchema = {
  type: COMPONENT_TYPE.EFFECT,
  label: 'Particle Effect',
  description: 'Plays a JOYFX effect attached to the entity transform.',
  category: COMPONENT_CATEGORY.EFFECTS,
  fields: [
    {name: 'asset', kind: COMPONENT_FIELD_KIND.ASSET, label: 'JOYFX effect', required: true, assetKinds: [COMPONENT_ASSET_KIND.PARTICLE_EFFECT]},
    {name: 'enabled', kind: COMPONENT_FIELD_KIND.BOOLEAN, default: EFFECT_COMPONENT_DEFAULTS.enabled},
    {name: 'loop', kind: COMPONENT_FIELD_KIND.BOOLEAN, default: EFFECT_COMPONENT_DEFAULTS.loop, description: 'Restart once every particle has expired.'},
    {name: 'seed', kind: COMPONENT_FIELD_KIND.NUMBER, integer: true, step: 1, default: EFFECT_COMPONENT_DEFAULTS.seed},
    {name: 'parameters', kind: COMPONENT_FIELD_KIND.RECORD, description: 'Effect parameter overrides; omitted parameters use the effect defaults.'}
  ],
  presets: [
    {id: COMPONENT_PRESET.EFFECT_PARTICLES, label: 'JOYFX Effect', data: {asset: '', enabled: true, loop: true, seed: 7777, parameters: {}}}
  ]
};

/**
 * Schemas for every component the engine interprets, in authoring display order.
 */
export const BUILT_IN_COMPONENT_SCHEMAS: readonly ComponentSchema[] = Object.freeze([
  RENDER_SCHEMA,
  LIGHT_SCHEMA,
  EFFECT_SCHEMA,
  PHYSICS_SCHEMA,
  VEHICLE_SCHEMA
]);
