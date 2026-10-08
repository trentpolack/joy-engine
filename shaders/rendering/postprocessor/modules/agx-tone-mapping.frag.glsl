// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

layout(std140) uniform agxToneMappingUniforms {
  float exposure;
  float look;
} agxToneMapping;

vec3 agxToneMapping_contrastApproximation(vec3 value) {
  vec3 valueSquared = value * value;
  vec3 valueFourth = valueSquared * valueSquared;
  return 15.5 * valueFourth * valueSquared - 40.14 * valueFourth * value +
    31.96 * valueFourth - 6.868 * valueSquared * value +
    0.4298 * valueSquared + 0.1191 * value - 0.00232;
}

vec3 agxToneMapping_applyPunchyLook(vec3 color) {
  const vec3 luminanceWeights = vec3(0.2126, 0.7152, 0.0722);
  float luminance = dot(color, luminanceWeights);
  vec3 contrastColor = pow(max(color, vec3(0.0)), vec3(1.35));
  return mix(vec3(luminance), contrastColor, 1.4);
}

vec4 agxToneMapping_filterColor_ext(vec4 color, vec2 texSize, vec2 texCoord) {
  const mat3 inset = mat3(
    0.842479, 0.042328, 0.042375,
    0.078433, 0.878468, 0.078434,
    0.079224, 0.079166, 0.879142
  );
  const mat3 outset = mat3(
    1.196879, -0.052896, -0.052972,
    -0.098020, 1.151904, -0.098043,
    -0.099029, -0.098961, 1.151073
  );
  vec3 value = inset * max(color.rgb * agxToneMapping.exposure, vec3(0.0));
  value = clamp((log2(max(value, vec3(0.000001))) + 12.47393) / 16.5, 0.0, 1.0);
  value = agxToneMapping_contrastApproximation(value);
  value = mix(value, agxToneMapping_applyPunchyLook(value), agxToneMapping.look);
  value = max(outset * value, vec3(0.0));
  return vec4(pow(value, vec3(2.2)), color.a);
}
