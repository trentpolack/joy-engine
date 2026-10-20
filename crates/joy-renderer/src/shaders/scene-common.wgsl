// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Shared scene contract for every scene pass: frame uniforms, the frame's clustered point lights, volumetric fog for
// transparent surfaces, the engine's flat lighting (used by terrain and the lighting debug views), and the motion
// vectors opaque surfaces write beside their color for scene effects. The renderer appends shadow-receiver.wgsl, whose
// shadow lookups the lighting here calls. Mesh passes add scene-vertex.wgsl, the shading-model contract, a model, and
// their entry points after both.

struct Frame {
  viewProjection: mat4x4f,
  view: mat4x4f,
  // Inverse of `viewProjection`, for rebuilding world positions from rasterized depth.
  inverseViewProjection: mat4x4f,
  // XYZ eye position in world units; W is presentation time in seconds.
  eye: vec4f,
  // X point lights drawn this frame, Y ambient irradiance, ZW unused.
  lighting: vec4f,
  // XYZ direction toward the environment's primary light; W intensity.
  primaryDirection: vec4f,
  // RGB primary light color.
  primaryColor: vec4f,
  // RGB sky color overhead; A skylight intensity.
  skylight: vec4f,
  // RGB light the ground reflects up into the skylight's lower hemisphere; A unused.
  groundSkylight: vec4f,
  // Unjittered view-projection, and the previous frame's, for motion vectors and light clusters.
  stableViewProjection: mat4x4f,
  previousViewProjection: mat4x4f,
  // X 1 while volumetric fog renders, Y the view depth its volume covers, ZW unused.
  fog: vec4f,
  // X debug view (0 for the lit scene; see `DebugView` in Rust), Y a per-frame offset in [0, 1) for screen-space
  // noise (zero unless temporal antialiasing averages frames), ZW reserved.
  debug: vec4f,
  // XY render size in pixels, ZW its reciprocal.
  viewport: vec4f,
  // Light clusters: X columns, Y rows, Z depth slices, W 1 when any point light is drawn.
  clusterGrid: vec4f,
  // Depth slice of a view depth: floor(log2(depth) * X + Y), for depths up to Z; W unused.
  clusterDepth: vec4f,
};

@group(0) @binding(0) var<uniform> frame: Frame;
// Integrated volumetric fog along view rays: RGB light scattered toward the eye, A transmittance. A one-texel clear
// volume when volumetric fog is off.
@group(0) @binding(1) var fogVolume: texture_3d<f32>;
@group(0) @binding(2) var fogSampler: sampler;
// The frame's point lights, strongest first: row 0 holds each light's position and range, row 1 its color and shadow
// slot plus one (zero without a shadow).
@group(0) @binding(3) var pointLights: texture_2d<f32>;
// Each cluster's light list (column + row * columns, slice): X its first entry in `clusterLightIndices`, Y its length.
@group(0) @binding(4) var lightClusters: texture_2d<u32>;
// Concatenated light lists, CLUSTER_INDEX_ROW entries per row.
@group(0) @binding(5) var clusterLightIndices: texture_2d<u32>;

/**
 * Orient a world-space normal toward the eye, giving double-sided surfaces a consistent front for lighting and bias.
 */
fn faceEye(normal: vec3f, worldPosition: vec3f) -> vec3f {
  if(dot(normal, frame.eye.xyz - worldPosition) < 0.0) {
    return -normal;
  }
  return normal;
}

// Clustered point lights. The renderer bins every drawn light into the view-aligned clusters its range reaches
// (`lighting/clusters.rs`); a surface reads only the lights listed for the cluster it lies in, so the cost of shading a
// pixel follows the lights that can reach it rather than every light in the frame.

// Entries per row of `clusterLightIndices`; must match `CLUSTER_INDEX_ROW` in lighting/clusters.rs.
const CLUSTER_INDEX_ROW: u32 = 1024u;

struct PointLight {
  // World position and the distance at which the light reaches zero.
  positionRange: vec4f,
  // Linear RGB irradiance; A is the light's shadow slot plus one, or zero without a shadow.
  color: vec4f,
};

/**
 * One of the frame's point lights by index.
 */
