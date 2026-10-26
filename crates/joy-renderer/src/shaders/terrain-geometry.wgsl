// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Height-only continuous LOD shared by every terrain pipeline: X/Z never move, and a tile's fractional LOD moves Y onto
// the parent triangles. Each drawn tile is one instance whose record (placement, state, and edges) arrives as instance
// attributes, and whose lattice lives in one layer of the pooled height texture array. Shaders work in tile-lattice
// coordinates (0 to tile cells); details arrive relative to the tile's level, so the morph is the same for every level.
//
// A layer holds the tile's lattice with a one-sample apron, `(cells + 3)` texels square, followed by rows of "rings":
// for each stride `2^k` (k from 1 to log2(cells)) and each side (-X, +X, -Z, +Z), the lattice heights one stride beyond
// that side at every stride-`2^k` point along it. Central differences at any stride therefore read real neighboring
// heights at the tile's edges, so normals agree across tiles of every level.
//
// Each terrain shader is this file followed by its own entry points; the lit pass composes it as its shading model's
// material group. Terrain bindings live in group 3, the material group; pipelines with fewer groups move them down
// (`terrain_geometry_source`).

// The material group header the surface passes read, followed by terrain settings.
struct MaterialParameters {
  // X alpha cutoff, Y alpha mode (opaque), Z shading model index for debug views, W reserved.
  coverage: vec4f,
  // X tile cells, Y diagnostic mode, ZW unused.
  geometry: vec4f,
  // Material rules: XY slope where rock starts and finishes replacing ground, ZW height where snow starts and covers.
  materials: vec4f,
};

struct TerrainModel {
  // Local-to-world transform.
  model: mat4x4f,
  // The previous frame's transform, for motion vectors (equal to `model` without history).
  previousModel: mat4x4f,
  // X receives shadows (0 or 1), ZW unused.
  flags: vec4f,
};

@group(3) @binding(0) var<uniform> material: MaterialParameters;
@group(3) @binding(1) var heightTexture: texture_2d_array<f32>;
@group(3) @binding(2) var<uniform> terrainModel: TerrainModel;

struct TerrainVertex {
  @location(0) gridPosition: vec2f,
};

// One drawn tile, as instance attributes.
struct TerrainPatch {
  // XY local X/Z of the tile's lattice origin, ZW local X/Z size of one lattice cell.
  @location(1) placement: vec4f,
  // X height-texture layer, Y fractional detail relative to the tile's level, Z the tile's level, W unused.
  @location(2) state: vec4f,
  // Reconciled edge details relative to the tile's level, ordered -X, +X, -Z, +Z.
  @location(3) edges: vec4f,
};

/**
 * Tile cells per side.
 */
fn terrainCells() -> f32 {
  return material.geometry.x;
}

/**
 * Load one tile height (world units) at an exact integer lattice coordinate inside the tile or its apron.
 * Filtering is bypassed because interpolated texels would not match the indexed triangle surface.
 */
fn apronHeight(q: vec2i, layer: i32) -> f32 {
  return textureLoad(heightTexture, q + vec2i(1), layer, 0).r;
}

/**
 * The lattice height at `q`, reached by a step of stride `2^k` from a lattice point of the tile. Points beyond the apron
 * lie on one side's ring for that stride.
 */
fn latticeHeight(q: vec2i, k: i32, layer: i32) -> f32 {
  let cells = i32(terrainCells());
  if(all(q >= vec2i(-1)) && all(q <= vec2i(cells + 1))) {
    return apronHeight(q, layer);
  }

  // Rings follow the apron rows, ordered by stride, then side, then position along the side.
  var side = 3;
  var along = q.x;
  if(q.x < 0) {
    side = 0;
    along = q.y;
  } else if(q.x > cells) {
    side = 1;
    along = q.y;
  } else if(q.y < 0) {
    side = 2;
  }
  let count = (cells >> u32(k)) + 1;
  let previous = 4*((cells - (cells >> u32(k - 1))) + (k - 1));
  let offset = previous + (side*count) + (along >> u32(k));
  let width = cells + 3;
  return textureLoad(heightTexture, vec2i(offset%width, width + (offset/width)), layer, 0).r;
}

/**
 * Reconstruct height on the diagonal split used by the terrain index buffer.
 * The final cell origin is clamped so all four corner loads stay inside the tile.
 */
fn triangleHeight(p: vec2f, stride: f32, layer: i32) -> f32 {
  let b = min(floor(p/stride)*stride, vec2f(terrainCells() - stride));
  let f = (p - b)/stride;
  let a = apronHeight(vec2i(b), layer);
  let bb = apronHeight(vec2i(b + vec2f(stride, 0.0)), layer);
  let c = apronHeight(vec2i(b + vec2f(0.0, stride)), layer);
  let d = apronHeight(vec2i(b + vec2f(stride)), layer);

  if(f.x >= f.y) {
    return a + ((bb - a)*f.x) + ((d - bb)*f.y);
  }

  return a + ((d - c)*f.x) + ((c - a)*f.y);
}

/**
 * Blend a point between its current and parent triangle lattices.
 * Neighboring tiles use the reconciled edge detail, so their boundary morphs are numerically identical.
 */
fn morphedHeight(p: vec2f, detail: f32, layer: i32) -> f32 {
  let stride = exp2(floor(detail));
  let fine = triangleHeight(p, stride, layer);
  return mix(fine, triangleHeight(p, min(terrainCells(), stride*2.0), layer), fract(detail));
}

/**
 * Resolve a parent-lattice corner while honoring any stitched tile boundary.
 * Interior corners keep their samples; boundary corners use the shared edge morph to prevent cracks.
 */
