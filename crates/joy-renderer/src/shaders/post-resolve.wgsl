// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Display resolve, the last pass before any UI: upscales the scene when it renders below the output size, applies
// exposure, adds bloom (computed from the exposed scene), darkens the vignette in linear light, applies the display
// transform, extends highlights into any HDR headroom, and writes what the presentation target expects. Composed after
// tone-mapping.wgsl.
//
// Presentation targets receive sRGB-encoded values when they store them directly (non-sRGB views and extended sRGB
// surfaces), and linear light when the view encodes on write or the surface is extended linear (scRGB). Eight-bit
// targets get one code value of dither so smooth gradients do not band.

struct Resolve {
  // X manual exposure, Y tone mapper (TONE_MAPPER_* in tone-mapping.wgsl), Z AgX look (0 default, 1 punchy), W bloom
  // strength (zero without bloom).
  display: vec4f,
  // Filmic contrast, linear-section start, linear-section length, and toe.
  filmic: vec4f,
  // X vignette strength, Y vignette radius, Z output aspect (width over height), W unused.
  vignette: vec4f,
  // X display headroom (multiple of SDR white; one for standard output), Y 1 when the scene is upscaled, Z 1 when the
  // target expects linear light, W the linear signal of SDR white (one except on extended linear surfaces).
  output: vec4f,
  // XY scene size in pixels, ZW its reciprocal.
  scene: vec4f,
  // X dither amplitude in encoded units (one code value on eight-bit targets, zero otherwise), Y 1 when the display
  // transform does not end at SDR white (no transform passes light as it is), so nothing extends into the headroom; ZW unused.
  encoding: vec4f,
};

@group(0) @binding(0) var<uniform> resolve: Resolve;
@group(0) @binding(1) var sceneTexture: texture_2d<f32>;
@group(0) @binding(2) var bloomTexture: texture_2d<f32>;
@group(0) @binding(3) var linearSampler: sampler;
@group(0) @binding(4) var exposureTexture: texture_2d<f32>;

struct ResolveVarying {
  @builtin(position) position: vec4f,
  @location(0) coordinate: vec2f,
};

/**
 * Full-screen triangle with top-left texture coordinates.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> ResolveVarying {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));
  var output: ResolveVarying;
  output.position = vec4f((position*2.0) - 1.0, 0.0, 1.0);
  output.coordinate = vec2f(position.x, 1.0 - position.y);
  return output;
}

/**
 * Scene color at an output coordinate. A scene rendered below the output size is upscaled with a Catmull-Rom bicubic
 * filter, which keeps edges and texture detail sharper than bilinear filtering; nine bilinear fetches cover its sixteen
 * taps. The result is clamped to the four nearest texels so the filter's negative lobes cannot ring around highlights.
 */
fn sampleScene(coordinate: vec2f) -> vec3f {
  if(resolve.output.y < 0.5) {
    return textureSampleLevel(sceneTexture, linearSampler, coordinate, 0.0).rgb;
  }

  let size = resolve.scene.xy;
  let texel = resolve.scene.zw;
  let position = (coordinate*size) - vec2f(0.5);
  let origin = floor(position);
  let fraction = position - origin;

  // Catmull-Rom weights for the taps one before, at, after, and two after the origin texel along each axis.
  let weight0 = fraction*(-0.5 + (fraction*(1.0 - (0.5*fraction))));
  let weight1 = 1.0 + ((fraction*fraction)*((1.5*fraction) - 2.5));
  let weight2 = fraction*(0.5 + (fraction*(2.0 - (1.5*fraction))));
  let weight3 = (fraction*fraction)*((0.5*fraction) - 0.5);

  // The middle two taps share one bilinear fetch placed between their texel centers by their weights.
  let weightMiddle = weight1 + weight2;
  let coordinateBefore = (origin - vec2f(0.5))*texel;
  let coordinateMiddle = (origin + vec2f(0.5) + (weight2/weightMiddle))*texel;
  let coordinateAfter = (origin + vec2f(2.5))*texel;

  let rows = array<f32, 3>(coordinateBefore.y, coordinateMiddle.y, coordinateAfter.y);
  let rowWeights = array<f32, 3>(weight0.y, weightMiddle.y, weight3.y);
  let columns = array<f32, 3>(coordinateBefore.x, coordinateMiddle.x, coordinateAfter.x);
  let columnWeights = array<f32, 3>(weight0.x, weightMiddle.x, weight3.x);
  var color = vec3f(0.0);
  var totalWeight = 0.0;
  for(var row = 0u; row < 3u; row++) {
    for(var column = 0u; column < 3u; column++) {
      let weight = rowWeights[row]*columnWeights[column];
      let tap = vec2f(columns[column], rows[row]);
      color+= textureSampleLevel(sceneTexture, linearSampler, tap, 0.0).rgb*weight;
      totalWeight+= weight;
    }
  }
  color = color/totalWeight;

  // Neighborhood clamp against the four texels surrounding the sample point.
  let lastTexel = vec2i(size) - vec2i(1);
  let nearest = vec2i(origin);
  let topLeft = textureLoad(sceneTexture, clamp(nearest, vec2i(0), lastTexel), 0).rgb;
  let topRight = textureLoad(sceneTexture, clamp(nearest + vec2i(1, 0), vec2i(0), lastTexel), 0).rgb;
  let bottomLeft = textureLoad(sceneTexture, clamp(nearest + vec2i(0, 1), vec2i(0), lastTexel), 0).rgb;
  let bottomRight = textureLoad(sceneTexture, clamp(nearest + vec2i(1, 1), vec2i(0), lastTexel), 0).rgb;
  let minimum = min(min(topLeft, topRight), min(bottomLeft, bottomRight));
  let maximum = max(max(topLeft, topRight), max(bottomLeft, bottomRight));
  return clamp(color, minimum, maximum);
}