fn pointLight(index: u32) -> PointLight {
  var light: PointLight;
  light.positionRange = textureLoad(pointLights, vec2u(index, 0u), 0);
  light.color = textureLoad(pointLights, vec2u(index, 1u), 0);
  return light;
}

/**
 * The light list of the cluster holding `worldPosition`: X its first entry, Y how many lights it holds. Positions
 * behind the eye or beyond the farthest light's reach have no lights.
 */
fn lightCluster(worldPosition: vec3f) -> vec2u {
  if(frame.clusterGrid.w < 0.5) {
    return vec2u(0u);
  }
  let clip = frame.stableViewProjection*vec4f(worldPosition, 1.0);
  let viewDepth = -(frame.view*vec4f(worldPosition, 1.0)).z;
  if((clip.w <= 0.0) || (viewDepth <= 0.0) || (viewDepth > frame.clusterDepth.z)) {
    return vec2u(0u);
  }
  let grid = vec3u(frame.clusterGrid.xyz);
  let coordinate = clamp(vec2f((clip.x/clip.w*0.5) + 0.5, 0.5 - (clip.y/clip.w*0.5)), vec2f(0.0), vec2f(1.0));
  let tile = min(vec2u(coordinate*vec2f(grid.xy)), grid.xy - vec2u(1u));
  let slice = min(u32(max((log2(viewDepth)*frame.clusterDepth.x) + frame.clusterDepth.y, 0.0)), grid.z - 1u);
  return textureLoad(lightClusters, vec2u(tile.x + (tile.y*grid.x), slice), 0).xy;
}

/**
 * The frame light index stored at one entry of the concatenated cluster lists.
 */
fn clusterLightIndex(entry: u32) -> u32 {
  return textureLoad(clusterLightIndices, vec2u(entry%CLUSTER_INDEX_ROW, entry/CLUSTER_INDEX_ROW), 0).x;
}

/**
 * How strongly a point light reaches a position: one at the light, falling quadratically to zero at its range.
 */
fn pointLightReach(light: PointLight, worldPosition: vec3f) -> f32 {
  let reach = max(0.0, 1.0 - (distance(light.positionRange.xyz, worldPosition)/light.positionRange.w));
  return reach*reach;
}

/**
 * Skylight irradiance on a surface: the sky overhead blended toward the ground's reflected light as the normal tilts
 * down, the exact irradiance of two uniform hemispheres (`Skylight::irradiance` in joy-core).
 */
fn skylightIrradiance(normal: vec3f) -> vec3f {
  let skyWeight = clamp(0.5 + (0.5*normal.y), 0.0, 1.0);
  return mix(frame.groundSkylight.rgb, frame.skylight.rgb, skyWeight)*frame.skylight.a;
}

/**
 * The engine's standard flat irradiance from everything but the primary and point lights: uniform ambient plus the
 * sky-and-ground skylight.
 */
fn standardAmbient(normal: vec3f) -> vec3f {
  return vec3f(frame.lighting.y) + skylightIrradiance(normal);
}

/**
 * The engine's standard flat direct irradiance: a double-sided primary light and the cluster's point lights, with
 * their shadows. Diffuse terms use |n.l| without a 1/pi factor so existing scenes keep their authored brightness.
 */
fn standardDirect(worldPosition: vec3f, normal: vec3f, receivesShadow: bool) -> vec3f {
  var irradiance = vec3f(0.0);
  if(frame.primaryDirection.w > 0.0) {
    var visibility = 1.0;
    if(receivesShadow) {
      visibility = directionalShadow(worldPosition, normal);
    }
    let diffuse = abs(dot(normal, normalize(frame.primaryDirection.xyz)));
    irradiance+= frame.primaryColor.rgb*frame.primaryDirection.w*diffuse*visibility;
  }

  let cluster = lightCluster(worldPosition);
  for(var entry = cluster.x; entry < cluster.x + cluster.y; entry++) {
    let light = pointLight(clusterLightIndex(entry));
    let reach = pointLightReach(light, worldPosition);
    if(reach <= 0.0) {
      continue;
    }
    var visibility = 1.0;
    if(receivesShadow) {
      visibility = pointShadow(i32(light.color.a) - 1, worldPosition, normal);
    }
    let direction = normalize(light.positionRange.xyz - worldPosition);
    irradiance+= light.color.rgb*reach*abs(dot(normal, direction))*visibility;
  }
  return irradiance;
}

