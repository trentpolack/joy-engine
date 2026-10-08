// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PARTICLE_PARAMETER_TYPE, PARTICLE_RENDERER, PARTICLE_VALUE_KIND } from './constants.ts';
import type { Instruction, ValueKind } from './script.ts';
import { readParticleTextureSource } from './texture-source.ts';
import { compileParticleScript, PARTICLE_FIELDS, PARTICLE_INPUTS } from './script.ts';

export type ParticleParameter = {
    value: number;
    min: number;
    max: number;
    type?: typeof PARTICLE_PARAMETER_TYPE.SCALAR;
} | {
    type: typeof PARTICLE_PARAMETER_TYPE.VECTOR3;
    value: [
        number,
        number,
        number
    ];
    min: [
        number,
        number,
        number
    ];
    max: [
        number,
        number,
        number
    ];
} | {
    type: typeof PARTICLE_PARAMETER_TYPE.COLOR;
    value: [
        number,
        number,
        number,
        number
    ];
    min: [
        number,
        number,
        number,
        number
    ];
    max: [
        number,
        number,
        number,
        number
    ];
};
export interface ParticleChild {
  emitter: string;
  count: number;
}
export interface ParticleTexture {
  source: string;
  columns: number;
  rows: number;
  frames: number;
  fps: number;
  loop: boolean;
}
export interface ParticleLight {
  intensity: number;
  range: number;
}
export interface ParticleEmitterAsset {
  id: string;
  enabled: boolean;
  root: boolean;
  duration: number;
  rate: number;
  bursts: {
      time: number;
      count: number;
  }[];
  lifetime: number;
  renderer: typeof PARTICLE_RENDERER.SOFT | typeof PARTICLE_RENDERER.STREAK | typeof PARTICLE_RENDERER.RING | typeof PARTICLE_RENDERER.BILLOW | typeof PARTICLE_RENDERER.CIRCLE | typeof PARTICLE_RENDERER.SQUARE | typeof PARTICLE_RENDERER.TEXTURED | typeof PARTICLE_RENDERER.FLIPBOOK;
  texture?: ParticleTexture;
  light?: ParticleLight;
  spawn: string;
  update: string;
  trail?: ParticleChild & {
      interval: number;
  };
  death?: ParticleChild;
}
export interface ParticleEffectAsset {
  version: 1;
  name: string;
  maxParticles: number;
  parameters: Record<string, ParticleParameter>;
  emitters: ParticleEmitterAsset[];
}
export type CompiledEmitter = Readonly<ParticleEmitterAsset> & {
    textureDimensions?: Readonly<{
        width: number;
        height: number;
    }>;
    spawnProgram: readonly Instruction[];
    updateProgram: readonly Instruction[];
};
export interface CompiledParticleEffect {
  readonly asset: ParticleEffectAsset;
  readonly emitters: readonly CompiledEmitter[];
}

/** Validate and compile a JSON-compatible effect into an immutable shared definition.
 * Owns a normalized copy; callers can continue editing their source asset safely.
 * Effects use caller-defined world units, seconds, radians and linear HDR RGB.
 * @param input */
