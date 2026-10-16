// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// GPU counterpart of terrain_selection.rs on WebGPU-class backends. `evaluate` picks every fixed leaf's fractional LOD;
// `reconcile`, in a separate ordered dispatch so every neighbor LOD is ready, writes stitched patch records and the
// indexed indirect draw commands the render passes consume. Nothing is read back.

// Matches TerrainScene in terrain-geometry.wgsl.
struct TerrainScene {
  matrix: mat4x4f,
  // XYZ eye position; W CSS projection scale.
  eye: vec4f,
  // X sample size, Y tile cells, Z/W world X/Z extents.
  geometry: vec4f,
  // X height offset, Y height scale, Z error tolerance, W diagnostic mode.
  settings: vec4f,
  // XYZ protected target; W pin radius.
  focus: vec4f,
};

struct Metadata {
  // XY sample origin, Z minimum height, W maximum height.
  bounds: vec4f,
  // Eight error slots, finest first.
  errors0: vec4f,
  errors1: vec4f,
};

// One patch record per 256-byte slot, so render passes can bind each one at a uniform dynamic offset.
struct PatchSlot {
  base: vec4f,
  edges: vec4f,
  padding: array<vec4f, 14>,
};

// The five-word drawIndexedIndirect command (20 bytes).
struct Draw {
  count: u32,
  instances: u32,
  first: u32,
  base: u32,
  instance: u32,
};

@group(0) @binding(0) var<uniform> scene: TerrainScene;
@group(0) @binding(1) var<storage, read> metadata: array<Metadata>;
@group(0) @binding(2) var<storage, read_write> details: array<f32>;
@group(0) @binding(3) var<storage, read_write> patches: array<PatchSlot>;
@group(0) @binding(4) var<storage, read_write> draws: array<Draw>;
@group(0) @binding(5) var<storage, read> ranges: array<vec2<u32>>;

/**
 * Conservative world-space error for one LOD.
 * A projected cell-size floor (32 CSS pixels) keeps geometrically flat patches from producing oversized cells.
 */
fn errorAt(p: Metadata, level: u32) -> f32 {
  var error: f32;
  if(level < 4u) {
    error = p.errors0[level];
  } else {
    error = p.errors1[level - 4u];
  }

  let spacing = max(scene.geometry.z, scene.geometry.w)/(scene.geometry.x - 1.0);
  return max(error, exp2(f32(level))*spacing*scene.settings.z/32.0);
}

/**
 * Select one leaf's fractional LOD from the nearest distance to its bounds.
 * The fraction later drives a continuous height morph rather than an abrupt topology change.
 */
@compute @workgroup_size(64) fn evaluate(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x;
  if(i >= arrayLength(&metadata)) {
    return;
  }

  let p = metadata[i];
  let origin = (p.bounds.xy/(scene.geometry.x - 1.0))*scene.geometry.zw - (scene.geometry.zw/2.0);
  let end = origin + ((scene.geometry.y/(scene.geometry.x - 1.0))*scene.geometry.zw);
  let lowPoint = vec3f(origin.x, p.bounds.z, origin.y);
  let highPoint = vec3f(end.x, p.bounds.w, end.y);

  // Pin finest detail where the horizontal footprint touches the target disk.
  let targetDistance = length(max(max(origin - scene.focus.xz, scene.focus.xz - end), vec2f(0.0)));
  if((scene.focus.w > 0.0) && (targetDistance < scene.focus.w)) {
    details[i] = 0.0;
    return;
  }

  // Pixel tolerance becomes a world-height error budget at this distance.
  let distance = max(0.01, length(max(max(lowPoint - scene.eye.xyz, scene.eye.xyz - highPoint), vec3f(0.0))));
  let budget = (distance/scene.eye.w)*scene.settings.z;
  let maxLevel = u32(log2(scene.geometry.y));

  // The integer LOD chooses topology; the fraction morphs toward the next level. The finest level has zero error.
  var level = 0u;
  loop {
    if((level >= maxLevel) || (errorAt(p, level + 1u) > budget)) {
      break;
    }
    level+= 1u;
  }

  var selected = f32(level);
  var low = 0.0;
  if(level > 0u) {
    low = errorAt(p, level);
  }
  if(level < maxLevel) {
    selected+= clamp((budget - low)/(errorAt(p, level + 1u) - low), 0.0, 1.0);
  }

  // Fade the pin over one more radius with smoothstep, avoiding a hard transition at its boundary.
  var fade = 1.0;
  if(scene.focus.w > 0.0) {
    fade = clamp((targetDistance - scene.focus.w)/scene.focus.w, 0.0, 1.0);
  }

  details[i] = selected*fade*fade*(3.0 - (2.0*fade));
}

/**
 * Adopt the coarser fractional level on each shared edge and emit the leaf's record and indexed draw.
 * Boundary heights use the same surface on both sides, so no neighbor ratio restriction is needed.
 */
@compute @workgroup_size(64) fn reconcile(@builtin(global_invocation_id) id: vec3<u32>) {
  let i = id.x;
  if(i >= arrayLength(&metadata)) {
    return;
  }

  let axis = u32((scene.geometry.x - 1.0)/scene.geometry.y);
  let x = i % axis;
  let z = i/axis;
  let detail = details[i];

  var edges = vec4f(detail);
  if(x > 0u) {
    edges.x = max(detail, details[i - 1u]);
  }
  if((x + 1u) < axis) {
    edges.y = max(detail, details[i + 1u]);
  }
  if(z > 0u) {
    edges.z = max(detail, details[i - axis]);
  }
  if((z + 1u) < axis) {
    edges.w = max(detail, details[i + axis]);
  }

  patches[i].base = vec4f(metadata[i].bounds.xy, detail, 0.0);
  patches[i].edges = edges;

  // firstInstance stays zero; the per-draw dynamic offset supplies the patch record instead.
  let range = ranges[u32(floor(detail))];
  draws[i] = Draw(range.y, 1u, range.x, 0u, 0u);
}
