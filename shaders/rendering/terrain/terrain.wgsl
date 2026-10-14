// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

@vertex fn vertexMain(input: VertexInput) -> VertexOutput {
  return terrainSurface(input);
}

/**
 * Shades the morphed terrain surface and optional LOD diagnostics.
 * Normals come from the rendered triangle derivatives, ensuring lighting follows the same continuously morphed geometry seen on screen.
 */
@fragment fn fragmentMain(input:VertexOutput) -> @location(0) vec4<f32> {
  // Derivatives follow the actual morphed triangle, without a normal texture.  Orient upward because either winding can be visible with culling disabled.
  var n = normalize(cross(dpdx(input.worldPosition), dpdy(input.worldPosition)));
  if(n.y < 0.0) {
    n = -n;
  }

  // Simple slope/altitude coloring and diffuse light; this is not a PBR material.
  let slope = 1.0 - n.y;
  var color = mix(vec3<f32>(0.21, 0.36, 0.26), vec3<f32>(0.49, 0.43, 0.36), smoothstep(0.1, 0.65, slope));
  color = mix(color, vec3<f32>(0.79, 0.82, 0.77), smoothstep(390.0, 520.0, input.worldPosition.y));
  if(scene.settings.w > 0.5) {
    color = 0.5 + 0.4*cos(vec3<f32>(0.0, 2.0, 4.0) + input.levelData.z*1.2);
  }

  let light = 0.3 + 0.7*max(0.0, dot(n, normalize(vec3<f32>(-0.4, 0.8, 0.2))));
  color*= light;

  // Screen derivatives keep the diagnostic grid about one pixel wide as LOD and distance change. Mode 2 overlays it on the LOD colors from mode 1.
  let lines = abs(fract(input.levelData.xy - 0.5) - 0.5)/max(fwidth(input.levelData.xy), vec2<f32>(0.0001));
  if(scene.settings.w > 1.5) {
    color*= mix(0.3, 1.0, smoothstep(0.0, 1.0, min(lines.x, lines.y)));
  }

  // Apply display gamma to the lit color for this standalone terrain pass.
  return(vec4<f32>(pow(color, vec3<f32>(1.0/2.2)), 1.0));
}