fn parentVertex(p: vec2f, stride: f32, tile: TerrainPatch) -> f32 {
  let cells = terrainCells();
  let layer = i32(tile.state.x);
  var detail = log2(stride);
  var boundary = false;

  if(p.x == 0.0) {
    detail = max(detail, tile.edges.x);
    boundary = true;
  }
  if(p.x == cells) {
    detail = max(detail, tile.edges.y);
    boundary = true;
  }
  if(p.y == 0.0) {
    detail = max(detail, tile.edges.z);
    boundary = true;
  }
  if(p.y == cells) {
    detail = max(detail, tile.edges.w);
    boundary = true;
  }
  if(boundary) {
    return morphedHeight(p, detail, layer);
  }

  return apronHeight(vec2i(p), layer);
}

/**
 * Interpolate the stitched parent triangle that receives disappearing vertices.
 * Using the actual parent triangle rather than raw samples keeps interior morph targets continuous at seams.
 */
fn parentSurface(p: vec2f, stride: f32, tile: TerrainPatch) -> f32 {
  let b = min(floor(p/stride)*stride, vec2f(terrainCells() - stride));
  let f = (p - b)/stride;
  let a = parentVertex(b, stride, tile);
  let bb = parentVertex(b + vec2f(stride, 0.0), stride, tile);
  let c = parentVertex(b + vec2f(0.0, stride), stride, tile);
  let d = parentVertex(b + vec2f(stride, stride), stride, tile);

  if(f.x >= f.y) {
    return a + ((bb - a)*f.x) + ((d - bb)*f.y);
  }

  return a + ((d - c)*f.x) + ((c - a)*f.y);
}

/**
 * The fractional detail one vertex follows: boundary vertices take their shared edge's detail (tile corners survive
 * every stride, so their heights stay invariant), interior vertices the tile's.
 */
fn vertexDetail(p: vec2f, tile: TerrainPatch) -> f32 {
  let cells = terrainCells();
  var detail = tile.state.y;
  if(p.x == 0.0) {
    detail = tile.edges.x;
  }
  if(p.x == cells) {
    detail = tile.edges.y;
  }
  if(p.y == 0.0) {
    detail = max(detail, tile.edges.z);
  }
  if(p.y == cells) {
    detail = max(detail, tile.edges.w);
  }
  return detail;
}

/**
 * Morph one grid vertex onto its reconciled continuous LOD surface; returns its position in the terrain's local space.
 * Interior vertices converge on stitched parent triangles; boundary vertices follow the shared edge detail.
 */
fn terrainPosition(p: vec2f, tile: TerrainPatch) -> vec3f {
  let cells = terrainCells();
  let layer = i32(tile.state.x);
  let detail = vertexDetail(p, tile);
  let stride = exp2(floor(detail));

  // Interior vertices are tile samples that, before the index range changes, morph onto the stitched parent triangle
  // surface. Boundary vertices morph along their shared edge.
  var h = 0.0;
  if((p.x > 0.0) && (p.x < cells) && (p.y > 0.0) && (p.y < cells)) {
    h = mix(apronHeight(vec2i(p), layer), parentSurface(p, min(cells, stride*2.0), tile), fract(detail));
  } else {
    h = morphedHeight(p, detail, layer);
  }

  // Lattice coordinates become local X/Z through the tile's placement; Y is already a local height.
  return vec3f(tile.placement.x + (p.x*tile.placement.z), h, tile.placement.y + (p.y*tile.placement.w));
}

/**
 * Height change per local unit along X and Z at lattice point `q`, by central differences at stride `2^k`.
 */
fn latticeGradient(q: vec2i, k: i32, tile: TerrainPatch) -> vec2f {
  let layer = i32(tile.state.x);
  let s = 1 << u32(k);
  let dx = latticeHeight(q + vec2i(s, 0), k, layer) - latticeHeight(q - vec2i(s, 0), k, layer);
  let dz = latticeHeight(q + vec2i(0, s), k, layer) - latticeHeight(q - vec2i(0, s), k, layer);
  return vec2f(dx, dz)/(2.0*f32(s)*tile.placement.zw);
}

/**
 * The stride-`2^k` lattice's gradients interpolated at `p` over the same diagonal split as `triangleHeight`: smooth
 * shading of the triangles drawn at that stride.
 */
fn triangleGradient(p: vec2f, k: i32, tile: TerrainPatch) -> vec2f {
  let stride = exp2(f32(k));
  let b = min(floor(p/stride)*stride, vec2f(terrainCells() - stride));
  let f = (p - b)/stride;
  let base = vec2i(b);
  let s = i32(stride);
  let a = latticeGradient(base, k, tile);
  if(all(f == vec2f(0.0))) {
    return a;
  }

  let d = latticeGradient(base + vec2i(s), k, tile);
  if(f.x >= f.y) {
    let bb = latticeGradient(base + vec2i(s, 0), k, tile);
    return a + ((bb - a)*f.x) + ((d - bb)*f.y);
  }
  let c = latticeGradient(base + vec2i(0, s), k, tile);
  return a + ((d - c)*f.x) + ((c - a)*f.y);
}

/**
 * The unit local-space normal of the morphed surface at a grid vertex: the drawn stride's smooth normal blended toward
 * its parent stride's as the vertex morphs, so shading changes as continuously as the geometry.
 */
fn terrainNormal(p: vec2f, tile: TerrainPatch) -> vec3f {
  let detail = vertexDetail(p, tile);
  let finest = i32(floor(detail));
  let coarsest = i32(log2(terrainCells()));
  let fine = triangleGradient(p, finest, tile);
  let coarse = triangleGradient(p, min(finest + 1, coarsest), tile);
  let gradient = mix(fine, coarse, fract(detail));
  return normalize(vec3f(-gradient.x, 1.0, -gradient.y));
}
