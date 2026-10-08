// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PARTICLE_PARAMETER_TYPE } from './constants.ts';
import type { CompiledParticleEffect, CompiledEmitter, ParticleParameter } from './asset.ts';
import type { Instruction } from './script.ts';
import { createSeededRandom } from '../core/random.ts';
import { createParticleProfile, resetParticleProfile, refreshParticleProfileLive } from './profile.ts';
import { PARTICLE_FIELDS, runParticleScript } from './script.ts';

export type { CompiledParticleEffect };
export type { CompiledEmitter };
export type EffectParticle = Record<string, number> & {
    x: number;
    y: number;
    z: number;
    vx: number;
    vy: number;
    vz: number;
    ax: number;
    ay: number;
    az: number;
    drag: number;
    size: number;
    rotation: number;
    spin: number;
    r: number;
    g: number;
    b: number;
    alpha: number;
    lightIntensity: number;
    lightRange: number;
    lifetime: number;
    age: number;
    u0: number;
    u1: number;
    u2: number;
    u3: number;
    emitterIndex: number;
    generation: number;
    index: number;
    originX: number;
    originY: number;
    originZ: number;
    parentVx: number;
    parentVy: number;
    parentVz: number;
    nextTrail: number;
};
export interface ParticleEffectOptions {
  seed?: number;
  random?: () => number;
  parameters?: Record<string, number | [
      number,
      number,
      number
  ] | [
      number,
      number,
      number,
      number
  ]>;
  position?: {
      x: number;
      y: number;
      z?: number;
  };
  profiling?: boolean;
  profileClock?: () => number;
}
export interface ChildRequest {
  emitterIndex: number;
  count: number;
  parent: EffectParticle;
}

export const PARTICLE_STEP_SECONDS = 1 / 60;
const MAXIMUM_GENERATION = 4;
const MAXIMUM_TOTAL_SPAWNS = 65536;
const MAXIMUM_STEP_OPERATIONS = 2000000;

/** Owns one effect's simulation, seed, parameters and particles; no host resources.
 * Definition is borrowed and immutable. Particles are borrowed read-only views.
 * World coordinates are XYZ; velocities/accelerations use seconds and drag is
 * an exponential damping coefficient per second. Emission is cosmetic only.
 * Optional random is borrowed: reset/seek continue that stream rather than
 * rewinding it. Omit random for seed-reproducible editor reset and seek.
 */
export class ParticleEffect {
  declare definition: CompiledParticleEffect;
  declare profile: ReturnType<typeof createParticleProfile>;
  private declare profileClock: () => number;
  declare seed: number;
  private declare randomSource: (() => number) | undefined;
  declare position: Readonly<{ x: number; y: number; z: number; }>;
  declare parameters: Readonly<Record<string, number>>;
  declare emitterIndices: Map<string, number>;
  private declare liveParticles: EffectParticle[];
  declare random: () => number;
  declare ticks: number;
  declare accumulator: number;
  declare emitted: number;
  declare droppedParticles: number;
  declare stopped: boolean;
  declare disposed: boolean;
  declare error: Error | null;
  declare rootStates: { nextBurst: number; accumulator: number; }[];
  declare budget: { remaining: number; };

