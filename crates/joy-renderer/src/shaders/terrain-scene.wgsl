// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Lit scene terrain: the vertex stage that feeds terrain into the engine's surface passes. Composed after the surface
// pass sources with terrain-geometry.wgsl and terrain-material.wgsl as the shading model, so terrain shades through
// the same fragment entry points (forward, scene-effect attachments, G-buffer, and debug views), light loop, and
// shadows as every mesh.

// Rate at which the LOD tint cycles through hues per level of detail.
const TERRAIN_TINT_RATE: f32 = 1.2;

/**
 * Morph a grid vertex, apply the terrain's world transform, project it with the frame camera, and carry the smooth
 * surface normal, the LOD tint, and lattice cell coordinates to the surface fragment stage.
 */
@vertex fn terrainVertexMain(vertex: TerrainVertex, tile: TerrainPatch) -> SurfaceVarying {
  let local = vec4f(terrainPosition(vertex.gridPosition, tile), 1.0);
  let world = terrainModel.model*local;
  let clips = motionClips(world, terrainModel.previousModel*local);

  var output: SurfaceVarying;
  output.position = frame.viewProjection*vec4f(world.xyz, 1.0);
  output.worldPosition = world.xyz;
  output.worldNormal = worldNormal(terrainModel.model, terrainNormal(vertex.gridPosition, tile));

  // Diagnostics color by global detail (synthesized detail levels have their own hues) and count cells at the drawn
  // stride.
  output.color = vec4f(1.0);
  if(material.geometry.y >= TERRAIN_LOD_COLORS) {
    let detail = tile.state.y + tile.state.z;
    output.color = vec4f(0.5 + (0.4*cos(vec3f(0.0, 2.0, 4.0) + (detail*TERRAIN_TINT_RATE))), 1.0);
  }
  output.uv = vertex.gridPosition/exp2(floor(tile.state.y));
  output.receivesShadow = terrainModel.flags.x;
  output.currentClip = clips[0];
  output.previousClip = clips[1];
  return output;
}
