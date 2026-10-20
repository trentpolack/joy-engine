// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Display transforms for the resolve: each maps exposed, linear Rec.709 scene light to display-linear Rec.709 light,
// where one is SDR white.
//
// - Filmic: a parametric toe, linear section, and shoulder per channel (Uchimura 2017, "HDR Theory and Practice"), the
//   classic game curve with controls.
// - AgX: log-encoded sigmoid in an inset gamut, with Blender's punchy look as an option.
const TONE_MAPPER_NONE: f32 = 0.0;
const TONE_MAPPER_FILMIC: f32 = 1.0;
const TONE_MAPPER_AGX: f32 = 2.0;

/**
 * The selected display transform of exposed scene light. `filmic` holds the curve's contrast, linear-section start,
 * linear-section length, and toe; `look` is one for AgX's punchy look. The result is display-linear and at most one
 * (SDR white).
 */
fn toneMap(color: vec3f, mapper: f32, look: f32, filmic: vec4f) -> vec3f {
  let exposed = max(color, vec3f(0.0));
  if(mapper == TONE_MAPPER_FILMIC) {
    return filmicToneMap(exposed, filmic);
  } else if(mapper == TONE_MAPPER_AGX) {
    return agxToneMap(exposed, look);
  }

  // No tonemapper.
  return exposed;
}

// ---------------------------------------------------------------------------------------------------------------------
// Filmic
// ---------------------------------------------------------------------------------------------------------------------

/**
 * Uchimura's curve with a peak of one: a power toe below `linearStart`, a straight section of slope `contrast` for
 * `linearLength` of the remaining range, and an exponential shoulder that approaches one. Applied per channel, so very
 * bright colors desaturate toward white.
 */
fn filmicToneMap(color: vec3f, curve: vec4f) -> vec3f {
  let contrast = curve.x;
  let linearStart = curve.y;
  let linearLength = curve.z;
  let toe = curve.w;
  let peak = 1.0;

  let shoulderLength = ((peak - linearStart)*linearLength)/contrast;
  let shoulderStart = linearStart + shoulderLength;
  let shoulderValue = linearStart + (contrast*shoulderLength);
  let shoulderRate = -((contrast*peak)/(peak - shoulderValue))/peak;

  let toeWeight = vec3f(1.0) - smoothstep(vec3f(0.0), vec3f(linearStart), color);
  let shoulderWeight = step(vec3f(shoulderStart), color);
  let linearWeight = vec3f(1.0) - toeWeight - shoulderWeight;

  let toeCurve = linearStart*pow(color/linearStart, vec3f(toe));
  let linearCurve = vec3f(linearStart) + (contrast*(color - vec3f(linearStart)));
  let shoulderCurve = vec3f(peak) - ((peak - shoulderValue)*exp(shoulderRate*(color - vec3f(shoulderStart))));
  return (toeCurve*toeWeight) + (linearCurve*linearWeight) + (shoulderCurve*shoulderWeight);
}

// ---------------------------------------------------------------------------------------------------------------------
// AgX
// ---------------------------------------------------------------------------------------------------------------------

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
 * AgX: inset gamut, log encode, contrast shape, the optional punchy look, then back to linear Rec.709.
 */
fn agxToneMap(color: vec3f, look: f32) -> vec3f {
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

  var value = inset*color;
  value = clamp((log2(max(value, vec3f(0.000001))) + 12.47393)/16.5, vec3f(0.0), vec3f(1.0));
  value = agxContrast(value);

  // Punchy look: power 1.35 and saturation 1.4 around luminance.
  let luminance = dot(value, vec3f(0.2126, 0.7152, 0.0722));
  let punchy = mix(vec3f(luminance), pow(max(value, vec3f(0.0)), vec3f(1.35)), 1.4);
  value = mix(value, punchy, look);
  value = max(outset*value, vec3f(0.0));
  return pow(value, vec3f(2.2));
}