  /** @param definition @param [options] */
  constructor(definition: CompiledParticleEffect, options: ParticleEffectOptions = {}) {
    this.definition = definition;
    this.profile = createParticleProfile(definition, options.profiling ?? false);
    /** @private Millisecond monotonic clock, called only when profiling is enabled. */
    this.profileClock = options.profileClock ?? (() => performance.now());
    const seed = options.seed ?? 0;
    if(!Number.isInteger(seed) || !Number.isFinite(seed)) {
      throw new RangeError('Particle seed must be a finite integer.');
    }
    this.seed = seed >>> 0;
    /** @private Borrowed game stream, if supplied. Reset continues this stream. */
    this.randomSource = options.random;
    const position = options.position ?? { x: 0, y: 0, z: 0 };
    this.position = Object.freeze({
      x: position.x,
      y: position.y,
      z: position.z ?? 0,
    });
    for(const value of Object.values(this.position)) {
      if(!Number.isFinite(value) || Math.abs(value) > 1e6) {
        throw new RangeError('Particle origin must be finite and within ±1 million world units.');
      }
    }

    const parameters: Record<string, number> = Object.create(null);
    for(const [name, parameter] of Object.entries(definition.asset.parameters)) {
      assignParameterComponents(parameters, name, parameter.value, parameter.type);
    }
    for(const [name, value] of Object.entries(options.parameters ?? {})) {
      const parameter = definition.asset.parameters[name];
      if(!parameter || !parameterValueIsValid(parameter, value)) {
        throw new RangeError(`Invalid value for effect parameter '${name}'.`);
      }
      assignParameterComponents(parameters, name, value, parameter.type);
    }
    this.parameters = Object.freeze(parameters);
    this.emitterIndices = new Map(definition.emitters.map((emitter, index) => [emitter.id, index]));
    /** @private */
    this.liveParticles = [];
    this.random = this.randomSource ?? createSeededRandom(this.seed);
    this.ticks = 0;
    this.accumulator = 0;
    this.emitted = 0;
    this.droppedParticles = 0;
    this.stopped = false;
    this.disposed = false;

    this.error = null;
    this.rootStates = definition.emitters.map(() => ({
      nextBurst: 0,
      accumulator: 0,
    }));
    this.budget = { remaining: MAXIMUM_STEP_OPERATIONS };
    this.reset();
  }

