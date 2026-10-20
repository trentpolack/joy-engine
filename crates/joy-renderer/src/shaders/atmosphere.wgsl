// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Procedural sky for the environment frame: Rayleigh/Mie approximations, sun and moon discs, wind-advected clouds, and
// a forward-scattering halo. The packed uniform block mirrors `AtmosphereUniforms` (materials/mod.rs), and
// `EnvironmentLighting::pack` fills its `environment` slots.

struct Atmosphere {
  inverseViewProjection: mat4x4f,
  cameraPosition: vec4f,
  viewport: vec4f,
  environment: array<vec4f, 16>,
};

@group(0) @binding(0) var<uniform> atmosphere: Atmosphere;

struct AtmosphereOutput {
  @builtin(position) position: vec4f,
  @location(0) screenPosition: vec2f,
};

/**
 * Build a full-screen triangle without a vertex buffer. One oversized triangle avoids a diagonal seam. It sits on the far
 * plane: drawn after opaque geometry with a less-or-equal depth test, the sky shades only the pixels nothing covers.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> AtmosphereOutput {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));

  var output: AtmosphereOutput;
  output.screenPosition = (position*2.0) - 1.0;
  output.position = vec4f(output.screenPosition, 1.0, 1.0);
  return output;
}

/**
 * Repeatable scalar noise for a lattice coordinate. Suitable for sky detail only, never gameplay randomness.
 */
fn hashNoise(value: vec2f) -> f32 {
  return fract(sin(dot(value, vec2f(127.1, 311.7)))*43758.5453);
}

/**
 * Four value-noise octaves forming the broad cloud shapes; the octave offset avoids obvious alignment.
 */
fn cloudNoise(initialPoint: vec2f) -> f32 {
  var point = initialPoint;
  var result = 0.0;
  var weight = 0.5;

  for(var octave = 0; octave < 4; octave++) {
    let cell = floor(point);
    var local = fract(point);
    local = local*local*(3.0 - (2.0*local));

    let low = mix(hashNoise(cell), hashNoise(cell + vec2f(1.0, 0.0)), local.x);
    let high = mix(hashNoise(cell + vec2f(0.0, 1.0)), hashNoise(cell + vec2f(1.0)), local.x);
    result+= mix(low, high, local.y)*weight;
    point = (point*2.03) + 17.17;
    weight*= 0.5;
  }

  return result;
}

/**
 * The sky alone, for passes that write only the scene color.
 */
@fragment fn fragmentMain(input: AtmosphereOutput) -> @location(0) vec4f {
  return skyColor(input);
}

struct SkyTargets {
  @location(0) color: vec4f,
  @location(1) motion: vec4f,
};

/**
 * The sky in passes that also write the scene-effect attachment. The sky carries no surface, so it writes the
 * attachment's cleared value (zero coverage) and effects reproject it from the camera. Writing every target keeps one
 * write mask for all of them, which WebGL 2 requires (it cannot blend or mask targets independently).
 */
@fragment fn fragmentTargets(input: AtmosphereOutput) -> SkyTargets {
  return SkyTargets(skyColor(input), vec4f(0.0));
}

/**
 * Reconstruct a world-space view ray and shade the sky in linear HDR.
 */
fn skyColor(input: AtmosphereOutput) -> vec4f {
  let sunDirectionIntensity = atmosphere.environment[0];
  let sunColorRadius = atmosphere.environment[1];
  let moonDirectionIntensity = atmosphere.environment[2];
  let moonColorRadius = atmosphere.environment[3];
  let skylightColorIntensity = atmosphere.environment[4];
  let windVelocityTime = atmosphere.environment[5];
  let scattering = atmosphere.environment[6];
  let fogCloudCoverageDensityShadow = atmosphere.environment[7];
  let cloudAltitudeThicknessShaftIntensityDecay = atmosphere.environment[8];
  let shaftExposureAoIntensityRadiusTimeOfDay = atmosphere.environment[9];

  let world = atmosphere.inverseViewProjection*vec4f(input.screenPosition, 1.0, 1.0);
  let viewDirection = normalize((world.xyz/world.w) - atmosphere.cameraPosition.xyz);
  let sunAmount = max(dot(viewDirection, sunDirectionIntensity.xyz), 0.0);
  let moonAmount = max(dot(viewDirection, moonDirectionIntensity.xyz), 0.0);
  let horizon = pow(clamp(1.0 - max(viewDirection.y, 0.0), 0.0, 1.0), 3.0);
  let rayleighPhase = 0.75*(1.0 + (sunAmount*sunAmount));
  let miePhase = pow(sunAmount, mix(8.0, 128.0, clamp(scattering.y, 0.0, 1.0)));

  // Base sky: skylight plus Rayleigh and Mie scattering of the sun, and a warm horizon band.
  var sky = skylightColorIntensity.rgb*skylightColorIntensity.a;
  sky+= sunColorRadius.rgb*sunDirectionIntensity.a*((rayleighPhase*0.08*scattering.x) + (miePhase*scattering.y));
  sky+= vec3f(0.34, 0.18, 0.08)*horizon*scattering.z*0.12;

  // Sun and moon discs fade to zero at twice their angular radius.
  let sunDisc = smoothstep(sunColorRadius.a*2.0, sunColorRadius.a, acos(clamp(sunAmount, -1.0, 1.0)));
  let moonDisc = smoothstep(moonColorRadius.a*2.0, moonColorRadius.a, acos(clamp(moonAmount, -1.0, 1.0)));
  sky+= sunColorRadius.rgb*sunDirectionIntensity.a*sunDisc;
  sky+= moonColorRadius.rgb*moonDirectionIntensity.a*moonDisc;

  // Wind-advected value-noise cloud layer.
  let windOffset = windVelocityTime.xz*windVelocityTime.w*0.00004;
  let cloudCoordinate = ((viewDirection.xz/max(viewDirection.y + 0.18, 0.08))*0.8) + windOffset;
  let cloudShape = cloudNoise(cloudCoordinate);
  var clouds = smoothstep(1.0 - fogCloudCoverageDensityShadow.y, 1.18 - fogCloudCoverageDensityShadow.y, cloudShape)*fogCloudCoverageDensityShadow.z;
  clouds*= smoothstep(-0.08, 0.12, viewDirection.y);
  let cloudLight = mix(skylightColorIntensity.rgb, sunColorRadius.rgb, 0.65)*sunDirectionIntensity.a*0.18;
  sky = mix(sky, cloudLight, clamp(clouds, 0.0, 0.94));

  // Forward-scattering halo around the sun, dimmed by cloud cover.
  let shaft = pow(sunAmount, 24.0)*cloudAltitudeThicknessShaftIntensityDecay.z*shaftExposureAoIntensityRadiusTimeOfDay.x;
  sky+= sunColorRadius.rgb*shaft*(1.0 - (clouds*fogCloudCoverageDensityShadow.w));

  return vec4f(max(sky, vec3f(0.0)), 1.0);
}