/**
 * The engine's standard flat lighting of an albedo: ambient, skylight, the primary light, and point lights.
 */
fn standardLighting(worldPosition: vec3f, color: vec3f, normal: vec3f, receivesShadow: f32) -> vec3f {
  return color*(standardAmbient(normal) + standardDirect(worldPosition, normal, receivesShadow > 0.5));
}

// Volumetric fog. The effect integrates scattering and transmittance into a froxel volume aligned with the view; slices
// are spaced by the square root of view depth, so near slices are thin.

/**
 * Fog between the eye and `worldPosition`: RGB scattered light reaching the eye, A transmittance. Transparent surfaces
 * apply it themselves; the effect applies it to opaque surfaces and the sky in one pass.
 */
fn volumetricFog(worldPosition: vec3f) -> vec4f {
  if(frame.fog.x < 0.5) {
    return vec4f(0.0, 0.0, 0.0, 1.0);
  }
  let clip = frame.stableViewProjection*vec4f(worldPosition, 1.0);
  let viewDepth = -(frame.view*vec4f(worldPosition, 1.0)).z;
  if((clip.w <= 0.0) || (viewDepth <= 0.0)) {
    return vec4f(0.0, 0.0, 0.0, 1.0);
  }
  let uv = vec2f((clip.x/clip.w*0.5) + 0.5, 0.5 - (clip.y/clip.w*0.5));
  return fogAt(uv, viewDepth);
}

/**
 * Fog at a screen coordinate and view depth. The volume stores each slice's value at its far boundary, so the lookup
 * shifts by half a slice and eases in from no fog at the eye.
 */
fn fogAt(uv: vec2f, viewDepth: f32) -> vec4f {
  let slices = f32(textureDimensions(fogVolume).z);
  let slice = sqrt(clamp(viewDepth/frame.fog.y, 0.0, 1.0));
  let coordinate = vec3f(clamp(uv, vec2f(0.0), vec2f(1.0)), max(slice - (0.5/slices), 0.5/slices));
  let fog = textureSampleLevel(fogVolume, fogSampler, coordinate, 0.0);
  return mix(vec4f(0.0, 0.0, 0.0, 1.0), fog, clamp(slice*slices, 0.0, 1.0));
}

/**
 * Fog over a straight-alpha color.
 */
fn applyFog(color: vec4f, worldPosition: vec3f) -> vec4f {
  let fog = volumetricFog(worldPosition);
  return vec4f((color.rgb*fog.a) + fog.rgb, color.a);
}

// Scene-effect attachments. With effects that need them, opaque materials write motion vectors beside their color in
// the one scene pass, instead of rasterizing opaque geometry again.

struct SceneTargets {
  @location(0) color: vec4f,
  // RG screen-space motion in UV units from the previous frame's position to this one's, without jitter; B view
  // depth; A one where a surface was drawn (zero for the sky). Effects read depth here rather than from the depth
  // buffer, which WebGL 2 shaders cannot load.
  @location(1) motion: vec4f,
};

/**
 * Clip positions for motion vectors: this frame's unjittered camera and the previous frame's camera and transform.
 */
fn motionClips(world: vec4f, previousWorld: vec4f) -> mat2x4f {
  return mat2x4f(frame.stableViewProjection*world, frame.previousViewProjection*previousWorld);
}

/**
 * Every scene target for one opaque fragment.
 */
fn sceneTargets(color: vec4f, worldPosition: vec3f, clips: mat2x4f) -> SceneTargets {
  // History behind the eye is not a valid correspondence and must never produce NaNs. Framebuffer UVs run downward
  // while clip-space Y runs upward.
  let current = clips[0];
  let previous = clips[1];
  var velocity = vec2f(0.0);
  if((current.w > 0.000001) && (previous.w > 0.000001)) {
    velocity = ((current.xy/current.w) - (previous.xy/previous.w))*vec2f(0.5, -0.5);
  }

  var targets: SceneTargets;
  targets.color = color;
  targets.motion = vec4f(velocity, -(frame.view*vec4f(worldPosition, 1.0)).z, 1.0);
  return targets;
}