  get particles() {
    return this.liveParticles;
  }
  /** Elapsed fixed-step simulation time in seconds; excludes the fractional accumulator. */
  get time() {
    return this.ticks * PARTICLE_STEP_SECONDS;
  }
  get isAlive() {
    if(this.disposed) {
      return false;
    }
    if(this.liveParticles.length) {
      return true;
    }
    return (
      !this.stopped &&
      this.emitted < MAXIMUM_TOTAL_SPAWNS &&
      this.definition.emitters.some(
        (emitter, index) =>
          emitter.enabled &&
          emitter.root &&
          (this.rootStates[index].nextBurst < emitter.bursts.length ||
            (emitter.rate > 0 && this.time < emitter.duration)),
      )
    );
  }
  /** Accumulate real time without making frame cadence change the simulation.
   * @param deltaSeconds Finite delta in [0, 0.25]. Owners clamp long pauses.
   */
  update(deltaSeconds: number) {
    this.assertActive();
    if(!Number.isFinite(deltaSeconds) || deltaSeconds < 0 || deltaSeconds > 0.25) {
      throw new RangeError('Particle delta must be between 0 and 0.25 seconds.');
    }
    this.accumulator += deltaSeconds;
    while(this.accumulator + 1e-10 >= PARTICLE_STEP_SECONDS) {
      this.step();
      this.accumulator = Math.max(0, this.accumulator - PARTICLE_STEP_SECONDS);
    }
  }
  /** Advance one fixed tick. Survivors update before newly emitted children. */
  step() {
    this.assertActive();
    if(this.error) {
      throw this.error;
    }
    const started = this.profile.enabled ? this.profileClock() : 0;
    this.budget.remaining = MAXIMUM_STEP_OPERATIONS;
    const previousTime = this.time;
    this.ticks++;

    const survivors: EffectParticle[] = [];

    const children: ChildRequest[] = [];
    try {
      for(const particle of this.liveParticles) {
        const emitter = this.definition.emitters[particle.emitterIndex];
        const row = this.profile.enabled ? this.profile.emitters[particle.emitterIndex] : null;
        const updateStarted = row ? this.profileClock() : 0;
        try {
          particle.age += PARTICLE_STEP_SECONDS;
          this.execute(emitter.updateProgram, particle, 'update');
          integrate(particle);
          const alive = particle.age + 1e-10 < particle.lifetime;
          if(!this.stopped && particle.generation < MAXIMUM_GENERATION) {
            if(alive && emitter.trail && particle.age + 1e-10 >= particle.nextTrail) {
              particle.nextTrail += emitter.trail.interval;
              children.push({
                emitterIndex:  ((this.emitterIndices.get(emitter.trail.emitter)) as number),
                count: emitter.trail.count,
                parent: particle,
              });
            }
            if(!alive && emitter.death) {
              children.push({
                emitterIndex:  ((this.emitterIndices.get(emitter.death.emitter)) as number),
                count: emitter.death.count,
                parent: particle,
              });
            }
          }
          if(alive) {
            survivors.push(particle);
          }
        } finally {
          if(row) {
            row.updateMs += Math.max(0, this.profileClock() - updateStarted);
          }
        }
      }
      this.liveParticles = survivors;
      if(!this.stopped) {
        for(const request of children) {
          this.spawn(request.emitterIndex, request.count, request.parent);
        }
        this.emitRoots(previousTime, this.time);
      }
    } catch (error) {
      this.fail(error);
    } finally {
      if(this.profile.enabled) {
        this.profile.steps++;
        refreshParticleProfileLive(this.profile, this.liveParticles);
        this.profile.simulationMs += Math.max(0, this.profileClock() - started);
      }
    }
  }
  /** Reset time and particles at the current origin; emits time-zero bursts.
   * Owned seeds rewind; borrowed random streams continue from their current state.
   */
  reset() {
    this.assertActive();
    resetParticleProfile(this.profile);
    const started = this.profile.enabled ? this.profileClock() : 0;
    this.liveParticles = [];
    this.random = this.randomSource ?? createSeededRandom(this.seed);
    this.ticks = 0;
    this.accumulator = 0;
    this.emitted = 0;
    this.droppedParticles = 0;
    this.stopped = false;
    this.error = null;
    this.rootStates = this.definition.emitters.map(() => ({
      nextBurst: 0,
      accumulator: 0,
    }));
    this.budget.remaining = MAXIMUM_STEP_OPERATIONS;
    try {
      this.emitRoots(0, 0);
    } catch (error) {
      this.fail(error);
    } finally {
      if(this.profile.enabled) {
        refreshParticleProfileLive(this.profile, this.liveParticles);
        this.profile.simulationMs += Math.max(0, this.profileClock() - started);
      }
    }
  }
  /** Replay to a fixed tick, bounded to 30 seconds; deterministic with an owned seed.
   * Editors should schedule long seeks in chunks if responsiveness is required.
   * @param seconds
   */
  seek(seconds: number) {
    if(!Number.isFinite(seconds) || seconds < 0 || seconds > 30) {
      throw new RangeError('Seek must be between 0 and 30 seconds.');
    }
    this.reset();
    const ticks = Math.floor(seconds / PARTICLE_STEP_SECONDS + 1e-8);
    for(let index = 0; index < ticks; index++) {
      this.step();
    }
  }
  /**
   * Evict oldest live particles for a session-wide cosmetic budget. Does not
   * trigger death emitters or rewind the seed, clock, or total spawn count.
   * @param count Non-negative integer; excess count is harmless.
   */
  removeOldestParticles(count: number) {
    this.assertActive();
    if(!Number.isInteger(count) || count < 0) {
      throw new RangeError('Particle removal count must be a non-negative integer.');
    }
    this.liveParticles.splice(0, count);
    refreshParticleProfileLive(this.profile, this.liveParticles);
  }

  /**
   * Rescale world positions about zero after a world-bounds resize. Velocity,
   * size, time and force units stay unchanged. Future spawn origins follow the
   * same transform; reset retains the resized origin. Validation is atomic.
   * @param scale Positive finite axis factors.
   */
  scalePositions(scale: {
    x: number;
    y: number;
    z: number;
}) {
    this.assertActive();
    const axes = ((['x', 'y', 'z']) as const);
    for(const axis of axes) {
      const factor = scale[axis];
      const positions = [this.position[axis], ...this.liveParticles.flatMap(particle => [particle[axis], particle[`origin${axis.toUpperCase()}`]])];
      if(!Number.isFinite(factor) || factor <= 0 || positions.some(value => Math.abs(value * factor) > 1e6)) {
        throw new RangeError('Scaled particle positions must be finite and within ±1 million.');
      }
    }
    this.position = Object.freeze({ x: this.position.x * scale.x, y: this.position.y * scale.y, z: this.position.z * scale.z });
    for(const particle of this.liveParticles) {
      for(const axis of axes) {
        particle[axis] *= scale[axis];
        particle[`origin${axis.toUpperCase()}`] *= scale[axis];
      }
    }
  }

