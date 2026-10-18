// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Height-only continuous LOD shared by every terrain pipeline: X/Z never move, and a patch's fractional LOD moves Y
// onto the parent triangles. Each terrain shader is this file followed by its own entry points (the scene variants
// also prepend scene-common.wgsl). Terrain bindings live in group 3 so the scene's frame and shadow groups keep their
// indices.

struct TerrainScene {
  // View-projection matrix for the standalone pass (scene passes use their own camera).
  matrix: mat4x4f,
  // XYZ eye position in the terrain's local space; W is the CSS projection scale.
  eye: vec4f,
  // X sample size, Y tile cells, Z/W world X/Z extents.
  geometry: vec4f,
  // X height offset, Y height scale, Z error tolerance in CSS pixels, W diagnostic mode.
  settings: vec4f,
  // XYZ protected target in the terrain's local space; W pin radius.
  focus: vec4f,
};

struct TerrainPatch {
  // XY sample-space origin, Z fractional LOD, W unused.
  base: vec4f,
  // Reconciled edge LODs ordered -X, +X, -Z, +Z.
  edges: vec4f,
};

struct TerrainModel {
  // Local-to-world transform.
  model: mat4x4f,
  // X receives shadows (0 or 1), Y scene roughness, ZW unused.
  flags: vec4f,
};

@group(3) @binding(0) var<uniform> scene: TerrainScene;
@group(3) @binding(1) var heightTexture: texture_2d<f32>;
// Bound at a per-draw dynamic offset: the patch for this draw. Indirect draws keep firstInstance zero.
@group(3) @binding(2) var<uniform> patchRecord: TerrainPatch;
@group(3) @binding(3) var<uniform> terrainModel: TerrainModel;

struct TerrainVertexInput {
  @location(0) gridPosition: vec2f,
};

struct TerrainVertexOutput {
  // Invariant so the equal-depth surface pass reproduces the lit pass's depth exactly across pipelines.
  @builtin(position) @invariant position: vec4f,
  @location(0) worldPosition: vec3f,
  // XY cells at the current integer LOD, Z fractional LOD.
  @location(1) levelData: vec3f,
};

/**
 * Load one authored height (already decoded to world units) at an exact integer sample coordinate.
 * Filtering is bypassed because interpolated texels would not match the indexed triangle surface.
 */
fn heightAt(p: vec2f) -> f32 {
  return textureLoad(heightTexture, vec2<i32>(p), 0).r;
}

/**
 * Reconstruct height on the diagonal split used by the terrain index buffer.
 * The final cell origin is clamped so all four corner loads stay inside the height texture.
 */
fn triangleHeight(p: vec2f, stride: f32) -> f32 {
  let b = min(floor(p/stride)*stride, vec2f(scene.geometry.x - 1.0 - stride));
  let f = (p - b)/stride;
  let a = heightAt(b);
  let bb = heightAt(b + vec2f(stride, 0.0));
  let c = heightAt(b + vec2f(0.0, stride));
  let d = heightAt(b + vec2f(stride));

  if(f.x >= f.y) {
    return a + ((bb - a)*f.x) + ((d - bb)*f.y);
  }

  return a + ((d - c)*f.x) + ((c - a)*f.y);
}

/**
 * Blend a point between its current and parent triangle lattices.
 * Neighboring patches use the reconciled edge detail, so their boundary morphs are numerically identical.
 */
fn morphedHeight(local: vec2f, detail: f32, record: TerrainPatch) -> f32 {
  let stride = exp2(floor(detail));
  let p = record.base.xy + local;

  return mix(triangleHeight(p, stride), triangleHeight(p, min(scene.geometry.y, stride*2.0)), fract(detail));
}

/**
 * Resolve a parent-lattice corner while honoring any stitched patch boundary.
 * Interior corners keep authored samples; boundary corners use the shared edge morph to prevent cracks.
 */
fn parentVertex(local: vec2f, stride: f32, record: TerrainPatch) -> f32 {
  var detail = log2(stride);
  var boundary = false;

  if(local.x == 0.0) {
    detail = max(detail, record.edges.x);
    boundary = true;
  }
  if(local.x == scene.geometry.y) {
    detail = max(detail, record.edges.y);
    boundary = true;
  }
  if(local.y == 0.0) {
    detail = max(detail, record.edges.z);
    boundary = true;
  }
  if(local.y == scene.geometry.y) {
    detail = max(detail, record.edges.w);
    boundary = true;
  }
  if(boundary) {
    return morphedHeight(local, detail, record);
  }

  return heightAt(record.base.xy + local);
}

