// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PARTICLE_RENDERER } from './constants.ts';
import type { ParticleEffect, EffectParticle } from './particle-effect.ts';
import type { CompiledParticleEffect, ParticleEmitterAsset, ParticleTexture } from './asset.ts';
import type { RgbaColor } from '../core/types.ts';
import { addGradientRadialPolygon, addRadialPolygon, addRing } from '../rendering/geometry/geometry.ts';

export interface ParticleCameraBasis {
  right: readonly number[];
  up: readonly number[];
  backward: readonly number[];
}

/** Explicit fixed-plane basis for orthographic XY games. */
export const PARTICLE_XY_BASIS = Object.freeze({
  right: Object.freeze([1, 0, 0]),
  up: Object.freeze([0, 1, 0]),
  backward: Object.freeze([0, 0, 1]),
});

const ROUND = Object.freeze(Array.from({ length: 16 }, () => 1));
const BILLOW = Object.freeze(
  Array.from({ length: 24 }, (_, index) => {
    const angle = (index / 24) * Math.PI * 2;
    return 1 + Math.sin(angle * 3) * 0.09 + Math.cos(angle * 5) * 0.05;
  }),
);

/** Append camera-facing particles in full XYZ world space without advancing simulation.
 * Coordinates and radius are world units. Colors are linear HDR straight alpha.
 * The caller owns the batch and sorts it for the active camera before submission.
 * @param vertices Mutable interleaved XYZ/RGBA triangle list.
 * @param effect Borrowed runtime.
 * @param camera Orthonormal world-space camera basis. Sizes are world units.
 */
export function appendParticleEffect(vertices: number[], effect: ParticleEffect, camera: ParticleCameraBasis) {
  if(effect.definition.emitters.some(emitter => emitter.renderer === PARTICLE_RENDERER.TEXTURED || emitter.renderer === PARTICLE_RENDERER.FLIPBOOK)) {
    throw new Error('Textured particles require createParticleEffectBatches and a texture-capable renderer.');
  }
  for(const particle of effect.particles) {
    if(isVisible(particle)) {
      appendColoredParticle(vertices, particle, effect.definition.emitters[particle.emitterIndex].renderer, camera);
    }
  }
}

/** Create caller-owned per-build CPU geometry metrics, with no GPU timing claims.
 * @param definition
 */
export function createParticleRenderMetrics(definition: CompiledParticleEffect) {
  return {
    emitters: definition.emitters.map(emitter => ({ id: emitter.id, triangles: 0, vertices: 0, geometryMs: 0 })),
    sortMs: 0,
  };
}
export type ParticleRenderMetrics = ReturnType<typeof createParticleRenderMetrics>;

/** Create owned transparent material runs, sorted globally by triangle depth.
 * Colored vertices have XYZ/RGBA stride 7; PNG runs add top-left-origin UV (stride 9).
 * Consecutive triangles sharing a source are merged without changing blend order.
 * Borrows effect/camera without mutation; no simulation or resource loading occurs.
 * @param effect
 * @param camera
 * @param [metrics] Optional owned record, reset and filled per call; emitter counts exclude additional geometry.
 * @param [additionalTriangles] Borrowed untextured XYZ/RGBA triangles, interleaved at camera depth without mutation.
 */
