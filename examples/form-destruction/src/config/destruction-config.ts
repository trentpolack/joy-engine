// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Dimensions use meters, gravity meters/second², launch velocity meters/second, and physics steps seconds.
 */
export const DESTRUCTION_CONFIG = Object.freeze({
  seed: 7319,
  chunkSize: [1, 1, 0.55] as const,
  wallColumns: 13,
  wallRows: 6,
  hitPoints: 6,
  frameHitPoints: 10,
  damageRadius: 1.15,
  damagePerHit: 1,
  maximumAimDistance: 60,
  debrisGravity: 24,
  debrisMass: 1,
  fracturedChunkScale: 0.9,
  debrisFriction: 0.7,
  debrisRestitution: 0.08,
  debrisLinearDamping: 0.12,
  debrisAngularDamping: 0.25,
  debrisLaunchSpeed: [0.8, 0.25, 1.8] as const,
  debrisLaunchVariation: [0.35, 0.08] as const,
  debrisSpinImpulse: [0.2, 0, 0.25] as const,
  physicsStepSeconds: 1/120,
  maximumPhysicsSubsteps: 6
});

/**
 * Ground dimensions are meters. The rendered slab and static collider share these values.
 */
export const GROUND_CONFIG = Object.freeze({
  center: [0, -1, 0] as const,
  size: [200, 2, 200] as const,
  tint: [0.3, 0.36, 0.42] as const,
  friction: 0.8,
  restitution: 0.08
});

/**
 * Grounded masonry can bridge one neighboring chunk; longer unsupported spans detach.
 */
export const SUPPORT_CONFIG = Object.freeze({
  groundHeight: GROUND_CONFIG.center[1] + GROUND_CONFIG.size[1]/2,
  maximumCantileverDistance: 1.05,
  contactTolerance: 0.0001,
  anchoredColor: [0.08, 0.6, 0.85] as const,
  verticalColor: [0.15, 0.8, 0.35] as const,
  bracedColor: [1, 0.65, 0.08] as const
});

export const STRUCTURE_VIEW = Object.freeze({material: 'material', integrity: 'integrity', support: 'support'} as const);
export type StructureView = typeof STRUCTURE_VIEW[keyof typeof STRUCTURE_VIEW];

/**
 * Integrity colors visualize remaining resistance in damage units, independent of surface materials.
 */
export const INTEGRITY_CONFIG = Object.freeze({
  enabled: true,
  meshInset: 0.985,
  criticalColor: [1, 0.06, 0.025] as const,
  weakenedColor: [1, 0.64, 0.08] as const,
  intactColor: [0.08, 0.6, 0.85] as const,
  reinforcedColor: [0.2, 0.3, 0.85] as const,
  ambient: 0.7,
  light: {x: 5, y: 12, z: 9, r: 2, g: 2, b: 2, range: 45}
});

/**
 * World-space impact diagnostics. Bounds and lifetime keep repeated clicks inexpensive.
 */
export const IMPACT_DEBUG_CONFIG = Object.freeze({
  enabled: true,
  maximumMarkers: 8,
  lifetimeSeconds: 6,
  fadeSeconds: 1,
  ringSegments: 48,
  lineWidth: 0.025,
  pointSize: 0.18,
  rayLength: 3,
  surfaceOffset: 0.025,
  radiusColor: [0.15, 0.85, 1] as const,
  rayColor: [1, 0.85, 0.15] as const,
  pointColor: [1, 1, 1] as const
});

/**
 * Camera angles use radians; drag distance and orbit sensitivity use CSS pixels.
 */
export const VIEW_CONFIG = Object.freeze({
  target: [0, 2.1, -0.5] as const,
  distance: 15,
  radius: 9,
  yaw: 0.05,
  pitch: 0.22,
  nearClip: 0.1,
  farClip: 300,
  minimumDistance: 6,
  maximumDistance: 28,
  minimumPitch: 0.03,
  maximumPitch: 1.25,
  orbitSensitivity: 0.004,
  zoomSensitivity: 0.001,
  dragThresholdPixels: 4,
  maximumPixelRatio: 2
});

export const PRESENTATION_CONFIG = Object.freeze({
  clearColor: [0.025, 0.035, 0.045, 1] as const,
  ambient: 0.42,
  primaryLight: {x: 7, y: 14, z: 9, r: 5, g: 3.2, b: 1.8, range: 45},
  secondaryLight: {x: -7, y: 5, z: -4, r: 0.25, g: 0.7, b: 1.2, range: 24},
  maximumFrameSeconds: 0.05,
  timingSampleCount: 180,
  hudIntervalFrames: 10
});