  /** Stop root and child emission; live particles finish normally. */
  stop() {
    this.stopped = true;
  }
  /** Idempotent terminal cleanup. The borrowed definition remains reusable. */
  destroy() {
    this.liveParticles = [];
    this.stopped = true;
    this.disposed = true;
    refreshParticleProfileLive(this.profile, this.liveParticles);
  }
  /** @private */
  private assertActive() {
    if(this.disposed) {
      throw new Error('Particle effect has been destroyed.');
    }
  }
  /** Emit roots over the elapsed interval, including unconsumed bursts at its end.
   * @private @param start Start time in seconds. @param end End time in seconds.
   */
  private emitRoots(start: number, end: number) {
    for(let index = 0; index < this.definition.emitters.length; index++) {
      const emitter = this.definition.emitters[index];
      if(!emitter.enabled || !emitter.root) {
        continue;
      }
      const state = this.rootStates[index];
      const activeSeconds = Math.max(
        0,
        Math.min(end, emitter.duration) - Math.min(start, emitter.duration),
      );
      state.accumulator += emitter.rate * activeSeconds;
      let count = Math.floor(state.accumulator + 1e-10);
      state.accumulator = Math.max(0, state.accumulator - count);
      while(
        state.nextBurst < emitter.bursts.length &&
        emitter.bursts[state.nextBurst].time <= end + 1e-10
      ) {
        count += emitter.bursts[state.nextBurst].count;
        state.nextBurst++;
      }
      this.spawn(index, count);
    }
  }
  /** @private @param emitterIndex @param count @param [parent] */
  private spawn(emitterIndex: number, count: number, parent?: EffectParticle) {
    const emitter = this.definition.emitters[emitterIndex];
    if(!emitter.enabled || count === 0) {
      return;
    }
    const row = this.profile.enabled ? this.profile.emitters[emitterIndex] : null;
    const started = row ? this.profileClock() : 0;
    try {
      const available = Math.max(
        0,
        Math.min(
          this.definition.asset.maxParticles - this.liveParticles.length,
          MAXIMUM_TOTAL_SPAWNS - this.emitted,
        ),
      );
      const accepted = Math.min(count, available);
      this.droppedParticles += count - accepted;
      if(row) {
        row.dropped += count - accepted;
      }
      for(let index = 0; index < accepted; index++) {
        const origin = parent ?? this.position;

        const particle: EffectParticle = {
          x: origin.x,
          y: origin.y,
          z: origin.z,
          vx: 0,
          vy: 0,
          vz: 0,
          ax: 0,
          ay: 0,
          az: 0,
          drag: 0,
          size: 4,
          rotation: 0,
          spin: 0,
          r: 2,
          g: 0.5,
          b: 0.1,
          alpha: 1,
          lightIntensity: emitter.light?.intensity ?? 0,
          lightRange: emitter.light?.range ?? 0,
          lifetime: emitter.lifetime,
          age: 0,
          u0: 0,
          u1: 0,
          u2: 0,
          u3: 0,
          emitterIndex,
          generation: parent ? parent.generation + 1 : 0,
          index: this.emitted,
          originX: origin.x,
          originY: origin.y,
          originZ: origin.z,
          parentVx: parent?.vx ?? 0,
          parentVy: parent?.vy ?? 0,
          parentVz: parent?.vz ?? 0,
          nextTrail: emitter.trail?.interval ?? Infinity,
        };
        this.emitted++;
        if(row) {
          row.spawned++;
        }
        this.execute(emitter.spawnProgram, particle, 'spawn');
        this.liveParticles.push(particle);
      }
    } finally {
      if(row) {
        row.spawnMs += Math.max(0, this.profileClock() - started);
      }
    }
  }
  /** @private @param program @param particle @param phase */
  private execute(program: readonly Instruction[], particle: EffectParticle, phase: string) {

    const values: Record<string, number> = Object.assign(Object.create(null), this.parameters, {
      dt: PARTICLE_STEP_SECONDS,
      age: particle.age,
      t: Math.min(1, particle.age / Math.max(1e-10, particle.lifetime)),
      index: particle.index,
      time: this.time,
      pi: Math.PI,
      tau: Math.PI * 2,
      originX: particle.originX,
      originY: particle.originY,
      originZ: particle.originZ,
      parentVx: particle.parentVx,
      parentVy: particle.parentVy,
      parentVz: particle.parentVz,
    });
    // Private bookkeeping is not script input and cannot shadow parameters.
    for(const field of PARTICLE_FIELDS) {
      values[field] = particle[field];
    }
    const remaining = this.budget.remaining;
    try {
      runParticleScript(program, {
        values,
        random: this.random,
        budget: this.budget,
      });
      for(const field of PARTICLE_FIELDS) {
        particle[field] = values[field];
      }
      validateParticle(particle);
    } catch (error) {
      if(error instanceof Error) {
        error.message = `Emitter '${this.definition.emitters[particle.emitterIndex].id}' ${phase}: ${error.message}`;
      }
      throw error;
    } finally {
      if(this.profile.enabled) {
        this.profile.emitters[particle.emitterIndex].operations += remaining - this.budget.remaining;
      }
    }
  }
  /** @private @param error */
  private fail(error: unknown): never {
    this.error = error instanceof Error ? error : new Error(String(error));
    this.stopped = true;
    throw this.error;
  }
}