export function compileParticleEffect(input: unknown): CompiledParticleEffect {
  const source = record(input, 'effect');
  if(source.version !== 1) {
    throw new Error('Unsupported particle effect version; expected 1.');
  }
  const name = label(source.name, 'effect.name');
  const maxParticles = number(source.maxParticles, 'maxParticles', 1, 4096, true);
  const suppliedParameters = record(source.parameters, 'parameters');
  if(Object.keys(suppliedParameters).length > 32) {
    throw new Error('An effect supports at most 32 parameters.');
  }

  const parameters: Record<string, ParticleParameter> = Object.create(null);
  const reserved = new Set([
    ...PARTICLE_FIELDS,
    ...PARTICLE_INPUTS,
    'let',
    'if',
    'else',
    'constructor',
    'prototype',
    '__proto__',
  ]);
  for(const [key, value] of Object.entries(suppliedParameters)) {
    identifier(key, 'parameter');
    if(reserved.has(key)) {
      throw new Error(`Reserved parameter name '${key}'.`);
    }
    const parameter = record(value, `parameter ${key}`);
    parameters[key] = readParameter(parameter, key);
  }
  const inputs = parameterInputs(parameters);

  const containers: Record<string, ValueKind> = {};
  for(const [key, parameter] of Object.entries(parameters)) {
    if(parameter.type === PARTICLE_PARAMETER_TYPE.VECTOR3 || parameter.type === PARTICLE_PARAMETER_TYPE.COLOR) {
      containers[key] = parameter.type === PARTICLE_PARAMETER_TYPE.VECTOR3 ? PARTICLE_VALUE_KIND.VECTOR : PARTICLE_VALUE_KIND.COLOR;
    }
  }
  if(new Set(inputs).size !== inputs.length || inputs.some(input => reserved.has(input)) || Object.keys(containers).some(name => inputs.includes(name))) {
    throw new Error('Scalar and vector parameter component names must be unique and unreserved.');
  }
  if(
    !Array.isArray(source.emitters) ||
    source.emitters.length < 1 ||
    source.emitters.length > 32
  ) {
    throw new Error('An effect requires 1–32 emitters.');
  }
  const ids = new Set();
  const emitters = source.emitters.map((value, index) => {
    const emitter = readEmitter(value, index);
    if(ids.has(emitter.id)) {
      throw new Error(`Duplicate emitter '${emitter.id}'.`);
    }
    ids.add(emitter.id);
    return emitter;
  });
  const textureSources = new Set(emitters.flatMap(emitter => emitter.texture ? [emitter.texture.source] : []));
  const encodedTextureBytes = emitters.reduce((total, emitter) => total + (emitter.texture?.source.length ?? 0), 0);
  if(textureSources.size > 8 || encodedTextureBytes > 4 * 1024 * 1024) {
    throw new Error('An effect supports at most 8 distinct PNG sources and 4 MiB of embedded texture data.');
  }
  for(const emitter of emitters) {
    for(const child of [emitter.trail, emitter.death]) {
      if(child && !ids.has(child.emitter)) {
        throw new Error(`Emitter '${emitter.id}' references missing child '${child.emitter}'.`);
      }
    }
  }
  const compiled = emitters.map((emitter) => {
    try {
      return Object.freeze({
        ...emitter,
        ...(emitter.texture ? { textureDimensions: Object.freeze(readParticleTextureSource(emitter.texture.source)) } : {}),
        spawnProgram: compileParticleScript(emitter.spawn, inputs, containers),
        updateProgram: compileParticleScript(emitter.update, inputs, containers),
      });
    } catch (error) {
      if(error instanceof Error) {
        error.message = `Emitter '${emitter.id}': ${error.message}`;
      }
      throw error;
    }
  });

  const asset: ParticleEffectAsset = { version: 1, name, maxParticles, parameters, emitters };
  // JSON records are frozen recursively so shared compiled definitions cannot
  // change underneath instances. Runtime state is always separately owned.
  freezeRecord(asset);
  return Object.freeze({ asset, emitters: Object.freeze(compiled) });
}

/** @param parameter @param key */
function readParameter(parameter: Record<string, unknown>, key: string): ParticleParameter {
  if(parameter.type === PARTICLE_PARAMETER_TYPE.COLOR) {
    const min = colorComponents(parameter.min, `${key}.min`, [0, 0, 0, 0], [1, 1, 1, 1]);
    const max = colorComponents(parameter.max, `${key}.max`, min, [1, 1, 1, 1]);
    return Object.freeze({ type: PARTICLE_PARAMETER_TYPE.COLOR, min, max, value: colorComponents(parameter.value, `${key}.value`, min, max) });
  }
  if(parameter.type !== undefined && parameter.type !== PARTICLE_PARAMETER_TYPE.SCALAR && parameter.type !== PARTICLE_PARAMETER_TYPE.VECTOR3) {
    throw new Error(`${key}.type must be scalar, vector3, or color.`);
  }
  if(parameter.type !== PARTICLE_PARAMETER_TYPE.VECTOR3) {
    const min = number(parameter.min, `${key}.min`, -1e6, 1e6);
    const max = number(parameter.max, `${key}.max`, min, 1e6);
    return Object.freeze({ min, max, value: number(parameter.value, `${key}.value`, min, max) });
  }
  const min = vector(parameter.min, `${key}.min`, [-1e6, -1e6, -1e6], [1e6, 1e6, 1e6]);
  const max = vector(parameter.max, `${key}.max`, min, [1e6, 1e6, 1e6]);
  return Object.freeze({ type: PARTICLE_PARAMETER_TYPE.VECTOR3, min, max, value: vector(parameter.value, `${key}.value`, min, max) });
}