export function createParticleEffectBatches(effect: ParticleEffect, camera: ParticleCameraBasis, metrics?: ParticleRenderMetrics, additionalTriangles: readonly number[] | Float32Array = []): {
    vertices: number[];
    source?: string;
}[] {
  if(metrics) {
    metrics.sortMs = 0;
    for(const row of metrics.emitters) {
      row.triangles = 0;
      row.vertices = 0;
      row.geometryMs = 0;
    }
  }

  const triangles: {
    vertices: number[];
    source?: string;
    depth: number;
}[] = [];
  for(const particle of effect.particles) {
    if(!isVisible(particle)) {
      continue;
    }
    const row = metrics?.emitters[particle.emitterIndex];
    const started = row ? performance.now() : 0;
    const emitter = effect.definition.emitters[particle.emitterIndex];
    const textured = emitter.renderer === PARTICLE_RENDERER.TEXTURED || emitter.renderer === PARTICLE_RENDERER.FLIPBOOK;

    const vertices: number[] = [];
    let source;
    if(textured) {
      const texture = emitter.texture;
      if(!texture || !emitter.textureDimensions) {
        throw new Error('Textured particle emitter requires a texture.');
      }
      source = texture.source;
      appendTexturedParticle(vertices, particle, texture, emitter.renderer === PARTICLE_RENDERER.FLIPBOOK, camera, emitter.textureDimensions);
    } else {
      appendColoredParticle(vertices, particle, emitter.renderer, camera);
    }
    const stride = textured ? 9 : 7;
    appendDepthTriangles(triangles, vertices, stride, source, camera.backward);
    if(row) {
      row.vertices += vertices.length / stride;
      row.triangles += vertices.length / (stride * 3);
      row.geometryMs += Math.max(0, performance.now() - started);
    }
  }
  if(additionalTriangles.length%21 !== 0) {
    throw new RangeError('Additional particle geometry requires complete XYZ/RGBA triangles.');
  }
  appendDepthTriangles(triangles, additionalTriangles, 7, undefined, camera.backward, true);
  const sortStarted = metrics ? performance.now() : 0;
  triangles.sort((a, b) => a.depth - b.depth);

  const batches: {
    vertices: number[];
    source?: string;
}[] = [];
  for(const triangle of triangles) {
    let batch = batches[batches.length - 1];
    if(!batch || batch.source !== triangle.source) {
      batch = triangle.source ? { vertices: [], source: triangle.source } : { vertices: [] };
      batches.push(batch);
    }
    batch.vertices.push(...triangle.vertices);
  }
  if(metrics) {
    metrics.sortMs = Math.max(0, performance.now() - sortStarted);
  }
  return batches;
}

/** Keep reference geometry and textured/colored particles in one stable depth order. */
function appendDepthTriangles(out: {vertices: number[]; source?: string; depth: number}[], vertices: readonly number[] | Float32Array, stride: number, source: string | undefined, backward: readonly number[], validate: boolean = false) {
  for(let offset = 0; offset < vertices.length; offset+= stride*3) {
    let depth = 0;
    for(let vertex = 0; vertex < 3; vertex++) {
      for(let axis = 0; axis < 3; axis++) {
        depth+= vertices[offset + vertex*stride + axis]*backward[axis];
      }
    }
    const triangle = vertices instanceof Float32Array
      ? Array.from(vertices.subarray(offset, offset + stride*3))
      : vertices.slice(offset, offset + stride*3);
    if(validate && !triangle.every(Number.isFinite)) {
      throw new RangeError('Particle geometry must contain finite vertex values.');
    }
    out.push({vertices: triangle, source, depth});
  }
}

/** @param particle */
function isVisible(particle: EffectParticle) {
  return particle.alpha > 0 && particle.size > 0 && particle.age < particle.lifetime;
}

/** @param vertices @param particle
 * @param renderer @param camera
 */
function appendColoredParticle(vertices: number[], particle: EffectParticle, renderer: ParticleEmitterAsset['renderer'], camera: ParticleCameraBasis) {
  const { size, rotation } = particle;
  const x = 0,
    y = 0,
    z = 0;

  const local: number[] = [];

  const color: RgbaColor = [
    Math.max(0, particle.r),
    Math.max(0, particle.g),
    Math.max(0, particle.b),
    Math.min(1, particle.alpha),
  ];

  const edge: RgbaColor = [color[0], color[1], color[2], 0];
  if(renderer === PARTICLE_RENDERER.CIRCLE) {
    addRadialPolygon(local, x, y, size, color, ROUND, rotation, z);
  } else if(renderer === PARTICLE_RENDERER.SQUARE) {
    appendQuad(local, size, rotation, color);
  } else if(renderer === PARTICLE_RENDERER.RING) {
    addRing(local, x, y, size, Math.max(0.15, size * 0.045), color, 32, z);
  } else if(renderer === PARTICLE_RENDERER.STREAK) {
    appendStreak(vertices, particle, color, camera);
    addGradientRadialPolygon(local, x, y, size * 1.7, color, edge, ROUND, rotation, z);
  } else if(renderer === PARTICLE_RENDERER.BILLOW) {
    // Overlapping feathered lobes give volume without project-specific code.
    addGradientRadialPolygon(local, x, y, size, color, edge, BILLOW, rotation, z);

    const inner: RgbaColor = [color[0] * 1.5, color[1] * 1.5, color[2] * 1.5, color[3] * 0.65];
    addGradientRadialPolygon(
      local,
      x - size * 0.16,
      y - size * 0.18,
      size * 0.65,
      inner,
      edge,
      BILLOW,
      rotation + 0.7,
      z + 0.01,
    );
  } else {
    addGradientRadialPolygon(local, x, y, size, color, edge, ROUND, rotation, z);
  }
  for(let offset = 0; offset < local.length; offset += 7) {
    for(let axis = 0; axis < 3; axis++) {
      const origin = axis === 0 ? particle.x : axis === 1 ? particle.y : particle.z;
      vertices.push(
        origin +
          local[offset] * camera.right[axis] +
          local[offset + 1] * camera.up[axis] +
          local[offset + 2] * camera.backward[axis],
      );
    }
    vertices.push(local[offset + 3], local[offset + 4], local[offset + 5], local[offset + 6]);
  }
}

