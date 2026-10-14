#version 300 es

// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

precision highp float;

/**
 * WebGL counterpart of terrain.wgsl; the CPU supplies reconciled patch LODs.
 */

/*
 * Scene matches TerrainRenderer.frameData:
 *  - View-projection matrix
 *  - eye.xyz + CSS projection scale
 *  - CSS projection scale, sample size/tile cells/XZ extents, height offset/scale +
 *  - Error tolerance/diagnostic mode
 *  - World-space target.xyz + pin radius.
 *
 *    TODO (trent, 10/12/26): Unpack these and put them into properly-sized vectors and explicit scalars. There's no packing benefit here.
 */
layout(location=0) in vec2 gridPosition;
layout(std140) uniform scene { mat4 matrix; vec4 eye; vec4 geometry; vec4 settings; vec4 target; };

/**
 * - Patch base = sample origin X/Z, fractional LOD, padding
 * - Patch edges = -X/+X/-Z/+Z.
 *
 *    TODO (trent, 10/12/26): Unpack these and put them into properly-sized vectors and explicit scalars. There's no packing benefit here.
 */
layout(std140) uniform patchUniforms {
  vec4 base;
  vec4 edges;
};

uniform highp sampler2D heightTexture;
out vec3 worldPosition;
out vec3 diagnostic;

/**
 * Loads one authored terrain height at an exact integer sample coordinate.
 * Filtering is deliberately bypassed because interpolated texture values would not match the indexed triangle surface.
 */
float heightAt(ivec2 p) {
  return(texelFetch(heightTexture,p,0).r);
}

/**
 * Reconstructs height on the same diagonal split used by the terrain index buffer.
 * The final cell origin is clamped so all four corner loads remain inside the height texture.
 */
float triangleHeight(vec2 p,float stride) {
  vec2 b = min(floor(p/stride)*stride,vec2(geometry.x - 1.0 - stride));
  vec2 f = (p - b)/stride;
  float a = heightAt(ivec2(b)), bb = heightAt(ivec2(b + vec2(stride,0)));
  float c = heightAt(ivec2(b + vec2(0,stride))), d = heightAt(ivec2(b + vec2(stride)));
  return((f.x >= f.y) ? (a + (bb - a)*f.x + (d - bb)*f.y) : (a + (d - c)*f.x + (c - a)*f.y));
}

/**
 * Blends a terrain point between its current and parent triangle lattices.
 * Neighboring patches use the reconciled edge detail, which makes their boundary morphs numerically identical.
 */
float morphedHeight(vec2 local,float detail) {
  float stride = exp2(floor(detail));
  vec2 p = base.xy + local;
  return(mix(triangleHeight(p,stride),triangleHeight(p,min(geometry.y,stride*2.0)),fract(detail)));
}

/**
 * Resolves a parent-lattice corner while honoring any stitched patch boundary.
 * Interior corners retain authored samples, whereas boundary corners use the shared edge morph to prevent cracks.
 */
float parentVertex(vec2 local,float stride) {
  float detail = log2(stride);
  bool boundary = false;
  if(local.x == 0.0) {
    detail = max(detail,edges.x);
    boundary = true;
  }
  if(local.x == geometry.y) {
    detail = max(detail,edges.y);
    boundary = true;
  }
  if(local.y == 0.0) {
    detail = max(detail,edges.z);
    boundary = true;
  }
  if(local.y == geometry.y) {
    detail = max(detail,edges.w);
    boundary = true;
  }

  return(boundary ? morphedHeight(local,detail) : heightAt(ivec2(base.xy + local)));
}

/**
 * Interpolates the stitched parent triangle that receives disappearing vertices.
 * Using the actual parent triangle, rather than raw height samples, keeps interior morph targets continuous at seams.
 */
float parentSurface(vec2 local,float stride) {
  vec2 b = min(floor(local/stride)*stride,vec2(geometry.y - stride));
  vec2 f = (local - b)/stride;
  float a = parentVertex(b,stride), bb = parentVertex(b + vec2(stride,0),stride);
  float c = parentVertex(b + vec2(0,stride),stride), d = parentVertex(b + vec2(stride),stride);

  return((f.x >= f.y) ? (a + (bb - a)*f.x + (d - bb)*f.y) : (a + (d - c)*f.x + (c - a)*f.y));
}

/**
 * Morphs one terrain-grid vertex to its reconciled continuous LOD surface.
 * Interior vertices converge on stitched parent triangles, while boundary vertices follow the shared edge detail to avoid cracks.
 */
void main() {
  vec2 p = base.xy + gridPosition;
  float detail = base.z;

  // Boundaries use reconciled edge LODs; corners retain their authored heights because every supported stride includes all four tile corners.
  if(gridPosition.x == 0.0) {
    detail = edges.x;
  }
  if(gridPosition.x == geometry.y) {
    detail = edges.y;
  }
  if(gridPosition.y == 0.0) {
    detail = max(detail, edges.z);
  }
  if(gridPosition.y == geometry.y) {
    detail = max(detail, edges.w);
  }

  float stride = exp2(floor(detail));
  bool boundary = gridPosition.x == 0.0 || gridPosition.x == geometry.y || gridPosition.y == 0.0 || gridPosition.y == geometry.y;

  // Current interior vertices sit on source samples and move onto the parent surface before switching index ranges. Boundary vertices use shared morphs.
  float h = boundary ? morphedHeight(gridPosition, detail) : mix(heightAt(ivec2(p)), parentSurface(gridPosition, min(geometry.y, stride*2.0)), fract(detail));

  // Sample-space X/Z map to centered world extents; height is already world Y.
  worldPosition = vec3((p.x/(geometry.x - 1.0) - 0.5)*geometry.z, h, (p.y/(geometry.x - 1.0) - 0.5)*geometry.w);

  // Count cells at the current integer LOD for the diagnostic grid overlay.
  diagnostic = vec3(gridPosition/exp2(floor(base.z)), base.z);
  gl_Position = matrix*vec4(worldPosition, 1.0);
}
