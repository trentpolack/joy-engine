// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Instanced particles: every particle is one instance of a six-vertex quad. The vertex stage expands it to face the
// camera (or, for streaks, to follow the trail across the view plane), and the fragment stage computes the shape
// analytically: soft discs fade linearly to zero alpha at the edge, circles and rings get an antialiased edge, billows
// overlap two feathered lobes, streaks draw a ribbon under a head glow, and sprites sample one atlas cell.
//
// This file declares no bindings. particle-depth.wgsl (or particle-depth-none.wgsl) and particle-scene.wgsl follow it
// with the scene depth and sprite bindings and the entry points. Shapes return premultiplied color, so straight-alpha
// and premultiplied emitters share one source-over pipeline and additive emitters a one-one pipeline. Units are world
// units and radians; colors are linear HDR, or display-encoded values decoded per shape layer when the instance says
// so.

// Shape ids; must match `ParticleShape` in particles/instance.rs.
const PARTICLE_SHAPE_SOFT: u32 = 0u;
const PARTICLE_SHAPE_STREAK: u32 = 1u;
const PARTICLE_SHAPE_RING: u32 = 2u;
const PARTICLE_SHAPE_BILLOW: u32 = 3u;
const PARTICLE_SHAPE_CIRCLE: u32 = 4u;
const PARTICLE_SHAPE_SQUARE: u32 = 5u;
const PARTICLE_SHAPE_SPRITE: u32 = 6u;

// Flag bits above the shape byte (`*_FLAG` in particles/instance.rs): the instance color is premultiplied (alpha zero
// still glows); the frame's lights shade the instance; its RGB holds display-encoded (sRGB) values.
const PARTICLE_SHAPE_MASK: u32 = 255u;
const PARTICLE_PREMULTIPLIED: u32 = 256u;
const PARTICLE_LIT: u32 = 512u;
const PARTICLE_DISPLAY_COLOR: u32 = 1024u;

// How far a lit particle's normal leans from facing the camera toward its quad's corners, so a cheap camera-facing
// light still reads as a rounded volume rather than a flat card.
const PARTICLE_NORMAL_BEND: f32 = 0.6;

// A streak's head glow radius and ribbon half width, in multiples of its size.
const STREAK_GLOW: f32 = 1.7;
const STREAK_HALF_WIDTH: f32 = 0.5;

// Below this projected trail length a streak has no direction across the view and faces the camera's right axis.
const STREAK_MINIMUM_LENGTH: f32 = 1e-6;

// A ring's smallest half thickness in world units, and its half thickness as a fraction of its radius.
const RING_MINIMUM_HALF_WIDTH: f32 = 0.15;
const RING_WIDTH_FRACTION: f32 = 0.045;

// The billow: its outline's largest radius (1 + 0.09 + 0.05), the smaller lobe's offset (unrotated, in sizes), radius,
// extra roll, color boost, and alpha.
const BILLOW_EXTENT: f32 = 1.14;
const BILLOW_INNER_OFFSET: vec2f = vec2f(-0.16, -0.18);
const BILLOW_INNER_RADIUS: f32 = 0.65;
const BILLOW_INNER_ROTATION: f32 = 0.7;
const BILLOW_INNER_BOOST: f32 = 1.5;
const BILLOW_INNER_ALPHA: f32 = 0.65;

// One particle (`ParticleInstance` in particles/instance.rs).
struct ParticleInstance {
  // XYZ world position (a streak's head), W size: the radius or half extent in world units.
  @location(0) positionSize: vec4f,
  // Linear HDR RGB and alpha; premultiplied when the shape's flag says so.
  @location(1) color: vec4f,
  // XYZ a streak's world offset from head to tail, W camera-facing roll in radians.
  @location(2) trailRotation: vec4f,
  // A sprite's atlas cell: left, top, right, bottom (top-left origin).
  @location(3) uvRect: vec4f,
  // Shape id in the low byte, flags above.
  @location(4) shape: u32,
  // Soft-particle distance in world units; zero is hard.
  @location(5) softness: f32,
};

struct ParticleVarying {
  @builtin(position) clip: vec4f,
  // Position in the particle's plane in world units: along camera right and up from the center, or for streaks along
  // the trail and across it from the head.
  @location(0) local: vec2f,
  // Sprite texture coordinates; zero for other shapes.
  @location(1) uv: vec2f,
  @location(2) worldPosition: vec3f,
  @location(3) @interpolate(flat) color: vec4f,
  // X size, Y roll, Z a streak's projected trail length, W soft-particle distance.
  @location(4) @interpolate(flat) parameters: vec4f,
  @location(5) @interpolate(flat) shape: u32,
  // The camera-facing normal lit particles shade with, leaning toward the quad's corners; normalize per fragment.
  @location(6) normal: vec3f,
};