/** A world-space velocity ribbon, including motion toward/away from the camera.
 * @param out @param p
 * @param color @param camera
 */
function appendStreak(out: number[], p: EffectParticle, color: RgbaColor, camera: ParticleCameraBasis) {
  const velocity = [p.vx, p.vy, p.vz];
  const view = camera.backward;
  let side = [
    velocity[1] * view[2] - velocity[2] * view[1],
    velocity[2] * view[0] - velocity[0] * view[2],
    velocity[0] * view[1] - velocity[1] * view[0],
  ];
  const length = Math.hypot(...side);
  side = length > 1e-8 ? side.map((value) => value / length) : [...camera.right];
  const head = [p.x, p.y, p.z];
  // A fixed 35 ms velocity trail gives the ribbon a world-space length independent of frame rate.
  const tail = head.map((value, axis) => value - velocity[axis] * 0.035);
  for(const [end, sign] of [
    [head, -1],
    [tail, -1],
    [tail, 1],
    [head, -1],
    [tail, 1],
    [head, 1],
  ]) {
    const point = ((end) as number[]);
    const direction = ((sign) as number);
    out.push(
      point[0] + side[0] * p.size * direction * 0.5,
      point[1] + side[1] * p.size * direction * 0.5,
      point[2] + side[2] * p.size * direction * 0.5,
      ...color,
    );
  }
}

/** Append a rotated billboard, with size as its world-space half extent.
 * @param out @param size @param rotation
 * @param color
 */
function appendQuad(out: number[], size: number, rotation: number, color: RgbaColor) {
  const cosine = Math.cos(rotation);
  const sine = Math.sin(rotation);
  for(const [x, y] of [[-1, 1], [-1, -1], [1, -1], [-1, 1], [1, -1], [1, 1]]) {
    out.push((x * cosine - y * sine) * size, (x * sine + y * cosine) * size, 0, ...color);
  }
}

/** @param out @param particle
 * @param texture @param animated
 * @param camera @param dimensions
 */
function appendTexturedParticle(out: number[], particle: EffectParticle, texture: ParticleTexture, animated: boolean, camera: ParticleCameraBasis, dimensions: {
    width: number;
    height: number;
}) {

  const local: number[] = [];
  appendQuad(local, particle.size, particle.rotation, [Math.max(0, particle.r), Math.max(0, particle.g), Math.max(0, particle.b), Math.min(1, particle.alpha)]);
  let frame = 0;
  if(animated) {
    const elapsedFrames = texture.fps > 0 ? particle.age * texture.fps : particle.age / particle.lifetime * texture.frames;
    const index = Math.max(0, Math.floor(elapsedFrames));
    frame = texture.loop ? index % texture.frames : Math.min(texture.frames - 1, index);
  }
  const column = frame % texture.columns;
  const row = Math.floor(frame / texture.columns);
  // Inset atlas UVs by half a source texel so linear filtering cannot sample adjacent frames.
  const left = column / texture.columns + 0.5 / dimensions.width;
  const right = (column + 1) / texture.columns - 0.5 / dimensions.width;
  const top = row / texture.rows + 0.5 / dimensions.height;
  const bottom = (row + 1) / texture.rows - 0.5 / dimensions.height;
  const uvs = [[left, top], [left, bottom], [right, bottom], [left, top], [right, bottom], [right, top]];
  for(let offset = 0; offset < local.length; offset += 7) {
    for(let axis = 0; axis < 3; axis++) {
      const origin = axis === 0 ? particle.x : axis === 1 ? particle.y : particle.z;
      out.push(origin + local[offset] * camera.right[axis] + local[offset + 1] * camera.up[axis]);
    }
    out.push(local[offset + 3], local[offset + 4], local[offset + 5], local[offset + 6], ...uvs[offset / 7]);
  }
}