/**
 * Darkening toward the frame's corners: none inside the radius (a fraction of the center-to-corner distance), easing
 * to `strength` at the corners. Distances are aspect-corrected so the falloff stays circular.
 */
fn vignette(coordinate: vec2f) -> f32 {
  let strength = resolve.vignette.x;
  if(strength <= 0.0) {
    return 1.0;
  }
  let aspect = vec2f(resolve.vignette.z, 1.0);
  let distance = length((coordinate - vec2f(0.5))*aspect)/length(vec2f(0.5)*aspect);
  let falloff = smoothstep(resolve.vignette.y, 1.0, distance);
  return max(1.0 - (strength*falloff*falloff), 0.0);
}

/**
 * Extend tone-mapped highlights into the display's HDR headroom. The filmic and AgX transforms end at SDR white, so on
 * an HDR display the brightest part of their shoulder is stretched toward the headroom: colors scale by a factor that
 * stays at one up to half of SDR white luminance and rises smoothly to the headroom at SDR white. Normally lit content,
 * and therefore the look tuned on standard displays, is unchanged; highlights keep their hue because every channel
 * scales together. With one (no) headroom this returns the color unchanged.
 */
fn extendHighlights(color: vec3f, headroom: f32) -> vec3f {
  let luminance = dot(color, vec3f(0.2126, 0.7152, 0.0722));
  let shoulder = smoothstep(0.5, 1.0, luminance);
  let extended = color*(1.0 + ((headroom - 1.0)*(shoulder*shoulder)));
  return min(extended, vec3f(headroom));
}

/**
 * Encode linear light with the exact IEC 61966-2-1 sRGB curve (`joy_core::linear_to_srgb`); values above one continue
 * the curve.
 */
fn encodeSrgb(linear: vec3f) -> vec3f {
  let curve = (1.055*pow(linear, vec3f(1.0/2.4))) - vec3f(0.055);
  return select(curve, linear*12.92, linear <= vec3f(0.0031308));
}

fn decodeSrgb(encoded: vec3f) -> vec3f {
  let curve = pow((encoded + vec3f(0.055))/1.055, vec3f(2.4));
  return select(curve, encoded/12.92, encoded <= vec3f(0.04045));
}

/**
 * Interleaved gradient noise in [0, 1) (Jimenez 2014): a stable, high-frequency pattern per pixel.
 */
fn interleavedGradientNoise(pixel: vec2f) -> f32 {
  return fract(52.9829189*fract(dot(pixel, vec2f(0.06711056, 0.00583715))));
}

/**
 * Expose, bloom, vignette, and tone map one output pixel, then encode it for the presentation target.
 */
@fragment fn fragmentMain(input: ResolveVarying) -> @location(0) vec4f {
  let exposure = resolve.display.x*textureLoad(exposureTexture, vec2i(0), 0).r;
  var color = sampleScene(input.coordinate)*exposure;
  if(resolve.display.w > 0.0) {
    color+= textureSampleLevel(bloomTexture, linearSampler, input.coordinate, 0.0).rgb*resolve.display.w;
  }
  color = color*vignette(input.coordinate);

  // Display transform. Filmic and AgX end at SDR white and extend into the headroom; no tonemapping does not.
  let headroom = resolve.output.x;
  color = toneMap(color, resolve.display.y, resolve.display.z, resolve.filmic);
  if(resolve.encoding.y < 0.5) {
    color = extendHighlights(color, headroom);
  }
  color = clamp(color, vec3f(0.0), vec3f(headroom));

  // Encode, dither by up to one code value, and hand linear targets linear light again.
  var encoded = encodeSrgb(color);
  let noise = interleavedGradientNoise(input.position.xy) - 0.5;
  encoded = max(encoded + vec3f(noise*resolve.encoding.x), vec3f(0.0));
  if(resolve.output.z > 0.5) {
    return vec4f(decodeSrgb(encoded)*resolve.output.w, 1.0);
  }

  return vec4f(encoded, 1.0);
}
