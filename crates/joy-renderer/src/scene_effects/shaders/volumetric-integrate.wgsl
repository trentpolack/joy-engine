// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Volumetric integration: march each froxel column from the eye outward, accumulating the light the medium scatters
// toward the eye and the transmittance so far, with each slice integrated analytically (Hillaire 2015) so thick slices
// conserve energy. Every froxel stores the result at its far boundary. Composed after volumetric-common.wgsl.

@group(0) @binding(0) var<uniform> volumetric: Volumetric;
@group(0) @binding(1) var scatterVolume: texture_3d<f32>;
@group(0) @binding(2) var integratedVolume: texture_storage_3d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn integrateMain(@builtin(global_invocation_id) id: vec3u) {
  let grid = vec3u(volumetric.grid.xyz);
  if(any(id.xy >= grid.xy)) {
    return;
  }

  // Distance along the view ray per unit of view depth, constant along the column.
  let coordinate = (vec2f(id.xy) + vec2f(0.5))/volumetric.grid.xy;
  let probe = viewPositionAt(volumetric.inverseProjection, coordinate, 1.0);
  let rayScale = length(probe)/max(-probe.z, 0.0001);

  var scattered = vec3f(0.0);
  var transmittance = 1.0;
  for(var slice = 0u; slice < grid.z; slice++) {
    let nearDepth = sliceDepth(f32(slice)/volumetric.grid.z, volumetric.grid.w);
    let farDepth = sliceDepth(f32(slice + 1u)/volumetric.grid.z, volumetric.grid.w);
    let sliceLength = (farDepth - nearDepth)*rayScale;
    let medium = textureLoad(scatterVolume, vec3i(vec3u(id.xy, slice)), 0);
    let extinction = max(medium.a, 0.000001);
    let sliceTransmittance = exp(-extinction*sliceLength);
    scattered+= transmittance*((medium.rgb - (medium.rgb*sliceTransmittance))/extinction);
    transmittance*= sliceTransmittance;
    textureStore(integratedVolume, vec3u(id.xy, slice), vec4f(scattered, transmittance));
  }
}
