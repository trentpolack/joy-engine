// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Final display pass: edge antialiasing blended by strength, then animated luminance-aware film grain. Reads the
// resolved, sRGB-encoded display image and writes the presentation target. Non-sRGB targets receive the encoded values
// as they are; an sRGB target view encodes on write, so the pass decodes first and the stored bytes are identical.

struct Presentation {
  // XY reciprocal target size, Z antialias strength, W grain intensity.
  frame: vec4f,
  // X grain cell size in pixels, Y grain speed in cycles per second, Z decorrelated variant (0 or 1), W time seconds.
  grain: vec4f,
  // X is 1 when the presentation target view encodes sRGB itself; YZW unused.
  output: vec4f,
};

@group(0) @binding(0) var<uniform> presentation: Presentation;
@group(0) @binding(1) var displayTexture: texture_2d<f32>;
@group(0) @binding(2) var linearSampler: sampler;

struct FinalOutput {
  @builtin(position) position: vec4f,
  @location(0) coordinate: vec2f,
};

/**
 * Full-screen triangle with top-left texture coordinates.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> FinalOutput {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));

  var output: FinalOutput;
  output.position = vec4f((position*2.0) - 1.0, 0.0, 1.0);
  output.coordinate = vec2f(position.x, 1.0 - position.y);
  return output;
}

/**
 * Perceptual luminance used for edge detection.
 */
fn luma(color: vec3f) -> f32 {
  return dot(color, vec3f(0.2126, 0.7152, 0.0722));
}

/**
 * Engine-owned fast approximate antialiasing: estimates the local edge direction from four diagonal neighbors and
 * blends samples along it, rejecting the wide blend when it would leave the local luminance range.
 */
fn antialias(coordinate: vec2f, texel: vec2f) -> vec3f {
  let center = textureSampleLevel(displayTexture, linearSampler, coordinate, 0.0).rgb;
  let lumaNorthWest = luma(textureSampleLevel(displayTexture, linearSampler, coordinate + (texel*vec2f(-1.0, -1.0)), 0.0).rgb);
  let lumaNorthEast = luma(textureSampleLevel(displayTexture, linearSampler, coordinate + (texel*vec2f(1.0, -1.0)), 0.0).rgb);
  let lumaSouthWest = luma(textureSampleLevel(displayTexture, linearSampler, coordinate + (texel*vec2f(-1.0, 1.0)), 0.0).rgb);
  let lumaSouthEast = luma(textureSampleLevel(displayTexture, linearSampler, coordinate + (texel*vec2f(1.0, 1.0)), 0.0).rgb);
  let lumaCenter = luma(center);
  let lumaMinimum = min(lumaCenter, min(min(lumaNorthWest, lumaNorthEast), min(lumaSouthWest, lumaSouthEast)));
  let lumaMaximum = max(lumaCenter, max(max(lumaNorthWest, lumaNorthEast), max(lumaSouthWest, lumaSouthEast)));

  // Skip flat regions so texture detail and gradients stay sharp.
  if((lumaMaximum - lumaMinimum) < max(0.0312, lumaMaximum*0.125)) {
    return center;
  }

  var direction = vec2f(
    -((lumaNorthWest + lumaNorthEast) - (lumaSouthWest + lumaSouthEast)),
    (lumaNorthWest + lumaSouthWest) - (lumaNorthEast + lumaSouthEast)
  );
  let reduce = max((lumaNorthWest + lumaNorthEast + lumaSouthWest + lumaSouthEast)*0.03125, 0.0078125);
  let reciprocal = 1.0/(min(abs(direction.x), abs(direction.y)) + reduce);
  direction = clamp(direction*reciprocal, vec2f(-8.0), vec2f(8.0))*texel;

  let near = 0.5*(
    textureSampleLevel(displayTexture, linearSampler, coordinate + (direction*((1.0/3.0) - 0.5)), 0.0).rgb +
    textureSampleLevel(displayTexture, linearSampler, coordinate + (direction*((2.0/3.0) - 0.5)), 0.0).rgb
  );
  let wide = (near*0.5) + (0.25*(
    textureSampleLevel(displayTexture, linearSampler, coordinate - (direction*0.5), 0.0).rgb +
    textureSampleLevel(displayTexture, linearSampler, coordinate + (direction*0.5), 0.0).rgb
  ));
  let lumaWide = luma(wide);
  if((lumaWide < lumaMinimum) || (lumaWide > lumaMaximum)) {
    return near;
  }
  return wide;
}

/**
 * Hash used by the grain; matches the TypeScript engine's portable noise function.
 */
fn rand(value: vec2f) -> f32 {
  return fract(sin(dot(value, vec2f(12.9898, 78.233)))*43758.547);
}

/**
 * Decode sRGB-encoded display values with the exact IEC 61966-2-1 curve (`joy_core::srgb_to_linear`).
 */
fn decodeSrgb(encoded: vec3f) -> vec3f {
  let curve = pow((encoded + vec3f(0.055))/1.055, vec3f(2.4));
  let segment = encoded/12.92;
  return select(curve, segment, encoded <= vec3f(0.04045));
}

/**
 * Antialias, then add midtone-weighted animated grain in quantized pixel cells.
 */
@fragment fn fragmentMain(input: FinalOutput) -> @location(0) vec4f {
  let texel = presentation.frame.xy;
  let source = textureSampleLevel(displayTexture, linearSampler, input.coordinate, 0.0).rgb;
  var color = source;
  if(presentation.frame.z > 0.0) {
    color = mix(source, antialias(input.coordinate, texel), presentation.frame.z);
  }

  if(presentation.frame.w > 0.0) {
    let size = vec2f(1.0)/texel;
    let grainCell = floor((input.coordinate*size)/max(presentation.grain.x, 0.5));
    let grainFrame = floor(presentation.grain.w*presentation.grain.y);
    let analogNoise = rand(grainCell + vec2f(grainFrame*37.0, grainFrame*17.0));
    let offsetNoise = rand(grainCell.yx + vec2f(grainFrame*11.0, grainFrame*29.0) + 19.19);
    let noiseValue = mix(analogNoise, (analogNoise + offsetNoise)*0.5, presentation.grain.z);
    let luminance = luma(color);
    let midtoneWeight = 1.0 - smoothstep(0.35, 1.0, abs((luminance*2.0) - 1.0));
    let grain = (noiseValue - 0.5)*presentation.frame.w*mix(0.6, 1.0, midtoneWeight);
    color = clamp(color + vec3f(grain), vec3f(0.0), vec3f(1.0));
  }

  if(presentation.output.x > 0.5) {
    color = decodeSrgb(color);
  }
  return vec4f(color, 1.0);
}
