// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

@group(3) @binding(4) var<uniform> terrainModel: mat4x4f;
@group(1) @binding(0) var<uniform> casterCamera: mat4x4f;

@vertex fn vertexMain(input: VertexInput) -> @builtin(position) vec4f {
  let surface = terrainSurface(input);
  return casterCamera*terrainModel*vec4f(surface.worldPosition, 1.0);
}

@fragment fn fragmentMain() {}
