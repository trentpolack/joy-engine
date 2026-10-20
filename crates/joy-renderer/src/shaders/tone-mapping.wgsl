// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Display transforms for the resolve: each maps exposed, linear Rec.709 scene light to display-linear Rec.709 light,
// where one is SDR white.
//
// - Filmic: a parametric toe, linear section, and shoulder per channel (Uchimura 2017, "HDR Theory and Practice"), the
//   classic game curve with controls.
// - AgX: log-encoded sigmoid in an inset gamut, with Blender's punchy look as an option.
// - ACES 2.0: the Academy Output Transform, ported from OpenColorIO's reference implementation (BSD-3-Clause,
//   Copyright Contributors to the OpenColorIO Project; see crates/joy-renderer/src/tone_mapping/LICENSE-OpenColorIO).
//   Its per-display tables come from `tone_mapping/aces2.rs`, which documents the transform's steps.
//
// The including module declares `aces: AcesParameters`.

const TONE_MAPPER_NONE: f32 = 0.0;
const TONE_MAPPER_FILMIC: f32 = 1.0;
const TONE_MAPPER_AGX: f32 = 2.0;
const TONE_MAPPER_ACES: f32 = 3.0;

/**
 * The selected display transform of exposed scene light. `filmic` holds the curve's contrast, linear-section start,
 * linear-section length, and toe; `look` is one for AgX's punchy look. The result is display-linear and at most one
 * (SDR white), except ACES 2.0, which renders up to its display's peak.
 */