/**
 * Quad corner `index` (0–5) of two counter-clockwise triangles, in [-1, 1].
 */
fn particleCorner(index: u32) -> vec2f {
  let right = (index == 1u) || (index == 2u) || (index == 4u);
  let top = (index == 2u) || (index == 4u) || (index == 5u);
  return vec2f(select(-1.0, 1.0, right), select(-1.0, 1.0, top));
}

/**
 * A ring's half thickness for radius `size`.
 */
fn ringHalfWidth(size: f32) -> f32 {
  return max(RING_MINIMUM_HALF_WIDTH, size*RING_WIDTH_FRACTION);
}

/**
 * Expand one corner of `instance` in the plane spanned by the camera's `right` and `up` (with `backward` toward the
 * viewer) and project it with `viewProjection`.
 */
fn expandParticle(
  vertexIndex: u32,
  instance: ParticleInstance,
  right: vec3f,
  up: vec3f,
  backward: vec3f,
  viewProjection: mat4x4f,
) -> ParticleVarying {
  let corner = particleCorner(vertexIndex);
  let center = instance.positionSize.xyz;
  let size = instance.positionSize.w;
  let rotation = instance.trailRotation.w;
  let shape = instance.shape & PARTICLE_SHAPE_MASK;

  var axisX = right;
  var axisY = up;
  var local = corner*size;
  var offset = local;
  var uv = vec2f(0.0);
  var trailLength = 0.0;
  if(shape == PARTICLE_SHAPE_SQUARE || shape == PARTICLE_SHAPE_SPRITE) {
    // Squares and sprites roll with the particle; texture rows run down the sprite.
    let cosine = cos(rotation);
    let sine = sin(rotation);
    offset = vec2f((corner.x*cosine) - (corner.y*sine), (corner.x*sine) + (corner.y*cosine))*size;
    let cell = (corner + vec2f(1.0))*0.5;
    uv = vec2f(mix(instance.uvRect.x, instance.uvRect.z, cell.x), mix(instance.uvRect.w, instance.uvRect.y, cell.y));
  } else if(shape == PARTICLE_SHAPE_RING) {
    local = corner*(size + ringHalfWidth(size));
    offset = local;
  } else if(shape == PARTICLE_SHAPE_BILLOW) {
    local = corner*(size*BILLOW_EXTENT);
    offset = local;
  } else if(shape == PARTICLE_SHAPE_STREAK) {
    // The trail projected onto the view plane sets the quad's direction: x runs from the head toward the tail, y across
    // it. The quad spans the head glow and the ribbon.
    let trail = instance.trailRotation.xyz;
    let planar = trail - (dot(trail, backward)*backward);
    trailLength = length(planar);
    if(trailLength > STREAK_MINIMUM_LENGTH) {
      axisX = planar/trailLength;
    }
    axisY = cross(backward, axisX);
    let glow = size*STREAK_GLOW;
    local = vec2f(select(-glow, max(trailLength, glow), corner.x > 0.0), corner.y*glow);
    offset = local;
  }

  var output: ParticleVarying;
  let world = center + (offset.x*axisX) + (offset.y*axisY);
  output.clip = viewProjection*vec4f(world, 1.0);
  output.local = local;
  output.uv = uv;
  output.worldPosition = world;
  output.color = instance.color;
  output.parameters = vec4f(size, rotation, trailLength, instance.softness);
  output.shape = instance.shape;
  output.normal = backward + (((corner.x*axisX) + (corner.y*axisY))*PARTICLE_NORMAL_BEND);
  return output;
}

/**
 * Decode a display (sRGB) color channel triple to linear light, continuing the curve past one for HDR values and
 * mirroring it below zero (`srgb_to_linear` in joy-core).
 */
fn decodeDisplayColor(encoded: vec3f) -> vec3f {
  let magnitude = abs(encoded);
  let curve = pow((magnitude + vec3f(0.055))/1.055, vec3f(2.4));
  let segment = magnitude/12.92;
  return sign(encoded)*select(curve, segment, magnitude <= vec3f(0.04045));
}

/**
 * One shape layer's linear RGB: display-encoded instances decode here, after the layer scales the authored value, as a
 * display-color pipeline would.
 */
fn particleSourceColor(rgb: vec3f, flags: u32) -> vec3f {
  if((flags & PARTICLE_DISPLAY_COLOR) != 0u) {
    return decodeDisplayColor(rgb);
  }
  return rgb;
}

/**
 * One premultiplied layer: `color` with its RGB scaled by `rgbScale` and alpha by `alphaScale`, covering `coverage`
 * of the fragment. Straight colors weight RGB by alpha; premultiplied colors (in `flags`) already carry it.
 */