/** @param target @param name @param value @param type */
function assignParameterComponents(target: Record<string, number>, name: string, value: number | readonly number[], type: ParticleParameter['type']) {
  if(Array.isArray(value)) {
    const suffixes = type === 'color' ? ['.r', '.g', '.b', '.a'] : ['X', 'Y', 'Z'];
    for(let index = 0; index < suffixes.length; index++) {
      target[name + suffixes[index]] = value[index];
    }
    return;
  }
  target[name] = ((value) as number);
}

/** @param parameter @param value */
function parameterValueIsValid(parameter: ParticleParameter, value: unknown) {
  if(parameter.type === PARTICLE_PARAMETER_TYPE.VECTOR3 || parameter.type === PARTICLE_PARAMETER_TYPE.COLOR) {
    return Array.isArray(value) && value.length === parameter.value.length && parameter.value.every((component, index) =>
      typeof value[index] === 'number' && Number.isFinite(value[index]) && value[index] >= parameter.min[index] && value[index] <= parameter.max[index]);
  }
  return typeof value === 'number' && Number.isFinite(value) && value >= parameter.min && value <= parameter.max;
}

/** Integrate velocity before position, then damp velocity for the next fixed tick.
 * @param particle Mutated in world units and seconds.
 */
function integrate(particle: EffectParticle) {
  const dt = PARTICLE_STEP_SECONDS;
  particle.vx += particle.ax * dt;
  particle.vy += particle.ay * dt;
  particle.vz += particle.az * dt;
  particle.x += particle.vx * dt;
  particle.y += particle.vy * dt;
  particle.z += particle.vz * dt;
  const damping = Math.exp(-particle.drag * dt);
  particle.vx *= damping;
  particle.vy *= damping;
  particle.vz *= damping;
  particle.rotation += particle.spin * dt;
  validateParticle(particle);
}
/** @param particle */
function validateParticle(particle: EffectParticle) {
  for(const field of PARTICLE_FIELDS) {
    if(!Number.isFinite(particle[field]) || Math.abs(particle[field]) > 1e6) {
      throw new RangeError(`Particle ${field} must be finite and within ±1 million.`);
    }
  }
  if(
    particle.lightIntensity < 0 ||
    particle.lightIntensity > 1000 ||
    particle.lightRange < 0 ||
    particle.lightRange > 10000 ||
    particle.size < 0 ||
    particle.size > 10000 ||
    particle.drag < 0 ||
    particle.drag > 1000 ||
    particle.lifetime < 0 ||
    particle.lifetime > 30
  ) {
    throw new RangeError('Particle light intensity, light range, size, drag or lifetime exceeded its supported range.');
  }
}
