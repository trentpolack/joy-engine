// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Hard particles: no scene depth to fade against. Composed in place of particle-depth.wgsl for the pipelines that draw
// batches without softness, and for every batch on WebGL 2, whose shaders cannot read the depth buffer they test
// against (soft particles fall back to hard intersections there).

/**
 * Every fragment survives; depth testing alone hides what opaque geometry covers.
 */
fn particleSceneFade(input: ParticleVarying) -> f32 {
  return 1.0;
}