/**
 * Interpolate the stitched parent triangle that receives disappearing vertices.
 * Using the actual parent triangle rather than raw samples keeps interior morph targets continuous at seams.
 */
fn parentSurface(local: vec2f, stride: f32, record: TerrainPatch) -> f32 {
  let b = min(floor(local/stride)*stride, vec2f(scene.geometry.y - stride));
  let f = (local - b)/stride;
  let a = parentVertex(b, stride, record);
  let bb = parentVertex(b + vec2f(stride, 0.0), stride, record);
  let c = parentVertex(b + vec2f(0.0, stride), stride, record);
  let d = parentVertex(b + vec2f(stride, stride), stride, record);

  if(f.x >= f.y) {
    return a + ((bb - a)*f.x) + ((d - bb)*f.y);
  }

  return a + ((d - c)*f.x) + ((c - a)*f.y);
}

/**
 * Morph one grid vertex onto its reconciled continuous LOD surface, in the terrain's local space.
 * Interior vertices converge on stitched parent triangles; boundary vertices follow the shared edge detail.
 */
fn terrainSurface(input: TerrainVertexInput) -> TerrainVertexOutput {
  let record = patchRecord;
  let local = input.gridPosition;
  let p = record.base.xy + local;
  var detail = record.base.z;

  // Boundary vertices follow shared edges; tile corners survive every stride, so corner heights stay invariant.
  if(local.x == 0.0) {
    detail = record.edges.x;
  }
  if(local.x == scene.geometry.y) {
    detail = record.edges.y;
  }
  if(local.y == 0.0) {
    detail = max(detail, record.edges.z);
  }
  if(local.y == scene.geometry.y) {
    detail = max(detail, record.edges.w);
  }

  let stride = exp2(floor(detail));
  var h = morphedHeight(local, detail, record);

  // Interior vertices are authored samples. Before the index range changes, morph removed vertices onto the stitched
  // parent triangle surface.
  if((local.x > 0.0) && (local.x < scene.geometry.y) && (local.y > 0.0) && (local.y < scene.geometry.y)) {
    h = mix(heightAt(p), parentSurface(local, min(scene.geometry.y, stride*2.0), record), fract(detail));
  }

  // Global sample offsets become centered X/Z; Y is already a world height. Diagnostics count cells at the integer LOD.
  var output: TerrainVertexOutput;
  output.worldPosition = vec3f(
    ((p.x/(scene.geometry.x - 1.0)) - 0.5)*scene.geometry.z,
    h,
    ((p.y/(scene.geometry.x - 1.0)) - 0.5)*scene.geometry.w
  );
  output.position = scene.matrix*vec4f(output.worldPosition, 1.0);
  output.levelData = vec3f(local/exp2(floor(record.base.z)), record.base.z);
  return output;
}

/**
 * Slope and altitude coloring shared by the standalone and scene passes, with the LOD diagnostics.
 * `lines` is the derivative-scaled grid distance, computed by the caller in uniform control flow.
 */
fn terrainAlbedo(worldHeight: f32, normalY: f32, levelData: vec3f, lines: vec2f) -> vec3f {
  let slope = 1.0 - abs(normalY);
  var color = mix(vec3f(0.21, 0.36, 0.26), vec3f(0.49, 0.43, 0.36), smoothstep(0.1, 0.65, slope));
  color = mix(color, vec3f(0.79, 0.82, 0.77), smoothstep(390.0, 520.0, worldHeight));
  if(scene.settings.w > 0.5) {
    color = 0.5 + (0.4*cos(vec3f(0.0, 2.0, 4.0) + (levelData.z*1.2)));
  }

  // Mode 2 overlays a grid about one pixel wide on the LOD colors from mode 1.
  if(scene.settings.w > 1.5) {
    color*= mix(0.3, 1.0, smoothstep(0.0, 1.0, min(lines.x, lines.y)));
  }
  return color;
}

/**
 * Distance to the nearest diagnostic grid line in pixels, from screen derivatives of the cell coordinates.
 */
fn terrainGridLines(levelData: vec3f) -> vec2f {
  return abs(fract(levelData.xy - 0.5) - 0.5)/max(fwidth(levelData.xy), vec2f(0.0001));
}
