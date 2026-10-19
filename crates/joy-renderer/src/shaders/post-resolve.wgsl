// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Display resolve: upscales the scene when it renders below the output size, composes bloom (glow plus starburst
// streaks) and the vignette edge blur into the linear HDR scene, applies exposure and the selected tone mapper, extends
// highlights into any HDR headroom, encodes the result as sRGB, then applies the grade (saturation, brightness/contrast,
// vibrance) and vignette to the encoded display values. When a blur follows bloom, the bloom composite runs earlier (see
// `compositeMain` in post-bloom.wgsl) and the bloom strength here is zero.
//
// The display target holds sRGB-encoded values: 8-bit encoded storage keeps dark gradients from banding on standard
// displays, a float target carries encoded values above SDR white on HDR displays, and the grade, antialiasing, and
// grain work on the perceptual values a viewer sees (they were tuned on them).

struct Resolve {
  // X exposure, Y tone mapper (0 none, 1 ACES, 2 AgX), Z AgX look (0 neutral, 1 punchy), W bloom strength (zero when
  // bloom is off or already composed).
  display: vec4f,
  // X saturation (signed adjustment), Y brightness offset, Z contrast (signed adjustment), W vibrance.
  grade: vec4f,
  // X vignette radius, Y vignette strength, Z vignette blur amount, W unused.
  vignette: vec4f,
  // XY output size in pixels, Z time in seconds, W unused.
  viewport: vec4f,
  // X display headroom (multiple of SDR white, 1 for standard output), Y its sRGB-encoded value (the largest value the
  // display chain keeps), Z 1 when the scene renders below the output size and is upscaled, W unused.
  output: vec4f,
  // XY scene size in pixels, ZW its reciprocal.
  scene: vec4f,
};

@group(0) @binding(0) var<uniform> resolve: Resolve;
@group(0) @binding(1) var sceneTexture: texture_2d<f32>;
@group(0) @binding(2) var bloomTexture: texture_2d<f32>;
@group(0) @binding(3) var edgeBlurTexture: texture_2d<f32>;
@group(0) @binding(4) var linearSampler: sampler;
@group(0) @binding(5) var lensTexture: texture_2d<f32>;

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
 * Scene color at a display coordinate. A scene rendered below the output size is upscaled with a Catmull-Rom bicubic
 * filter, which keeps edges and texture detail sharper than bilinear filtering; nine bilinear fetches cover its sixteen
 * taps. The result is clamped to the four nearest texels so the filter's negative lobes cannot ring around bright
 * highlights.
 */
fn sampleScene(coordinate: vec2f) -> vec3f {
  if(resolve.output.z < 0.5) {
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
 * Extend tone-mapped highlights into the display's HDR headroom. The tone mappers end at SDR white, so on an HDR display
 * the brightest part of their shoulder is stretched toward the headroom: colors scale by a factor that stays at one up
 * to half of SDR white luminance and rises smoothly to the headroom at SDR white. Normally lit content, and therefore
 * the look tuned on standard displays, is unchanged; highlights keep their hue because every channel scales together.
 * With one (no) headroom this returns the color unchanged.
 */
fn extendHighlights(color: vec3f, headroom: f32) -> vec3f {
  let luminance = dot(color, vec3f(0.2126, 0.7152, 0.0722));
  let shoulder = smoothstep(0.5, 1.0, luminance);
  let extended = color*(1.0 + ((headroom - 1.0)*(shoulder*shoulder)));
  return min(extended, vec3f(headroom));
}

/**
 * Encode linear light with the exact IEC 61966-2-1 sRGB curve (`joy_core::linear_to_srgb`); values above one continue
 * the curve and negative values mirror it, so the grade can still pull them back before the final clamp.
 */
fn encodeSrgb(linear: vec3f) -> vec3f {
  let magnitude = abs(linear);
  let curve = (1.055*pow(magnitude, vec3f(1.0/2.4))) - vec3f(0.055);
  let segment = magnitude*12.92;
  return sign(linear)*select(curve, segment, magnitude <= vec3f(0.0031308));
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
  var color = sampleScene(input.coordinate);

  // Bloom: reconstructed glow plus starburst streaks, scaled by strength.
  if(resolve.display.w > 0.0) {
    let glow = textureSampleLevel(bloomTexture, linearSampler, input.coordinate, 0.0).rgb;
    let lens = textureSampleLevel(lensTexture, linearSampler, input.coordinate, 0.0).rgb;
    color+= (glow + lens)*resolve.display.w;
  }

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

  // Display transform. Without a tone mapper, scene light shows as it is up to the display's peak.
  let exposure = resolve.display.x;
  let headroom = resolve.output.x;
  if(resolve.display.y > 1.5) {
    color = extendHighlights(agxToneMap(color, exposure, resolve.display.z), headroom);
  } else if(resolve.display.y > 0.5) {
    color = extendHighlights(acesToneMap(color, exposure), headroom);
  } else {
    color = min(max(color*exposure, vec3f(0.0)), vec3f(headroom));
  }

  // Encode for display; everything after this works on the values the viewer sees.
  color = encodeSrgb(color);

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

  return vec4f(clamp(color, vec3f(0.0), vec3f(resolve.output.y)), 1.0);
}