fn toneMap(color: vec3f, mapper: f32, look: f32, filmic: vec4f) -> vec3f {
  let exposed = max(color, vec3f(0.0));
  if(mapper == TONE_MAPPER_FILMIC) {
    return filmicToneMap(exposed, filmic);
  }
  if(mapper == TONE_MAPPER_AGX) {
    return agxToneMap(exposed, look);
  }
  if(mapper == TONE_MAPPER_ACES) {
    return acesToneMap(exposed);
  }
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

// ---------------------------------------------------------------------------------------------------------------------
// ACES 2.0
// ---------------------------------------------------------------------------------------------------------------------

// Entries per hue table (360 degrees plus wrapping) and the vectors that pack the reach table four to each.
const ACES_TABLE_ENTRIES: i32 = 363;
const ACES_REACH_VECTORS: i32 = 91;

// Model and compression constants (OpenColorIO `ACES2/Common.h`).
const ACES_COMPRESSION_OFFSET: f32 = 27.13;
const ACES_J_SCALE: f32 = 100.0;
const ACES_SMOOTH_CUSPS: f32 = 0.12;
const ACES_CUSP_MID_BLEND: f32 = 1.3;
const ACES_FOCUS_GAIN_BLEND: f32 = 0.3;
const ACES_COMPRESSION_THRESHOLD: f32 = 0.75;

struct AcesParameters {
  rec709ToAp1: mat3x3f,
  // AP1 to AP0 to the adapted CAM16 cone responses of the input.
  ap1ToCamera: mat3x3f,
  inputConeToAab: mat3x3f,
  outputAabToCone: mat3x3f,
  outputCameraToRgb: mat3x3f,
  // X F_L normalized, Y lightness exponent cz, Z its reciprocal, W achromatic white response.
  model: vec4f,
  // X reciprocal white response, Y AP1 input limit, Z output peak (one is SDR white), W J at the peak.
  limits: vec4f,
  // Tone scale n_r, g, t_1, and s_2.
  tonescale: vec4f,
  // X tone scale m_2, Y model gamma reciprocal, Z middle-gray J, W focus distance.
  compression: vec4f,
  // X chroma expansion, Y its threshold, Z chroma compression, W hue-norm scale.
  chroma: vec4f,
  // X lower-hull gamma reciprocal, YZ hue-search offsets below and above the nominal entry, W unused.
  gamut: vec4f,
  // Per hue entry: cusp J, cusp M, upper-hull gamma reciprocal, and the entry's hue in degrees.
  cusps: array<vec4f, ACES_TABLE_ENTRIES>,
  // Reach boundary M per degree, wrapped like the hue table, four per vector.
  reach: array<vec4f, ACES_REACH_VECTORS>,
};

/**
 * Post-adaptation cone response compression; the sign carries through.
 */
fn acesCompress(response: vec3f) -> vec3f {
  let powered = pow(abs(response), vec3f(0.42));
  return sign(response)*(powered/(vec3f(ACES_COMPRESSION_OFFSET) + powered));
}

fn acesExpand(compressed: vec3f) -> vec3f {
  let limited = min(abs(compressed), vec3f(0.99));
  return sign(compressed)*pow((ACES_COMPRESSION_OFFSET*limited)/(vec3f(1.0) - limited), vec3f(1.0/0.42));
}

fn acesCompressScalar(response: f32) -> f32 {
  let powered = pow(response, 0.42);
  return powered/(ACES_COMPRESSION_OFFSET + powered);
}

fn acesExpandScalar(compressed: f32) -> f32 {
  let limited = min(compressed, 0.99);
  return pow((ACES_COMPRESSION_OFFSET*limited)/(1.0 - limited), 1.0/0.42);
}

/**
 * Tone-mapped lightness J of a positive achromatic response: to luminance, through the Daniele Evo curve, and back.
 */
fn acesToneScale(achromatic: f32) -> f32 {
  let luminance = acesExpandScalar(aces.model.w*achromatic)/aces.model.x;
  let f = aces.compression.x*pow(luminance/(luminance + aces.tonescale.w), aces.tonescale.y);
  let mapped = max(0.0, (f*f)/(f + aces.tonescale.z))*aces.tonescale.x;
  let response = acesCompressScalar(mapped*aces.model.x);
  return ACES_J_SCALE*pow(response*aces.limits.x, aces.model.y);
}

/**
 * The reference's toe: values above `limit` pass through, values below roll off smoothly.
 */
fn acesToe(x: f32, limit: f32, k1In: f32, k2In: f32) -> f32 {
  if(x > limit) {
    return x;
  }
  let k2 = max(k2In, 0.001);
  let k1 = sqrt((k1In*k1In) + (k2*k2));
  let k3 = (limit + k1)/(limit + k2);
  let minusB = (k3*x) - k1;
  let minusAc = k2*k3*x;
  return 0.5*(minusB + sqrt((minusB*minusB) + (4.0*minusAc)));
}

/**
 * Hue-dependent colorfulness normalization: a third-order Fourier series in hue.
 */
fn acesChromaNorm(cosHue: f32, sinHue: f32) -> f32 {
  let cos2 = (2.0*cosHue*cosHue) - 1.0;
  let sin2 = 2.0*cosHue*sinHue;
  let cos3 = (4.0*cosHue*cosHue*cosHue) - (3.0*cosHue);
  let sin3 = (3.0*sinHue) - (4.0*sinHue*sinHue*sinHue);
  let norm = (11.34072*cosHue) + (16.46899*cos2) + (7.88380*cos3) + (14.66441*sinHue) - (6.37224*sin2)
    + (9.19364*sin3) + 77.12896;
  return norm*aces.chroma.w;
}

fn acesReachEntry(index: i32) -> f32 {
  return aces.reach[index/4][index%4];
}

/**
 * Reach boundary M at a hue in [0, 360), linear between whole degrees.
 */
fn acesReachM(hue: f32) -> f32 {
  let base = i32(hue);
  return mix(acesReachEntry(base + 1), acesReachEntry(base + 2), hue - f32(base));
}

/**
 * Index of the upper end of the hue-table interval containing `hue`, bisecting near its nominal entry.
 */
fn acesHueInterval(hue: f32) -> i32 {
  var index = i32(hue) + 1;
  var lower = max(index + i32(aces.gamut.y), 0);
  var upper = min(index + i32(aces.gamut.z), ACES_TABLE_ENTRIES - 2);
  loop {
    if(lower + 1 >= upper) {
      break;
    }
    if(hue > aces.cusps[index].w) {
      lower = index;
    } else {
      upper = index;
    }
    index = (lower + upper)/2;
  }
  return max(upper, 1);
}

fn acesSolveJIntersect(j: f32, m: f32, focusJ: f32, maxJ: f32, slopeGain: f32) -> f32 {
  let mScaled = m/slopeGain;
  let a = mScaled/focusJ;
  if(j < focusJ) {
    let b = 1.0 - mScaled;
    let c = -j;
    return (-2.0*c)/(b + sqrt((b*b) - (4.0*a*c)));
  }
  let b = -(1.0 + mScaled + (maxJ*a));
  let c = (maxJ*mScaled) + j;
  return (-2.0*c)/(b - sqrt((b*b) - (4.0*a*c)));
}

/**
 * M where the line J = slope * M + jAxisIntersect meets J = jMax * (M / mMax)^(1 / inverseGamma), approximately.
 */
fn acesBoundaryM(jAxisIntersect: f32, slope: f32, inverseGamma: f32, jMax: f32, mMax: f32, jReference: f32) -> f32 {
  let shifted = jReference*pow(jAxisIntersect/jReference, inverseGamma);
  return (shifted*mMax)/(jMax - (slope*mMax));
}

/**
 * Compress (J, M) into the display gamut along a line toward the hue's focus point.
 */
fn acesGamutCompress(j: f32, m: f32, hue: f32, reachMaxM: f32) -> vec2f {
  let limitJ = aces.limits.w;
  if(j <= 0.0) {
    return vec2f(0.0);
  }
  if((m <= 0.0) || (j > limitJ)) {
    return vec2f(j, 0.0);
  }

  // Interpolate the hue's cusp and upper-hull gamma.
  let upper = acesHueInterval(hue);
  let lowerEntry = aces.cusps[upper - 1];
  let upperEntry = aces.cusps[upper];
  let t = (hue - lowerEntry.w)/(upperEntry.w - lowerEntry.w);
  let cusp = mix(lowerEntry.xyz, upperEntry.xyz, t);
  let focusJ = mix(cusp.x, aces.compression.z, min(1.0, ACES_CUSP_MID_BLEND - (cusp.x/limitJ)));
  let threshold = mix(cusp.x, limitJ, ACES_FOCUS_GAIN_BLEND);

  // Focus gain, the compression line, and where it meets the gamut and reach boundaries.
  var slopeGain = limitJ*aces.compression.w;
  if(j > threshold) {
    // log10 through log2.
    let adjustment = log2((limitJ - threshold)/max(0.0001, limitJ - j))*0.30102999566;
    slopeGain = slopeGain*((adjustment*adjustment) + 1.0);
  }
  let jIntersect = acesSolveJIntersect(j, m, focusJ, limitJ, slopeGain);
  var direction = limitJ - jIntersect;
  if(jIntersect < focusJ) {
    direction = jIntersect;
  }
  let slope = (direction*(jIntersect - focusJ))/(focusJ*slopeGain);
  let jIntersectCusp = acesSolveJIntersect(cusp.x, cusp.y, focusJ, limitJ, slopeGain);
  let lowerM = acesBoundaryM(jIntersect, slope, aces.gamut.x, cusp.x, cusp.y, jIntersectCusp);
  let upperM = acesBoundaryM(limitJ - jIntersect, -slope, cusp.z, limitJ - cusp.x, cusp.y, limitJ - jIntersectCusp);
  let scaled = ACES_SMOOTH_CUSPS*cusp.y;
  let blend = max(scaled - abs(lowerM - upperM), 0.0)/scaled;
  let gamutM = min(lowerM, upperM) - (blend*blend*blend*scaled*(1.0/6.0));
  if(gamutM <= 0.0) {
    return vec2f(j, 0.0);
  }
  let reachM = acesBoundaryM(jIntersect, slope, aces.compression.y, limitJ, reachMaxM, limitJ);

  // Reinhard compression between the gamut and reach boundaries above a threshold.
  let proportion = max(gamutM/reachM, ACES_COMPRESSION_THRESHOLD);
  let start = proportion*gamutM;
  var mapped = m;
  if((m > start) && (proportion < 1.0)) {
    let reachOffset = reachM - start;
    let scale = reachOffset/((reachOffset/(gamutM - start)) - 1.0);
    let normalized = (m - start)/scale;
    mapped = start + ((scale*normalized)/(1.0 + normalized));
  }
  return vec2f(jIntersect + (mapped*slope), mapped);
}

/**
 * The ACES 2.0 Output Transform of exposed linear Rec.709 light, to display-linear Rec.709 clamped to the peak.
 */
fn acesToneMap(color: vec3f) -> vec3f {
  // Input: clamp in AP1, then the appearance model's Aab and JMh.
  let ap1 = clamp(aces.rec709ToAp1*color, vec3f(0.0), vec3f(aces.limits.y));
  let aab = aces.inputConeToAab*acesCompress(aces.ap1ToCamera*ap1);
  if(aab.x <= 0.0) {
    return vec3f(0.0);
  }
  let j = ACES_J_SCALE*pow(aab.x, aces.model.y);
  let m = length(aab.yz);
  var hue = degrees(atan2(aab.z, aab.y));
  if(hue < 0.0) {
    hue+= 360.0;
  }
  let cosHue = cos(radians(hue));
  let sinHue = sin(radians(hue));

  // Tone scale and chroma compression toward the reach boundary.
  let reachMaxM = acesReachM(hue);
  let mNorm = acesChromaNorm(cosHue, sinHue);
  let jToneMapped = acesToneScale(aab.x);
  let limitJ = aces.limits.w;
  var compressedM = m;
  if(m != 0.0) {
    let normalizedJ = jToneMapped/limitJ;
    let shadow = max(0.0, 1.0 - normalizedJ);
    let limit = (pow(normalizedJ, aces.compression.y)*reachMaxM)/mNorm;
    compressedM = (m*pow(jToneMapped/j, aces.compression.y))/mNorm;
    let saturationRolloff = sqrt((normalizedJ*normalizedJ) + aces.chroma.y);
    compressedM = limit - acesToe(limit - compressedM, limit - 0.001, shadow*aces.chroma.x, saturationRolloff);
    compressedM = acesToe(compressedM, limit, normalizedJ*aces.chroma.z, shadow)*mNorm;
  }

  // Gamut compression, then display RGB.
  let mapped = acesGamutCompress(jToneMapped, compressedM, hue, reachMaxM);
  let achromatic = pow(max(mapped.x, 0.0)/ACES_J_SCALE, aces.model.z);
  let display = aces.outputCameraToRgb*acesExpand(aces.outputAabToCone*vec3f(achromatic, mapped.y*cosHue, mapped.y*sinHue));
  return clamp(display, vec3f(0.0), vec3f(aces.limits.z));
}
