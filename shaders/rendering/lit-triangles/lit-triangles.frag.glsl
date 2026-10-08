// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

precision highp float;

// Matches the two-vec4 light records packed by createPointLightUniforms.
struct PointLight {
  vec4 positionRange;
  vec4 color;
};

layout(std140) uniform uniforms {
  mat4 viewProjection;
  vec4 tint;
  vec4 lighting;
  PointLight lights[32];
  vec4 atmosphereDirectionIntensity;
  vec4 atmosphereColor;
  vec4 skylightColorIntensity;
};

in vec4 vertexColor;
in vec3 worldPosition;
out vec4 fragmentColor;

void main() {
  vec3 surface = cross(dFdx(worldPosition), dFdy(worldPosition));
  vec3 normal = surface / max(length(surface), 0.000001);
  vec3 irradiance = vec3(lighting.y) + skylightColorIntensity.rgb*skylightColorIntensity.a;
  float atmosphereDiffuse = abs(dot(normal, atmosphereDirectionIntensity.xyz));
  irradiance+= atmosphereColor.rgb*atmosphereDirectionIntensity.a*atmosphereDiffuse;

  for(int index = 0; index < 32; index++) {
    if(index >= int(lighting.x)) {
      break;
    }

    vec3 offset = lights[index].positionRange.xyz - worldPosition;
    float distance = length(offset);
    vec3 direction = offset / max(distance, 0.000001);
    float reach = max(0.0, 1.0 - distance / lights[index].positionRange.w);
    float diffuse = abs(dot(normal, direction));

    irradiance+= lights[index].color.rgb*pow(reach, 2.0)*diffuse;
  }

  fragmentColor = vec4(vertexColor.rgb * irradiance, vertexColor.a);
}