/** @param value @param path @param min @param max */
function vector(value: unknown, path: string, min: readonly number[], max: readonly number[]): [
    number,
    number,
    number
] {
  if(!Array.isArray(value) || value.length !== 3) {
    throw new Error(`${path} must be an XYZ array.`);
  }
  return [
    number(value[0], `${path}[0]`, min[0], max[0]),
    number(value[1], `${path}[1]`, min[1], max[1]),
    number(value[2], `${path}[2]`, min[2], max[2]),
  ];
}

/** @param value @param path @param min @param max */
function colorComponents(value: unknown, path: string, min: readonly number[], max: readonly number[]): [
    number,
    number,
    number,
    number
] {
  if(!Array.isArray(value) || value.length !== 4) {
    throw new Error(`${path} must be an RGBA array.`);
  }
  return [
    number(value[0], `${path}[0]`, min[0], max[0]),
    number(value[1], `${path}[1]`, min[1], max[1]),
    number(value[2], `${path}[2]`, min[2], max[2]),
    number(value[3], `${path}[3]`, min[3], max[3]),
  ];
}

/** Flatten inputs into VM storage. Existing version-1 XYZ spellings remain valid.
 * @param parameters
 */
function parameterInputs(parameters: Record<string, ParticleParameter>) {
  return Object.entries(parameters).flatMap(([name, parameter]) => {
    if(parameter.type === PARTICLE_PARAMETER_TYPE.COLOR) {
      return ['r', 'g', 'b', 'a'].map(channel => `${name}.${channel}`);
    }
    return parameter.type === PARTICLE_PARAMETER_TYPE.VECTOR3 ? [`${name}X`, `${name}Y`, `${name}Z`] : [name];
  });
}

