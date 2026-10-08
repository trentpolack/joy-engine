// Copyright (c) 2026 Trent Polack.
// Licensed under the MIT License.

// Demonstration units only; not source physical dimensions.
export const TERRAIN_CONFIG = Object.freeze({
  manifest: 'dragon-pit/terrain.json',
  tolerance: 2.0,
  targetRadius: 90,

  // Match the page's Joy blue background (#1b293d); leave terrain materials local.
  backgroundColor: /** @type {[number, number, number, number]} */ ([
    0.1058, 0.1607, 0.2392, 1.0,
  ]),
  orbitDistance: 2300,
  orbitPitch: 0.52,
  orbitYaw: 0.8,
  target: [0, 220, 0],
  nearGroundDistance: 100,
  nearGroundPitch: 0.04,
  maxPixelRatio: 2,

  // World units/second for flight; angular sensitivities are radians/CSS pixel.
  cameraControls: Object.freeze({
    flySpeed: 150,
    lookSensitivity: 0.004,
    orbitYawSensitivity: 0.005,
    orbitPitchSensitivity: 0.004,
    minOrbitPitch: 0.01,
    maxOrbitPitch: 1.4,
    zoomSensitivity: 0.001,
    minDistance: 5,
    maxDistance: 10000,
    maxPitch: 1.5,
    wheelLinePixels: 16,

    // Matches OrbitCamera's current perspective projection.
    verticalFovRadians: Math.PI/4,
  })
});
