// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/*
 * Metallic/roughness microfacet lighting: GGX distribution, Smith visibility, Schlick Fresnel and energy-conserving Lambert diffuse.
 *  NOTE: No environment map.
 */

precision highp float;
struct PointLight {vec4 positionRange; vec4 color;};
layout(std140) uniform pbrLighting {
  mat4 lightingViewMatrix;
  vec4 environment;
  PointLight lights[32];
  vec4 counts;
};
vec3 pbrDirect(vec3 base, float metallic, float roughness, vec3 n, vec3 v, vec3 l);


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
  vec3 radiance = pbrDirect(base, metallic, roughness, n, v, normalize(lightDirection.xyz))*lightColor.rgb;
  for(int index = 0; index < int(counts.x); index++) {
    PointLight light = lights[index];
    vec3 offset = light.positionRange.xyz - worldPosition;
    float distance = length(offset);
    vec3 direction = offset/max(distance, 0.000001);
    float reach = max(0.0, 1.0 - distance/light.positionRange.w);
    radiance+= pbrDirect(base, metallic, roughness, n, v, direction)*light.color.rgb*reach*reach;
  }
  radiance+= (ambientColor.rgb*base*(1.0 - metallic)*occlusion + emission)*counts.y;
  if(environment.x > 0.0) {
    float cosine = cos(environment.y);
    float sine = sin(environment.y);
    vec3 reflection = reflect(-v, n);
    vec3 environmentNormal = vec3(cosine*n.x - sine*n.z, n.y, sine*n.x + cosine*n.z);
    vec3 environmentReflection = vec3(cosine*reflection.x - sine*reflection.z, reflection.y, sine*reflection.x + cosine*reflection.z);
    vec3 diffuseLight = textureLod(pbr_diffuseEnvSampler, environmentNormal, 0.0).rgb;
    vec3 specularLight = textureLod(pbr_specularEnvSampler, environmentReflection, roughness*environment.z).rgb;
    float nv = max(dot(n, v), 0.0001);
    vec2 brdf = textureLod(pbr_brdfLUT, vec2(nv, roughness), 0.0).rg;
    vec3 f0 = mix(vec3(0.04), base, metallic);
    vec3 fresnel = f0 + (max(vec3(1.0 - roughness), f0) - f0)*pow(1.0 - nv, 5.0);
    vec3 diffuse = (vec3(1) - fresnel)*(1.0 - metallic)*base*diffuseLight;
    radiance+= (diffuse + specularLight*(f0*brdf.x + brdf.y))*occlusion*environment.x;
  }

  // HDR sessions defer tone mapping to the shared postprocessor.
  fragmentColor = vec4((ambientColor.a > 0.5) ? displayColor(radiance) : radiance, alpha);
}

vec3 pbrDirect(vec3 base, float metallic, float roughness, vec3 n, vec3 v, vec3 l) {
  vec3 halfDirection = v + l;
  vec3 h = halfDirection/max(length(halfDirection), 0.000001);
  float nv = max(dot(n, v), 0.0001);
  float nl = max(dot(n, l), 0.0);
  float nh = max(dot(n, h), 0.0);
  float vh = max(dot(v, h), 0.0);
  float a2 = pow(roughness, 4.0);
  float denominator = nh*nh*(a2 - 1.0) + 1.0;
  float distribution = a2/max(PI*denominator*denominator, 0.000000000001);
  float visibility = 0.5/max(nl*sqrt(nv*nv*(1.0 - a2) + a2) + nv*sqrt(nl*nl*(1.0 - a2) + a2), 0.0001);
  vec3 f0 = mix(vec3(0.04), base, metallic);
  vec3 fresnel = f0 + (1.0 - f0)*pow(1.0 - vh, 5.0);
  vec3 diffuse = (1.0 - fresnel)*(1.0 - metallic)*base/PI;
  return (diffuse + distribution*visibility*fresnel)*nl;
}
