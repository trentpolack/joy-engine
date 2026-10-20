// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Volumetric fog over opaque surfaces and the sky: each pixel's view depth (the volume's far edge for the sky) looks up
// the integrated volume, and the pipeline blends the scattered light over the scene attenuated by the transmittance.
// Transparent surfaces apply fog themselves (`applyFog` in scene-common.wgsl). Composed after fullscreen.wgsl.

struct FogApply {
  // This frame's raster (jittered) inverse projection, which recovers view depth from the depth buffer.
  inverseProjection: mat4x4f,
  // X the view depth the volume covers, YZW unused.
  extent: vec4f,
};

@group(0) @binding(0) var<uniform> apply: FogApply;
@group(0) @binding(1) var depthTexture: texture_depth_2d;
@group(0) @binding(2) var integratedVolume: texture_3d<f32>;
@group(0) @binding(3) var volumeSampler: sampler;

@fragment fn fragmentMain(input: FullscreenVarying) -> @location(0) vec4f {
  let depth = textureLoad(depthTexture, vec2i(input.position.xy), 0);
  var viewDepth = apply.extent.x;
  if(depth < 1.0) {
    let ndc = vec2f((input.coordinate.x*2.0) - 1.0, 1.0 - (input.coordinate.y*2.0));
    let view = apply.inverseProjection*vec4f(ndc, depth, 1.0);
    viewDepth = -view.z/view.w;
  }
  let slices = f32(textureDimensions(integratedVolume).z);
  let slice = sqrt(clamp(viewDepth/apply.extent.x, 0.0, 1.0));
  let coordinate = vec3f(input.coordinate, max(slice - (0.5/slices), 0.5/slices));
  let fog = textureSampleLevel(integratedVolume, volumeSampler, coordinate, 0.0);
  return mix(vec4f(0.0, 0.0, 0.0, 1.0), fog, clamp(slice*slices, 0.0, 1.0));
}