fn particleLayer(color: vec4f, rgbScale: f32, alphaScale: f32, coverage: f32, flags: u32) -> vec4f {
  let covered = clamp(coverage, 0.0, 1.0);
  let rgb = particleSourceColor(color.rgb*rgbScale, flags);
  let alpha = color.a*alphaScale*covered;
  if((flags & PARTICLE_PREMULTIPLIED) != 0u) {
    return vec4f(rgb*(alphaScale*covered), alpha);
  }
  return vec4f(rgb*alpha, alpha);
}

/**
 * Premultiplied `top` composited over premultiplied `bottom`, as two blended draws would.
 */
fn particleOver(top: vec4f, bottom: vec4f) -> vec4f {
  return top + (bottom*(1.0 - top.a));
}

/**
 * The billow outline's radius at `angle` around its unrotated outline, in sizes.
 */
fn billowOutline(angle: f32) -> f32 {
  return (1.0 + (sin(angle*3.0)*0.09)) + (cos(angle*5.0)*0.05);
}

/**
 * The angle of `point` around the origin, zero at the origin itself.
 */
fn particleAngle(point: vec2f) -> f32 {
  return atan2(point.y, select(point.x, 1.0, (point.x == 0.0) && (point.y == 0.0)));
}

/**
 * Coverage of an edge at signed distance `inside` (positive inward) with a one-pixel ramp `pixel` wide, centered on the
 * edge so thin shapes keep their average width.
 */
fn particleEdge(inside: f32, pixel: f32) -> f32 {
  return clamp((inside/max(pixel, 1e-6)) + 0.5, 0.0, 1.0);
}

/**
 * The premultiplied color of `input`'s fragment. `texel` is the sprite sample (sampled by the entry point in uniform
 * control flow) and `pixel` the world size of one pixel in the particle's plane.
 */
fn shadeParticle(input: ParticleVarying, texel: vec4f, pixel: f32) -> vec4f {
  let shape = input.shape & PARTICLE_SHAPE_MASK;
  let flags = input.shape;
  let color = input.color;
  let size = max(input.parameters.x, 1e-6);
  let rotation = input.parameters.y;
  let point = input.local;
  let radius = length(point);

  if(shape == PARTICLE_SHAPE_CIRCLE) {
    return particleLayer(color, 1.0, 1.0, particleEdge(size - radius, pixel), flags);
  }
  if(shape == PARTICLE_SHAPE_RING) {
    let halfWidth = ringHalfWidth(size);
    let inner = max(0.0, size - halfWidth);
    var coverage = particleEdge((size + halfWidth) - radius, pixel);
    if(inner > 0.0) {
      coverage *= particleEdge(radius - inner, pixel);
    }
    return particleLayer(color, 1.0, 1.0, coverage, flags);
  }
  if(shape == PARTICLE_SHAPE_BILLOW) {
    // The outer lobe fades from its center to the lobed outline; the smaller, brighter lobe sits off center (its
    // offset ignores roll), turned further, and fades toward the outer lobe's color.
    let outerRadius = size*billowOutline(particleAngle(point) - rotation);
    let outer = particleLayer(color, 1.0, 1.0, 1.0 - (radius/outerRadius), flags);
    let lobe = point - (BILLOW_INNER_OFFSET*size);
    let lobeAngle = particleAngle(lobe) - (rotation + BILLOW_INNER_ROTATION);
    let lobeFraction = length(lobe)/((size*BILLOW_INNER_RADIUS)*billowOutline(lobeAngle));
    let boost = BILLOW_INNER_BOOST - ((BILLOW_INNER_BOOST - 1.0)*min(lobeFraction, 1.0));
    let inner = particleLayer(color, boost, BILLOW_INNER_ALPHA, 1.0 - lobeFraction, flags);
    return particleOver(inner, outer);
  }
  if(shape == PARTICLE_SHAPE_STREAK) {
    // A solid ribbon from the head (x = 0) to the tail, under a soft glow around the head.
    let trailLength = input.parameters.z;
    let along = select(0.0, 1.0, (point.x >= 0.0) && (point.x <= trailLength));
    let across = particleEdge((size*STREAK_HALF_WIDTH) - abs(point.y), pixel);
    let ribbon = particleLayer(color, 1.0, 1.0, along*across, flags);
    let glow = particleLayer(color, 1.0, 1.0, 1.0 - (radius/(size*STREAK_GLOW)), flags);
    return particleOver(glow, ribbon);
  }
  if(shape == PARTICLE_SHAPE_SQUARE) {
    return particleLayer(color, 1.0, 1.0, 1.0, flags);
  }
  if(shape == PARTICLE_SHAPE_SPRITE) {
    let tinted = vec4f(color.rgb*texel.rgb, color.a);
    return particleLayer(tinted, 1.0, 1.0, texel.a, flags);
  }
  // Soft: the center color fading linearly to zero alpha at the edge.
  return particleLayer(color, 1.0, 1.0, 1.0 - (radius/size), flags);
}