/** @param value @param index */
function readEmitter(value: unknown, index: number): ParticleEmitterAsset {
  const item = record(value, `emitters[${index}]`);
  const id = identifier(item.id, `emitters[${index}].id`);
  if(typeof item.enabled !== 'boolean' || typeof item.root !== 'boolean') {
    throw new Error(`${id}: enabled and root must be booleans.`);
  }
  const duration = number(item.duration, `${id}.duration`, 0, 30);
  const rate = number(item.rate, `${id}.rate`, 0, 2000);
  const lifetime = number(item.lifetime, `${id}.lifetime`, 1 / 600, 30);
  if(!Array.isArray(item.bursts) || item.bursts.length > 128) {
    throw new Error(`${id}: bursts must be an array of at most 128 entries.`);
  }
  const bursts = item.bursts
    .map((value, burstIndex) => {
      const burst = record(value, `${id}.bursts[${burstIndex}]`);
      return {
        time: number(burst.time, `${id}.burst.time`, 0, duration),
        count: number(burst.count, `${id}.burst.count`, 1, 4096, true),
      };
    })
    .sort((a, b) => a.time - b.time);
  const renderer = item.renderer;
  if(
    renderer !== PARTICLE_RENDERER.SOFT &&
    renderer !== PARTICLE_RENDERER.STREAK &&
    renderer !== PARTICLE_RENDERER.RING &&
    renderer !== PARTICLE_RENDERER.BILLOW &&
    renderer !== PARTICLE_RENDERER.CIRCLE &&
    renderer !== PARTICLE_RENDERER.SQUARE &&
    renderer !== PARTICLE_RENDERER.TEXTURED &&
    renderer !== PARTICLE_RENDERER.FLIPBOOK
  ) {
    throw new Error(`${id}: unknown particle renderer.`);
  }
  if(typeof item.spawn !== 'string' || typeof item.update !== 'string') {
    throw new Error(`${id}: spawn and update scripts must be strings.`);
  }

  const emitter: ParticleEmitterAsset = {
    id,
    enabled: item.enabled,
    root: item.root,
    duration,
    rate,
    bursts,
    lifetime,
    renderer,
    spawn: item.spawn,
    update: item.update,
  };
  if(item.light !== undefined) {
    const light = record(item.light, `${id}.light`);
    emitter.light = {
      intensity: number(light.intensity, `${id}.light.intensity`, 0, 1000),
      range: number(light.range, `${id}.light.range`, 0.01, 10000),
    };
  }
  if(item.texture !== undefined) {
    emitter.texture = readTexture(item.texture, id);
  } else if(renderer === PARTICLE_RENDERER.TEXTURED || renderer === PARTICLE_RENDERER.FLIPBOOK) {
    throw new Error(`${id}: ${renderer} requires a texture.`);
  }
  if(item.trail !== undefined) {
    const trail = record(item.trail, `${id}.trail`);
    emitter.trail = {
      ...child(trail, `${id}.trail`),
      interval: number(trail.interval, `${id}.trail.interval`, 1 / 60, 30),
    };
  }
  if(item.death !== undefined) {
    emitter.death = child(record(item.death, `${id}.death`), `${id}.death`);
  }
  return emitter;
}
/** @param value @param id */
function readTexture(value: unknown, id: string): ParticleTexture {
  const item = record(value, `${id}.texture`);
  const { width, height } = readParticleTextureSource(item.source);
  const columns = number(item.columns === undefined ? 1 : item.columns, `${id}.texture.columns`, 1, Math.min(64, width), true);
  const rows = number(item.rows === undefined ? 1 : item.rows, `${id}.texture.rows`, 1, Math.min(64, height), true);
  const frames = number(item.frames === undefined ? columns * rows : item.frames, `${id}.texture.frames`, 1, columns * rows, true);
  const fps = number(item.fps === undefined ? 0 : item.fps, `${id}.texture.fps`, 0, 120);
  const loop = item.loop === undefined ? true : item.loop;
  if(typeof loop !== 'boolean') {
    throw new Error(`${id}.texture.loop must be a boolean.`);
  }
  return { source:  ((item.source) as string), columns, rows, frames, fps, loop };
}
/** @param source @param path */
function child(source: Record<string, unknown>, path: string) {
  return {
    emitter: identifier(source.emitter, `${path}.emitter`),
    count: number(source.count, `${path}.count`, 1, 4096, true),
  };
}
/** @param value @param path */
function record(value: unknown, path: string): Record<string, unknown> {
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`${path} must be an object.`);
  }
  return  ((value) as Record<string, unknown>);
}
/** @param value @param path */
function label(value: unknown, path: string) {
  if(typeof value !== 'string' || !value.trim() || value.length > 80) {
    throw new Error(`${path} requires 1–80 characters.`);
  }
  return value;
}
/** @param value @param path */
function identifier(value: unknown, path: string) {
  const name = label(value, path);
  if(!/^[a-zA-Z_][a-zA-Z_0-9]{0,47}$/.test(name)) {
    throw new Error(`${path} must be a simple identifier of at most 48 characters.`);
  }
  return name;
}
/** @param value @param path @param min @param max @param [integer] */
function number(value: unknown, path: string, min: number, max: number, integer: boolean = false) {
  if(
    typeof value !== 'number' ||
    !Number.isFinite(value) ||
    value < min ||
    value > max ||
    (integer && !Number.isInteger(value))
  ) {
    throw new Error(
      `${path} must be ${integer ? 'an integer' : 'a number'} from ${min} to ${max}.`,
    );
  }
  return value;
}
/** @param value */
function freezeRecord(value: object) {
  for(const child of Object.values(value)) {
    if(child && typeof child === 'object') {
      freezeRecord(child);
    }
  }
  Object.freeze(value);
}
