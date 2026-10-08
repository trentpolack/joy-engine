// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/*
 * Metallic/roughness microfacet lighting: GGX distribution, Smith visibility, Schlick Fresnel and energy-conserving Lambert diffuse.
 *  NOTE: No environment map.
 */

precision highp float;

layout(std140) uniform frameUniforms {
  mat4 viewProjection;
  vec4 eye;
  vec4 lightDirection;
  vec4 lightColor;
  vec4 ambientColor;
};

layout(std140) uniform materialUniforms {
  vec4 baseColor;
  vec4 emissiveMetallic;
  vec4 roughnessAlpha;
};

uniform sampler2D baseColorTexture;
uniform sampler2D normalTexture;
uniform sampler2D emissiveTexture;
uniform sampler2D metallicTexture;
uniform sampler2D roughnessTexture;
uniform sampler2D aoTexture;

in vec3 worldPosition;
in vec3 worldNormal;
in vec4 vertexColor;
in vec2 texCoord;
out vec4 fragmentColor;

void main() {
  // Sample before alpha discard so derivative-based tangent frames remain valid.
  vec4 sampledBase = texture(baseColorTexture, texCoord);
  vec3 base = baseColor.rgb*vertexColor.rgb*linearColor(sampledBase.rgb);
  float alpha = baseColor.a*vertexColor.a*sampledBase.a;
  vec3 emission = emissiveMetallic.rgb*linearColor(texture(emissiveTexture, texCoord).rgb);
  float metallic = clamp(emissiveMetallic.a*texture(metallicTexture, texCoord).r, 0.0, 1.0);
  float roughness = clamp(roughnessAlpha.x*texture(roughnessTexture, texCoord).r, 0.045, 1.0);
  float occlusion = texture(aoTexture, texCoord).r;
  vec3 tangentNormal = texture(normalTexture, texCoord).xyz*2.0 - 1.0;
  vec3 n = normalize(worldNormal)*(gl_FrontFacing ? 1.0 : -1.0);
  vec3 dpX = dFdx(worldPosition);
  vec3 dpY = dFdy(worldPosition);
  vec2 uvX = dFdx(texCoord);
  vec2 uvY = dFdy(texCoord);
  float determinant = uvX.x*uvY.y - uvX.y*uvY.x;

  vec3 tangent = dpX*uvY.y - dpY*uvX.y;
  tangent-= n*dot(n, tangent);

  if(roughnessAlpha.w > 0.5 && abs(determinant) > 0.0000001 && dot(tangent, tangent) > 0.0000001) {
    tangent = normalize(tangent)*sign(determinant);
    vec3 bitangent = normalize(cross(n, tangent))*sign(determinant);
    n = normalize(tangent*tangentNormal.x + bitangent*tangentNormal.y + n*tangentNormal.z);
  }

  if(roughnessAlpha.y < 0.5) {
    alpha = 1.0;
  } else if(roughnessAlpha.y < 1.5) {
    if(alpha < roughnessAlpha.z) {
      discard;
    }
    alpha = 1.0;
  }

  vec3 v = normalize(eye.xyz - worldPosition);
  vec3 l = normalize(lightDirection.xyz);
  vec3 h = normalize(v + l);
  float nv = max(dot(n, v), 0.0001);
  float nl = max(dot(n, l), 0.0);
  float nh = max(dot(n, h), 0.0);
  float vh = max(dot(v, h), 0.0);
  float a2 = pow(roughness, 4.0);
  float distributionDenominator = nh*nh*(a2 - 1.0) + 1.0;
  float distribution = a2/max(PI*distributionDenominator*distributionDenominator, 0.000000000001);
  float visibility = 0.5/max(nl*sqrt(nv*nv*(1.0 - a2) + a2) + nv*sqrt(nl*nl*(1.0 - a2) + a2), 0.0001);
  vec3 f0 = mix(vec3(0.04), base, metallic);
  vec3 fresnel = f0 + (1.0 - f0)*pow(1.0 - vh, 5.0);
  vec3 diffuse = (1.0 - fresnel)*(1.0 - metallic)*base/PI;

  vec3 radiance = (diffuse + distribution*visibility*fresnel)*lightColor.rgb*nl;
  radiance+= ambientColor.rgb*base*(1.0 - metallic)*occlusion + emission;

  // HDR sessions defer tone mapping to the shared postprocessor.
  fragmentColor = vec4((ambientColor.a > 0.5) ? displayColor(radiance) : radiance, alpha);
}
