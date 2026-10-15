// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

@group(3) @binding(4) var<uniform> terrainModel: mat4x4f;

@vertex fn vertexMain(input: VertexInput) -> VertexOutput {
  var output = terrainSurface(input);
  output.worldPosition = (terrainModel*vec4f(output.worldPosition, 1.0)).xyz;
  output.position = uniforms.viewProjection*vec4f(output.worldPosition, 1.0);
  return output;
}

@fragment fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  let surface = cross(dpdy(input.worldPosition), dpdx(input.worldPosition));
  let normal = surface/max(length(surface), 0.000001);
  let slope = 1.0 - abs(normal.y);
  var color = mix(vec3f(0.21, 0.36, 0.26), vec3f(0.49, 0.43, 0.36), smoothstep(0.1, 0.65, slope));
  color = mix(color, vec3f(0.79, 0.82, 0.77), smoothstep(390.0, 520.0, input.worldPosition.y));
  if(scene.settings.w > 0.5) {
    color = 0.5 + 0.4*cos(vec3f(0.0, 2.0, 4.0) + input.levelData.z*1.2);
  }
  if(scene.settings.w > 1.5) {
    let lines = abs(fract(input.levelData.xy - 0.5) - 0.5)/max(fwidth(input.levelData.xy), vec2f(0.0001));
    color*= mix(0.3, 1.0, smoothstep(0.0, 1.0, min(lines.x, lines.y)));
  }
  return sceneLighting(input.worldPosition, vec4f(color, 1.0), normal);
}
