// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Mesh vertex and instance inputs shared by every mesh pass (scene, depth pre-pass, and shadow casters). Geometry only:
// materials reach the passes through their own bind group, never through vertices. Instance attributes occupy locations
// 5 through 14, so mesh and instance attributes together stay within the sixteen WebGL 2 guarantees.

struct MeshVertex {
  @location(0) position: vec3f,
  // Model-space normal; zero where the mesh authored none (shading models then use the faceted triangle normal).
  @location(1) normal: vec3f,
  // Linear vertex color with straight alpha.
  @location(2) color: vec4f,
  // Texture coordinates with a top-left origin.
  @location(3) uv: vec2f,
};

struct Instance {
  @location(5) model0: vec4f,
  @location(6) model1: vec4f,
  @location(7) model2: vec4f,
  @location(8) model3: vec4f,
  // RGB tint and opacity.
  @location(9) tint: vec4f,
  // X receives shadows (0 or 1); remaining components are reserved.
  @location(10) flags: vec4f,
  // The previous frame's model matrix columns, for motion vectors (equal to the current matrix without history).
  @location(11) previous0: vec4f,
  @location(12) previous1: vec4f,
  @location(13) previous2: vec4f,
  @location(14) previous3: vec4f,
};

/**
 * Rebuild the instance's column-major model matrix from its four vertex-attribute columns.
 */
fn instanceModel(instance: Instance) -> mat4x4f {
  return mat4x4f(instance.model0, instance.model1, instance.model2, instance.model3);
}

/**
 * The instance's model matrix in the previous frame.
 */
fn instancePreviousModel(instance: Instance) -> mat4x4f {
  return mat4x4f(instance.previous0, instance.previous1, instance.previous2, instance.previous3);
}

/**
 * Transform a model-space normal into world space with the model's cofactor matrix.
 * The cofactor is the inverse transpose scaled by the determinant, so normalizing it handles non-uniform scale and
 * negative scale without a matrix inverse.
 */
fn worldNormal(model: mat4x4f, normal: vec3f) -> vec3f {
  let column0 = model[0].xyz;
  let column1 = model[1].xyz;
  let column2 = model[2].xyz;
  let cofactor = mat3x3f(cross(column1, column2), cross(column2, column0), cross(column0, column1));
  let transformed = cofactor*normal;
  return transformed/max(length(transformed), 0.000001);
}
