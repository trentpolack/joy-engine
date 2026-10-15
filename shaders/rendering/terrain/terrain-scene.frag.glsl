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

in vec3 diagnostic;
layout(std140) uniform scene { mat4 matrix; vec4 eye; vec4 geometry; vec4 settings; vec4 target; };
in vec3 worldPosition;
out vec4 fragmentColor;

/**
 * Lights a vertex-colored triangle with atmosphere and bounded point lights.
 * A derivative normal avoids expanding the portable vertex format, while range falloff keeps each point light local.
 */
void main() {
  vec3 surface = cross(dFdx(worldPosition), dFdy(worldPosition));
  vec3 normal = surface/max(length(surface), 0.000001);
  float slope = 1.0 - abs(normal.y);
  vec3 color = mix(vec3(0.21, 0.36, 0.26), vec3(0.49, 0.43, 0.36), smoothstep(0.1, 0.65, slope));
  color = mix(color, vec3(0.79, 0.82, 0.77), smoothstep(390.0, 520.0, worldPosition.y));
  if(settings.w > 0.5) {
    color = 0.5 + 0.4*cos(vec3(0.0, 2.0, 4.0) + diagnostic.z*1.2);
  }
  if(settings.w > 1.5) {
    vec2 lines = abs(fract(diagnostic.xy - 0.5) - 0.5)/max(fwidth(diagnostic.xy), vec2(0.0001));
    color*= mix(0.3, 1.0, smoothstep(0.0, 1.0, min(lines.x, lines.y)));
  }
  vec3 irradiance = vec3(lighting.y) + skylightColorIntensity.rgb*skylightColorIntensity.a;
  float atmosphereDiffuse = abs(dot(normal, atmosphereDirectionIntensity.xyz));
  irradiance+= atmosphereColor.rgb*atmosphereDirectionIntensity.a*atmosphereDiffuse;

  // Point lights are bounded by their configured range, and the diffuse contribution is modulated by the angle between the light direction and the surface normal.
  for(int index = 0; index < 32; index++) {
    if(index >= int(lighting.x)) {
      break;
    }

    vec3 offset = lights[index].positionRange.xyz - worldPosition;
    float distance = length(offset);
    vec3 direction = offset/max(distance, 0.000001);
    float reach = max(0.0, 1.0 - (distance/lights[index].positionRange.w));
    float diffuse = abs(dot(normal, direction));

    irradiance+= lights[index].color.rgb*pow(reach, 2.0)*diffuse;
  }

  fragmentColor = vec4(color*irradiance, 1.0);
}
