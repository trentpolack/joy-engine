/**
  * GPU counterpart of terrain-selection.js. Evaluate every fixed leaf first, then reconcile
  * in a separate ordered dispatch so all neighbor LODs are ready.
  */

/**
  * Scene matches TerrainRenderer.frameData: view-projection matrix, eye.xyz +
  * CSS projection scale, sample size/tile cells/XZ extents, height offset/scale +
  * error tolerance/diagnostic mode, and world-space target.xyz + pin radius.
  */

// Scene structure matches TerrainRenderer.frameData.
struct Scene {
  // View-projection matrix.
  matrix:mat4x4<f32>,

  // Eye position in world space and CSS projection scale.
  eye:vec4<f32>,

  // Sample Size, Tile Cell Count, and X/Z extents.
  geometry:vec4<f32>,

  // Height Offset, Height Scale, Error Tolerance, and Diagnostic Mode.
  settings:vec4<f32>,

  // World-space target position and pin radius.
  focus:vec4<f32>
};

struct Metadata {
  // Sample origin X/Z and Min/Max Y heights.
  bounds:vec4<f32>,

  // Eight error slots.
  errors0:vec4<f32>,
  errors1:vec4<f32>
};

// Patch: sample origin X/Z, fractional LOD, padding; edges are -X/+X/-Z/+Z.
struct Patch { base:vec4<f32>, edges:vec4<f32> };

// Draw follows the five-word drawIndexedIndirect command layout (20 bytes).
struct Draw { count:u32, instances:u32, first:u32, base:u32, instance:u32 };

@group(0) @binding(0) var<uniform> scene:Scene;
@group(0) @binding(1) var<storage,read> metadata:array<Metadata>;
@group(0) @binding(2) var<storage,read_write> details:array<f32>;
@group(0) @binding(3) var<storage,read_write> patches:array<Patch>;
@group(0) @binding(4) var<storage,read_write> draws:array<Draw>;
@group(0) @binding(5) var<storage,read> ranges:array<vec2<u32>>;

// Flat patches still need a projected cell-size limit. This error floor enforces
// a maximum of 32 CSS pixels per cell independently of geometric roughness.
fn errorAt(p:Metadata,level:u32) -> f32 {
  var error:f32;
  if(level < 4u) {
    error = p.errors0[level];
  } else {
    error = p.errors1[level - 4u];
  }

  let spacing = max(scene.geometry.z, scene.geometry.w)/(scene.geometry.x - 1.0);

  return(max(error, exp2(f32(level))*spacing*scene.settings.z/32.0));
}

// One invocation per leaf; the final workgroup can extend past the leaf count.
@compute @workgroup_size(64) fn evaluate(@builtin(global_invocation_id) id:vec3<u32>) {
  let i = id.x; if(i >= arrayLength(&metadata)) { return; }
  let p = metadata[i];

  // Measure nearest distance to the world-space bounds, not the patch center.
  let origin = p.bounds.xy/(scene.geometry.x - 1.0)*scene.geometry.zw - scene.geometry.zw/2.0;
  let end = origin + scene.geometry.y/(scene.geometry.x - 1.0)*scene.geometry.zw;
  let lowPoint = vec3<f32>(origin.x,p.bounds.z,origin.y);
  let highPoint = vec3<f32>(end.x,p.bounds.w,end.y);

  // Pin finest detail where the horizontal footprint touches the target disk.
  let targetDistance = length(max(max(origin - scene.focus.xz,scene.focus.xz - end),vec2<f32>(0)));
  if((scene.focus.w > 0.0) && (targetDistance < scene.focus.w)) {
    details[i] = 0.0;
    return;
  }

  let distance = max(0.01, length(max(max(lowPoint - scene.eye.xyz,scene.eye.xyz - highPoint), vec3<f32>(0))));

  // Pixel tolerance becomes a world-height error budget at this distance.
  let budget = distance/scene.eye.w*scene.settings.z;
  let maxLevel = u32(log2(scene.geometry.y));

  // Integer LOD chooses topology; its fraction morphs toward the next level.
  // Finest detail starts at zero error, bypassing the cell-size floor at level 0.
  var level = 0u;
  loop {
    if((level >= maxLevel) || (errorAt(p,level + 1u) > budget)) {
      break;
    }

    level+= 1u;
}

  var selected = f32(level);
  var low = 0.0;
  if(level > 0u) {
    low = errorAt(p,level);
  }

  if(level < maxLevel) {
    selected+= clamp((budget - low)/(errorAt(p, level + 1u) - low), 0.0, 1.0);
  }

  // Fade the pin over one additional radius with smoothstep, avoiding a hard topology transition as the target crosses a patch boundary.
  var fade = 1.0;
  if(scene.focus.w > 0.0) {
    fade = clamp((targetDistance - scene.focus.w)/scene.focus.w,0.0,1.0);
  }

  details[i] = selected*fade*fade*(3.0 - 2.0*fade);
}

/**
  * Both sides adopt the coarser fractional edge level. No finest/coarsest neighbor ratio restriction is needed because boundary heights use the same surface.
  * The final draw range is the union of all four edges, so the render pass can
  * draw the entire patch with a single indexed draw call. The draw range is stored
  * in the Draw structure, which matches the five-word drawIndexedIndirect layout.
  * The firstInstance is always zero; a static per-draw uniform supplies the descriptor index instead.
  */
@compute @workgroup_size(64) fn reconcile(@builtin(global_invocation_id) id:vec3<u32>) {
  let i = id.x; if(i >= arrayLength(&metadata)) { return; }
  let axis = u32((scene.geometry.x - 1.0)/scene.geometry.y);
  let x = i % axis; let z = i/axis; let detail = details[i];
  var edges = vec4<f32>(detail);
  if(x > 0u) {
    edges.x = max(detail,details[i - 1u]);
  }

  if(x + 1u < axis) {
    edges.y = max(detail,details[i + 1u]);
  }

  if(z > 0u) {
    edges.z = max(detail,details[i - axis]);
  }

  if((z + 1u) < axis) {
    edges.w = max(detail,details[i + axis]);
  }

  patches[i] = Patch(vec4<f32>(metadata[i].bounds.xy,detail,0),edges);

  // Emit the selected index range directly for the render pass. firstInstance stays zero; a static per-draw uniform supplies the descriptor index instead.
  let range = ranges[u32(floor(detail))];
  draws[i] = Draw(range.y, 1u, range.x, 0u, 0u);
}
