// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct agxToneMappingUniforms {
  exposure: f32,
  look: f32,
};

@group(0) @binding(auto) var<uniform> agxToneMapping: agxToneMappingUniforms;

/**
 * Approximates the contrast of a color with AGX tone mapping.
 * @param value The input color.
 * @returns The contrast-adjusted color.
 */
fn agxToneMapping_contrastApproximation(
  value: vec3f
  ) -> vec3f {
  let valueSquared = value * value;
  let valueFourth = valueSquared * valueSquared;
  return(15.5 * valueFourth * valueSquared - 40.14 * valueFourth * value +
    31.96 * valueFourth - 6.868 * valueSquared * value +
    0.4298 * valueSquared + 0.1191 * value - 0.00232);
}

fn agxToneMapping_applyPunchyLook(
  color: vec3f
  ) -> vec3f {
  let luminanceWeights = vec3f(0.2126, 0.7152, 0.0722);
  let luminance = dot(color, luminanceWeights);
  let contrastColor = pow(max(color, vec3f(0.0)), vec3f(1.35));
  return mix(vec3f(luminance), contrastColor, 1.4);
}

fn agxToneMapping_filterColor_ext(
  color: vec4f,
  texSize: vec2f,
  texCoord: vec2f
  ) -> vec4f {
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
  var value = inset * max(color.rgb * agxToneMapping.exposure, vec3f(0.0));
  value = clamp((log2(max(value, vec3f(0.000001))) + 12.47393) / 16.5, vec3f(0.0), vec3f(1.0));
  value = agxToneMapping_contrastApproximation(value);
  value = mix(value, agxToneMapping_applyPunchyLook(value), agxToneMapping.look);
  value = max(outset * value, vec3f(0.0));
  return vec4f(pow(value, vec3f(2.2)), color.a);
}
