// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Display resolve: composes bloom and the vignette edge blur into the linear HDR scene, applies exposure and the
// selected tone mapper, then the grade (saturation, brightness/contrast, vibrance) and vignette. The operators and their
// order match the TypeScript engine's post-processing chain.

struct Resolve {
  // X exposure, Y tone mapper (0 none, 1 ACES, 2 AgX), Z AgX look (0 neutral, 1 punchy), W bloom strength.
  display: vec4f,
  // X saturation (signed adjustment), Y brightness offset, Z contrast (signed adjustment), W vibrance.
  grade: vec4f,
  // X vignette radius, Y vignette strength, Z vignette blur amount, W unused.
  vignette: vec4f,
  // XY target size in pixels, Z time in seconds, W unused.
  viewport: vec4f,
};

@group(0) @binding(0) var<uniform> resolve: Resolve;
@group(0) @binding(1) var sceneTexture: texture_2d<f32>;
@group(0) @binding(2) var bloomTexture: texture_2d<f32>;
@group(0) @binding(3) var edgeBlurTexture: texture_2d<f32>;
@group(0) @binding(4) var linearSampler: sampler;

struct ResolveOutput {
  @builtin(position) position: vec4f,
  @location(0) coordinate: vec2f,
};

/**
 * Full-screen triangle with top-left texture coordinates.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> ResolveOutput {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));

  var output: ResolveOutput;
  output.position = vec4f((position*2.0) - 1.0, 0.0, 1.0);
  output.coordinate = vec2f(position.x, 1.0 - position.y);
  return output;
}

/**
 * AgX sigmoid contrast curve in the encoded working space (Horner-form polynomial).
 */
fn agxContrast(value: vec3f) -> vec3f {
  let valueSquared = value*value;
  let valueFourth = valueSquared*valueSquared;
  return (15.5*valueFourth*valueSquared) - (40.14*valueFourth*value) + (31.96*valueFourth)
    - (6.868*valueSquared*value) + (0.4298*valueSquared) + (0.1191*value) - 0.00232;
}

/**
 * AgX display transform: inset gamut, log encode, contrast shape, optional punchy look, and return to linear sRGB.
 */
fn agxToneMap(color: vec3f, exposure: f32, look: f32) -> vec3f {
  let inset = mat3x3f(
    0.842479, 0.042328, 0.042375,
    0.078433, 0.878468, 0.078434,
    0.079224, 0.079166, 0.879142
  );
  let outset = mat3x3f(
    1.196879, -0.052896, -0.052972,
    -0.098020, 1.151904, -0.098043,
    -0.099029, -0.098961, 1.151073
  );

  var value = inset*max(color*exposure, vec3f(0.0));
  value = clamp((log2(max(value, vec3f(0.000001))) + 12.47393)/16.5, vec3f(0.0), vec3f(1.0));
  value = agxContrast(value);

  // Punchy look: luminance-preserving contrast and saturation, blended by the look amount.
  let luminance = dot(value, vec3f(0.2126, 0.7152, 0.0722));
  let punchy = mix(vec3f(luminance), pow(max(value, vec3f(0.0)), vec3f(1.35)), 1.4);
  value = mix(value, punchy, look);
  value = max(outset*value, vec3f(0.0));
  return pow(value, vec3f(2.2));
}

/**
 * Fitted ACES filmic curve with the default (unit) maximum luminance.
 */
fn acesToneMap(color: vec3f, exposure: f32) -> vec3f {
  let exposed = max(color*exposure, vec3f(0.0));
  let numerator = exposed*((2.51*exposed) + vec3f(0.03));
  let denominator = (exposed*((2.43*exposed) + vec3f(0.59))) + vec3f(0.14);
  return clamp(numerator/denominator, vec3f(0.0), vec3f(1.0));
}

/**
 * Saturation toward the channel average; positive values expand, negative values desaturate.
 */
fn adjustSaturation(color: vec3f, saturation: f32) -> vec3f {
  let average = (color.r + color.g + color.b)/3.0;
  if(saturation > 0.0) {
    return color + ((vec3f(average) - color)*(1.0 - (1.0/(1.001 - saturation))));
  }
  return color + ((vec3f(average) - color)*(-saturation));
}

/**
 * Brightness offset followed by contrast around middle gray.
 */
fn adjustBrightnessContrast(color: vec3f, brightness: f32, contrast: f32) -> vec3f {
  let brightened = color + vec3f(brightness);
  if(contrast > 0.0) {
    return ((brightened - vec3f(0.5))/(1.0 - contrast)) + vec3f(0.5);
  }
  return ((brightened - vec3f(0.5))*(1.0 + contrast)) + vec3f(0.5);
}

/**
 * Vibrance: muted colors move more than already saturated ones.
 */
fn adjustVibrance(color: vec3f, amount: f32) -> vec3f {
  let average = (color.r + color.g + color.b)/3.0;
  let maximum = max(color.r, max(color.g, color.b));
  let weight = (maximum - average)*(-amount*3.0);
  return mix(color, vec3f(maximum), weight);
}

/**
 * Compose, tone map, grade, and vignette one display pixel.
 */
@fragment fn fragmentMain(input: ResolveOutput) -> @location(0) vec4f {
  var color = textureSampleLevel(sceneTexture, linearSampler, input.coordinate, 0.0).rgb;

  // Edge blur blends a pre-blurred copy near the frame boundary; aspect correction keeps the mask circular.
  if(resolve.vignette.z > 0.0) {
    let size = resolve.viewport.xy;
    let aspectScale = size/max(min(size.x, size.y), 1.0);
    let edgeDistance = length((input.coordinate - vec2f(0.5))*aspectScale);
    let edgeStart = resolve.vignette.x;
    let edgeEnd = max(0.82, edgeStart + 0.01);
    let edgeWeight = smoothstep(edgeStart, edgeEnd, edgeDistance)*resolve.vignette.z;
    let blurred = textureSampleLevel(edgeBlurTexture, linearSampler, input.coordinate, 0.0).rgb;
    color = mix(color, blurred, edgeWeight);
  }

  if(resolve.display.w > 0.0) {
    color+= textureSampleLevel(bloomTexture, linearSampler, input.coordinate, 0.0).rgb*resolve.display.w;
  }

  // Display transform.
  let exposure = resolve.display.x;
  if(resolve.display.y > 1.5) {
    color = agxToneMap(color, exposure, resolve.display.z);
  } else if(resolve.display.y > 0.5) {
    color = acesToneMap(color, exposure);
  } else {
    color = max(color*exposure, vec3f(0.0));
  }

  // Grade.
  color = adjustSaturation(color, resolve.grade.x);
  color = adjustBrightnessContrast(color, resolve.grade.y, resolve.grade.z);
  color = adjustVibrance(color, resolve.grade.w);

  // Vignette darkens toward black outside the configured radius.
  if(resolve.vignette.y > 0.0) {
    let distance = length(input.coordinate - vec2f(0.5));
    let ratio = smoothstep(0.8, resolve.vignette.x*0.799, distance*(resolve.vignette.y + resolve.vignette.x));
    color = color*ratio;
  }

  return vec4f(clamp(color, vec3f(0.0), vec3f(1.0)), 1.0);
}
