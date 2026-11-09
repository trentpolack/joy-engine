// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Particle entry points for the scene renderer's transparency queue. Composed after scene-common.wgsl (group 0: the
// frame uniforms, whose view matrix gives the camera axes, the frame's lights, and the fog volume; group 1, the shadow
// receiver, is bound but unused), particle-shapes.wgsl, and particle-depth.wgsl or particle-depth-none.wgsl (the soft
// fade, with the scene depth in group 3 for soft pipelines). Group 2 is the sprite texture: the batch's image, or one
// white texel for untextured batches. Particles are depth-tested against opaque geometry without depth writes.

@group(2) @binding(0) var particleTexture: texture_2d<f32>;
@group(2) @binding(1) var particleTextureSampler: sampler;

/**
 * Expand one quad corner facing the frame's camera.
 */
@vertex fn vertexMain(@builtin(vertex_index) vertexIndex: u32, instance: ParticleInstance) -> ParticleVarying {
  // The rows of the view matrix's rotation are the camera's right, up, and backward axes in world space.
  let right = vec3f(frame.view[0][0], frame.view[1][0], frame.view[2][0]);
  let up = vec3f(frame.view[0][1], frame.view[1][1], frame.view[2][1]);
  let backward = vec3f(frame.view[0][2], frame.view[1][2], frame.view[2][2]);
  return expandParticle(vertexIndex, instance, right, up, backward, frame.viewProjection);
}

/**
 * Half-Lambert wrap: light falls off smoothly across the particle and never reaches black on its far side, so a flat
 * camera-facing card reads as a soft volume.
 */
fn halfLambert(cosine: f32) -> f32 {
  let wrapped = (cosine*0.5) + 0.5;
  return wrapped*wrapped;
}

/**
 * Irradiance on a lit particle: uniform ambient and the skylight, the frame's directional light, and the point lights
 * of its light cluster, each through the half-Lambert wrap on the camera-facing normal. Particles receive no shadows;
 * the model stays cheap enough for thousands of overlapping fragments. Unlit particles keep their authored color.
 */
fn particleIrradiance(input: ParticleVarying) -> vec3f {
  if((input.shape & PARTICLE_LIT) == 0u) {
    return vec3f(1.0);
  }
  let normal = normalize(input.normal);
  var irradiance = vec3f(frame.lighting.y) + skylightIrradiance(normal);
  if(frame.primaryDirection.w > 0.0) {
    let cosine = dot(normal, normalize(frame.primaryDirection.xyz));
    irradiance+= frame.primaryColor.rgb*frame.primaryDirection.w*halfLambert(cosine);
  }

  let cluster = lightCluster(input.worldPosition);
  for(var entry = cluster.x; entry < cluster.x + cluster.y; entry++) {
    let light = pointLight(clusterLightIndex(entry));
    let reach = pointLightReach(light, input.worldPosition);
    if(reach <= 0.0) {
      continue;
    }
    let direction = normalize(light.positionRange.xyz - input.worldPosition);
    irradiance+= light.color.rgb*reach*halfLambert(dot(normal, direction));
  }
  return irradiance;
}

/**
 * The fragment's premultiplied color, lit and faded against the scene. Sampling and derivatives happen here, before any
 * per-shape branch.
 */
fn sceneParticle(input: ParticleVarying) -> vec4f {
  let texel = textureSample(particleTexture, particleTextureSampler, input.uv);
  let pixel = max(fwidth(input.local.x), fwidth(input.local.y));
  let shaded = shadeParticle(input, texel, pixel);
  let lit = vec4f(shaded.rgb*particleIrradiance(input), shaded.a);
  return lit*particleSceneFade(input);
}

/**
 * Straight and premultiplied particles, composited source-over (premultiplied blending) and fogged like any surface.
 */
@fragment fn fragmentOver(input: ParticleVarying) -> @location(0) vec4f {
  let color = sceneParticle(input);
  let fog = volumetricFog(input.worldPosition);
  return vec4f((color.rgb*fog.a) + (fog.rgb*color.a), color.a);
}

/**
 * Additive particles: emitted light dims through fog but adds no in-scattering, since it hides nothing behind it.
 */
@fragment fn fragmentAdditive(input: ParticleVarying) -> @location(0) vec4f {
  let color = sceneParticle(input);
  let fog = volumetricFog(input.worldPosition);
  return vec4f(color.rgb*fog.a, 0.0);
}
