// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

precision highp float;

layout(std140) uniform uniforms {
  mat4 inverseViewProjection;
  vec4 cameraPosition;
  vec4 viewport;
  vec4 sunDirectionIntensity;
  vec4 sunColorRadius;
  vec4 moonDirectionIntensity;
  vec4 moonColorRadius;
  vec4 skylightColorIntensity;
  vec4 windVelocityTime;
  vec4 atmosphere;
  vec4 fogCloudCoverageDensityShadow;
  vec4 cloudAltitudeThicknessShaftIntensityDecay;
  vec4 shaftExposureAoIntensityRadiusTimeOfDay;
  vec4 environmentReserved[6];
};

in vec2 screenPosition;
out vec4 fragmentColor;

/**
 * Produces a repeatable scalar noise value for a two-dimensional lattice coordinate.
 * This inexpensive hash is suitable for atmosphere detail, but not for gameplay randomness or persistent simulation state.
 *  TODO (trent, 10/12/26): Consider replacing this with a more robust noise function that produces a wider range of frequencies and amplitudes.
 */
float hashNoise(vec2 value) {
  return fract(sin(dot(value, vec2(127.1, 311.7)))*43758.5453);
}

/**
 * Accumulates four value-noise octaves into the broad shapes used by the cloud layer.
 * The non-integer octave offset prevents obvious alignment while the halved weights keep the result bounded.
 */
float cloudNoise(vec2 point) {
  float result = 0.0;
  float weight = 0.5;

  for(int octave = 0; octave < 4; octave++) {
    vec2 cell = floor(point);
    vec2 local = fract(point);
    local = local*local*(3.0 - 2.0*local);

    float low = mix(hashNoise(cell), hashNoise(cell + vec2(1.0, 0.0)), local.x);
    float high = mix(hashNoise(cell + vec2(0.0, 1.0)), hashNoise(cell + vec2(1.0)), local.x);
    result+= mix(low, high, local.y)*weight;
    point = point*2.03 + 17.17;

    weight*= 0.5;
  }

  return result;
}

/**
 * Reconstructs a world-space view ray and shades the procedural atmosphere.
 * Sky scattering, celestial discs, advected clouds, and shafts share packed environment values with the CPU environment system.
 */
void main() {
  vec4 world = inverseViewProjection*vec4(screenPosition, 1.0, 1.0);
  vec3 viewDirection = normalize((world.xyz/world.w) - cameraPosition.xyz);
  float sunAmount = max(dot(viewDirection, sunDirectionIntensity.xyz), 0.0);
  float moonAmount = max(dot(viewDirection, moonDirectionIntensity.xyz), 0.0);
  float horizon = pow(clamp(1.0 - max(viewDirection.y, 0.0), 0.0, 1.0), 3.0);
  float rayleighPhase = 0.75*(1.0 + sunAmount*sunAmount);
  float miePhase = pow(sunAmount, mix(8.0, 128.0, clamp(atmosphere.y, 0.0, 1.0)));

  // Base sky calculation is a linear combination of the three scattering components, each modulated by the atmosphere parameters and the sun/moon contributions.
  vec3 sky = skylightColorIntensity.rgb*skylightColorIntensity.a;
  sky+= sunColorRadius.rgb*sunDirectionIntensity.a*(rayleighPhase*0.08*atmosphere.x + miePhase*atmosphere.y);
  sky+= vec3(0.34, 0.18, 0.08)*horizon*atmosphere.z*0.12;

  // The sun and moon discs are smooth circular gradients that fade to zero at the configured radius.
  //  TODO (trent, 10/12/26): Potentially add a more physically accurate disc shape that includes the sun's corona and moon's halo (or a texture).
  float sunDisc = smoothstep(sunColorRadius.a*2.0, sunColorRadius.a, acos(clamp(sunAmount, -1.0, 1.0)));
  float moonDisc = smoothstep(moonColorRadius.a*2.0, moonColorRadius.a, acos(clamp(moonAmount, -1.0, 1.0)));
  sky+= sunColorRadius.rgb*sunDirectionIntensity.a*sunDisc;
  sky+= moonColorRadius.rgb*moonDirectionIntensity.a*moonDisc;

  // Clouds are a value-noise layer that is advected by the wind and modulated by the configured coverage and density.
  vec2 windOffset = windVelocityTime.xz*windVelocityTime.w*0.00004;
  vec2 cloudCoordinate = viewDirection.xz/max(viewDirection.y + 0.18, 0.08)*0.8 + windOffset;
  float cloudShape = cloudNoise(cloudCoordinate);
  float coverage = fogCloudCoverageDensityShadow.y;
  float clouds = smoothstep(1.0 - coverage, 1.0 - coverage + 0.18, cloudShape)*fogCloudCoverageDensityShadow.z;
  clouds*= smoothstep(-0.08, 0.12, viewDirection.y);

  // The cloud layer is an additive effect that is modulated by the sun's intensity and the configured altitude, thickness, and decay.
  vec3 cloudLight = mix(skylightColorIntensity.rgb, sunColorRadius.rgb, 0.65)*sunDirectionIntensity.a*0.18;
  sky = mix(sky, cloudLight, clamp(clouds, 0.0, 0.94));

  // Sun shafts are a radial gradient that is modulated by the sun's intensity and the configured altitude, thickness, decay, and exposure.
  float shaft = pow(sunAmount, 24.0)*cloudAltitudeThicknessShaftIntensityDecay.z*shaftExposureAoIntensityRadiusTimeOfDay.x;
  sky+= sunColorRadius.rgb*shaft*(1.0 - clouds*fogCloudCoverageDensityShadow.w);
  fragmentColor = vec4(max(sky, vec3(0.0)), 1.0);
}

