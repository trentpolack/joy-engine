// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// SceneShadows remaps light raster depth to WebGPU [0, 1]. Cube comparisons
// reconstruct that same projection depth from the major-axis light distance.
fn shadow_pointReferenceDepth(majorDistance: f32, nearPlane: f32, farPlane: f32) -> f32 {
  return farPlane/(farPlane - nearPlane) - (farPlane*nearPlane)/((farPlane - nearPlane)*majorDistance);
}
