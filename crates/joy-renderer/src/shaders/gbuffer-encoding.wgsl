// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// G-buffer encoding for deferred lighting, composed after surface-contract.wgsl wherever a shading model compiles.
//
// The G-buffer is one RGBA32Uint texel per pixel. The engine owns word X: the shading normal in octahedral form (15
// bits per axis) and whether the surface receives shadows. A shading model that supports deferred lighting fills words
// Y, Z, and W in `materialEncode` and reads them back in `materialDecode`; the helpers below pack common value types.
// Everything here uses plain shifts and masks, so it compiles on every backend, WebGL 2 included.

// Bits per octahedral axis of the engine's normal.
const GBUFFER_NORMAL_BITS: u32 = 15u;
// Bit of word X set when the surface receives shadows.
const GBUFFER_RECEIVES_SHADOW: u32 = 0x40000000u;

/**
 * Word X of a G-buffer texel.
 */
fn gbufferHeader(normal: vec3f, receivesShadow: bool) -> u32 {
  return packOctahedral(normal, GBUFFER_NORMAL_BITS) | select(0u, GBUFFER_RECEIVES_SHADOW, receivesShadow);
}

/**
 * The unit shading normal stored in word X.
 */
fn gbufferNormal(header: u32) -> vec3f {
  return unpackOctahedral(header, GBUFFER_NORMAL_BITS);
}

/**
 * Whether word X marks a surface that receives shadows.
 */
fn gbufferReceivesShadow(header: u32) -> bool {
  return (header & GBUFFER_RECEIVES_SHADOW) != 0u;
}

/**
 * The low `bits` bits of `word` starting at bit `offset`.
 */
fn gbufferField(word: u32, offset: u32, bits: u32) -> u32 {
  return (word >> offset) & ((1u << bits) - 1u);
}

/**
 * A value in [0, 1] rounded to `bits` bits.
 */
fn packUnorm(value: f32, bits: u32) -> u32 {
  let levels = f32((1u << bits) - 1u);
  return u32(round(clamp(value, 0.0, 1.0)*levels));
}

/**
 * The [0, 1] value a `packUnorm` field of `bits` bits holds.
 */
fn unpackUnorm(packed: u32, bits: u32) -> f32 {
  return f32(packed)/f32((1u << bits) - 1u);
}

/**
 * A unit vector as two `bits`-bit octahedral coordinates, X in the low field. Folding the lower hemisphere over the
 * upper keeps precision even across the sphere.
 */
fn packOctahedral(direction: vec3f, bits: u32) -> u32 {
  var unit = vec3f(0.0, 0.0, 1.0);
  let lengthSquared = dot(direction, direction);
  if(lengthSquared > 0.000000001) {
    unit = direction*inverseSqrt(lengthSquared);
  }
  var folded = unit.xy/(abs(unit.x) + abs(unit.y) + abs(unit.z));
  if(unit.z < 0.0) {
    let signs = select(vec2f(-1.0), vec2f(1.0), folded >= vec2f(0.0));
    folded = (vec2f(1.0) - abs(folded.yx))*signs;
  }
  let encoded = (folded*0.5) + 0.5;
  return packUnorm(encoded.x, bits) | (packUnorm(encoded.y, bits) << bits);
}

/**
 * The unit vector two `bits`-bit octahedral coordinates hold.
 */
fn unpackOctahedral(packed: u32, bits: u32) -> vec3f {
  let mask = (1u << bits) - 1u;
  let folded = (vec2f(unpackUnorm(packed & mask, bits), unpackUnorm((packed >> bits) & mask, bits))*2.0) - 1.0;
  var direction = vec3f(folded, 1.0 - abs(folded.x) - abs(folded.y));
  let overlap = max(-direction.z, 0.0);
  direction.x+= select(overlap, -overlap, direction.x >= 0.0);
  direction.y+= select(overlap, -overlap, direction.y >= 0.0);
  return normalize(direction);
}

/**
 * Nonnegative linear RGB as nine-bit mantissas sharing a five-bit exponent (the RGB9E5 layout): about three
 * significant digits of the brightest channel, up to 65408.
 */
fn packRgb9e5(color: vec3f) -> u32 {
  let clamped = clamp(color, vec3f(0.0), vec3f(65408.0));
  let brightest = max(clamped.r, max(clamped.g, clamped.b));
  if(brightest <= 0.0) {
    return 0u;
  }
  var exponent = max(floor(log2(brightest)), -16.0) + 16.0;
  var scale = exp2(exponent - 24.0);
  if(round(brightest/scale) >= 512.0) {
    exponent+= 1.0;
    scale*= 2.0;
  }
  let mantissa = vec3u(round(clamped/scale));
  return mantissa.r | (mantissa.g << 9u) | (mantissa.b << 18u) | (u32(exponent) << 27u);
}

/**
 * The linear RGB a `packRgb9e5` word holds.
 */
fn unpackRgb9e5(packed: u32) -> vec3f {
  let mantissa = vec3f(f32(packed & 511u), f32((packed >> 9u) & 511u), f32((packed >> 18u) & 511u));
  return mantissa*exp2(f32(packed >> 27u) - 24.0);
}
