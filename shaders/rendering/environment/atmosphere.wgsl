// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

struct Uniforms {
  inverseViewProjection: mat4x4f,
  cameraPosition: vec4f,
  viewport: vec4f,
  environment: array<vec4f, 16>,
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;

struct VertexOutput {
  @builtin(position) position: vec4f,
  @location(0) screenPosition: vec2f,
};

/**
 * Builds a full-screen triangle without allocating a vertex buffer.
 * The oversized triangle covers the viewport while avoiding a diagonal seam between two screen-space primitives.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32) -> VertexOutput {
  let position = vec2f(f32((vertexIndex << 1u) & 2u), f32(vertexIndex & 2u));
  var output: VertexOutput;
  output.screenPosition = position*2.0 - 1.0;
  output.position = vec4f(output.screenPosition, 0.0, 1.0);
  return output;
}

/**
 * Produces a repeatable scalar noise value for a two-dimensional lattice coordinate.
 * This inexpensive hash is suitable for atmosphere detail, but not for gameplay randomness or persistent simulation state.
 *  TODO (trent, 10/12/26): Consider replacing this with a more robust noise function that produces a wider range of frequencies and amplitudes.
 */
fn hashNoise(value: vec2f) -> f32 {
  return(fract(sin(dot(value, vec2f(127.1, 311.7)))*43758.5453));
}

/**
 * Accumulates four value-noise octaves into the broad shapes used by the cloud layer.
 * The non-integer octave offset prevents obvious alignment while the halved weights keep the result bounded.
 */
fn cloudNoise(initialPoint: vec2f) -> f32 {
  var point = initialPoint;
  var result = 0.0;
  var weight = 0.5;

  for(var octave = 0; octave < 4; octave++) {
    let cell = floor(point);

    var local = fract(point);
    local = local*local*(3.0 - 2.0*local);

    let low = mix(hashNoise(cell), hashNoise(cell + vec2f(1.0, 0.0)), local.x);
    let high = mix(hashNoise(cell + vec2f(0.0, 1.0)), hashNoise(cell + vec2f(1.0)), local.x);
    result+= mix(low, high, local.y)*weight;
    point = point*2.03 + 17.17;

    weight*= 0.5;
  }

  return result;
}

/**
 * Reconstructs a world-space view ray and shades the procedural atmosphere.
 *
 * Sky scattering, celestial discs, advected clouds, and shafts share packed environment values with the CPU environment system.
 */
@fragment fn fragmentMain(input: VertexOutput) -> @location(0) vec4f {
  let sunDirectionIntensity = uniforms.environment[0];
  let sunColorRadius = uniforms.environment[1];
  let moonDirectionIntensity = uniforms.environment[2];
  let moonColorRadius = uniforms.environment[3];
  let skylightColorIntensity = uniforms.environment[4];
  let windVelocityTime = uniforms.environment[5];
  let atmosphere = uniforms.environment[6];
  let fogCloudCoverageDensityShadow = uniforms.environment[7];
  let cloudAltitudeThicknessShaftIntensityDecay = uniforms.environment[8];
  let shaftExposureAoIntensityRadiusTimeOfDay = uniforms.environment[9];
  let world = uniforms.inverseViewProjection*vec4f(input.screenPosition, 1.0, 1.0);
  let viewDirection = normalize((world.xyz/world.w) - uniforms.cameraPosition.xyz);
  let sunAmount = max(dot(viewDirection, sunDirectionIntensity.xyz), 0.0);
  let moonAmount = max(dot(viewDirection, moonDirectionIntensity.xyz), 0.0);
  let horizon = pow(clamp(1.0 - max(viewDirection.y, 0.0), 0.0, 1.0), 3.0);
  let rayleighPhase = 0.75*(1.0 + sunAmount*sunAmount);
  let miePhase = pow(sunAmount, mix(8.0, 128.0, clamp(atmosphere.y, 0.0, 1.0)));

  // Base sky calculation is a linear combination of the three scattering components, each modulated by the atmosphere parameters and the sun/moon contributions.
  var sky = skylightColorIntensity.rgb*skylightColorIntensity.a;
  sky+= sunColorRadius.rgb*sunDirectionIntensity.a*(rayleighPhase*0.08*atmosphere.x + miePhase*atmosphere.y);
  sky+= vec3f(0.34, 0.18, 0.08)*horizon*atmosphere.z*0.12;

  // The sun and moon discs are smooth circular gradients that fade to zero at the configured radius.
  //  TODO (trent, 10/12/26): Potentially add a more physically accurate disc shape that includes the sun's corona and moon's halo (or a texture).
  let sunDisc = smoothstep(sunColorRadius.a*2.0, sunColorRadius.a, acos(clamp(sunAmount, -1.0, 1.0)));
  let moonDisc = smoothstep(moonColorRadius.a*2.0, moonColorRadius.a, acos(clamp(moonAmount, -1.0, 1.0)));
  sky+= sunColorRadius.rgb*sunDirectionIntensity.a*sunDisc;
  sky+= moonColorRadius.rgb*moonDirectionIntensity.a*moonDisc;

  // Clouds are a value-noise layer that is advected by the wind and modulated by the configured coverage and density.
  let windOffset = windVelocityTime.xz*windVelocityTime.w*0.00004;
  let cloudCoordinate = viewDirection.xz/max(viewDirection.y + 0.18, 0.08)*0.8 + windOffset;
  let cloudShape = cloudNoise(cloudCoordinate);

  // The cloud layer is an additive effect that is modulated by the sun's intensity and the configured altitude, thickness, and decay.
  var clouds = smoothstep(1.0 - fogCloudCoverageDensityShadow.y, 1.18 - fogCloudCoverageDensityShadow.y, cloudShape)*fogCloudCoverageDensityShadow.z;
  clouds*= smoothstep(-0.08, 0.12, viewDirection.y);
  let cloudLight = mix(skylightColorIntensity.rgb, sunColorRadius.rgb, 0.65)*sunDirectionIntensity.a*0.18;
  sky = mix(sky, cloudLight, clamp(clouds, 0.0, 0.94));

  // Sun shafts are a radial gradient that is modulated by the sun's intensity and the configured altitude, thickness, decay, and exposure.
  let shaft = pow(sunAmount, 24.0)*cloudAltitudeThicknessShaftIntensityDecay.z*shaftExposureAoIntensityRadiusTimeOfDay.x;
  sky+= sunColorRadius.rgb*shaft*(1.0 - clouds*fogCloudCoverageDensityShadow.w);

  return(vec4f(max(sky, vec3f(0.0)), 1.0));
}
