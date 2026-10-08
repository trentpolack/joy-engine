// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { PARTICLE_EMITTER_PRESET, PARTICLE_EXAMPLE_EMITTER } from './constants.ts';
import { PARTICLE_PARAMETER_TYPE, PARTICLE_RENDERER } from 'joy-engine/constants';
import type { ParticleEffectAsset, ParticleEmitterAsset } from 'joy-engine';
import orbitingLightsSource from '../assets/joyfx/orbiting-lights.joyfx?raw';
import prismSpriteGardenAssetSource from '../assets/joyfx/prism-sprite-garden.joyfx?raw';

const ORBITING_LIGHTS: ParticleEffectAsset = JSON.parse(orbitingLightsSource);

const prismSpriteGardenAsset: ParticleEffectAsset = JSON.parse(prismSpriteGardenAssetSource);

// Original procedural star and four expanding-ring frames are embedded in the
// portable example; source stays detached when the editor creates a document.

const PRISM_SPRITE_GARDEN: ParticleEffectAsset = ((prismSpriteGardenAsset) as ParticleEffectAsset);

const EMBER_IMPACT = {
  version:  ((1) as const),
  name: 'Ember Impact',
  maxParticles: 2200,
  parameters: {
    power: { value: 1, min: 0.25, max: 2 },
    wind: { type:  PARTICLE_PARAMETER_TYPE.VECTOR3, value:  (([8, 0, 0]) as [
    number,
    number,
    number
]), min:  (([-50, -50, -50]) as [
    number,
    number,
    number
]), max:  (([50, 50, 50]) as [
    number,
    number,
    number
]) },
    tint: { type:  PARTICLE_PARAMETER_TYPE.COLOR, value:  (([1, 0.65, 0.08, 1]) as [
    number,
    number,
    number,
    number
]), min:  (([0, 0, 0, 0]) as [
    number,
    number,
    number,
    number
]), max:  (([1, 1, 1, 1]) as [
    number,
    number,
    number,
    number
]) },
  },
  emitters: [
    {
      id: PARTICLE_EXAMPLE_EMITTER.FLASH, enabled: true, root: true, duration: 0, rate: 0,
      bursts: [{ time: 0, count: 26 }], lifetime: 0.42, renderer: PARTICLE_RENDERER.RING,
      spawn: `let angle = rand(0, tau);
size = rand(10, 22) * power;
position = origin + vector(cos(angle)*rand(0, 8), sin(angle)*rand(0, 8), rand(-8, 8));
color = tint;
color.r*= 3;`,
      update: `size += 90 * dt;
color.a = 1 - t;
color.r = mix(3, 1.2, t);`,
      death: { emitter: PARTICLE_EXAMPLE_EMITTER.SMOKE, count: 1 },
    },
    {
      id: PARTICLE_EXAMPLE_EMITTER.SPARKS, enabled: true, root: true, duration: 0, rate: 0,
      bursts: [{ time: 0, count: 110 }], lifetime: 1.25, renderer: PARTICLE_RENDERER.STREAK,
      spawn: `let angle = rand(0, tau);
let speed = rand(120, 360) * power;
let vertical = rand(-1, 1);
let radial = sqrt(1 - vertical * vertical);
velocity = vector(cos(angle)*radial, vertical, sin(angle)*radial)*speed;
size = rand(2, 5);
color = rgba(rand(2, 4), rand(0.25, 0.8), 0.03, 1);
lifetime = rand(0.45, 1.25);`,
      update: `velocity.y-= 190*dt;
velocity+= wind*dt;
size = mix(5, 0.5, t);
color.a = 1 - smoothstep(0.55, 1, t);`,
      trail: { emitter: PARTICLE_EXAMPLE_EMITTER.SMOKE, interval: 0.08, count: 1 },
    },
    {
      id: PARTICLE_EXAMPLE_EMITTER.BILLOW, enabled: true, root: true, duration: 0.16, rate: 180,
      bursts: [{ time: 0, count: 36 }], lifetime: 1.15, renderer: PARTICLE_RENDERER.BILLOW,
      spawn: `let angle = rand(0, tau);
let speed = rand(25, 95) * power;
let vertical = rand(-1, 1);
let radial = sqrt(1 - vertical * vertical);
velocity = vector(cos(angle)*radial, vertical, sin(angle)*radial)*speed;
size = rand(12, 30);
color = rgba(2.4, rand(0.2, 0.55), 0.04, 1);
lifetime = rand(0.55, 1.15);`,
      update: `drag = 2.4;
size += 32 * dt;
color.a = (1 - t) * 0.7;
color.g = mix(0.55, 0.07, t);`,
    },
    {
      id: PARTICLE_EXAMPLE_EMITTER.SMOKE, enabled: true, root: false, duration: 0, rate: 0,
      bursts: [], lifetime: 1.8, renderer: PARTICLE_RENDERER.SOFT,
      spawn: `velocity = parentVelocity*0.04 + vector(rand(-9, 9), rand(8, 22), rand(-9, 9));
size = rand(7, 15);
color = rgba(0.22, 0.16, 0.13, 1);
lifetime = rand(0.7, 1.8);`,
      update: `drag = 1.8;
size += 18 * dt;
color.a = (1 - t) * 0.34;`,
    },
  ],
};

const SOLAR_SWIRL = {
  version:  ((1) as const),
  name: 'Solar Swirl',
  maxParticles: 1800,
  parameters: {
    radius: { value: 145, min: 40, max: 260 },
    swirlSpeed: { value: 2.2, min: -5, max: 5 },
    lift: { value: 18, min: -40, max: 60 },
  },
  emitters: [
    {
      id: PARTICLE_EXAMPLE_EMITTER.ORBIT, enabled: true, root: true, duration: 6, rate: 100,
      bursts: [], lifetime: 2.4, renderer: PARTICLE_RENDERER.SOFT,
      spawn: `let angle = time * swirlSpeed + rand(-0.25, 0.25);
let reach = radius + rand(-18, 18);
position = origin + vector(cos(angle)*reach, rand(-30, 30), sin(angle)*reach);
velocity = vector(-sin(angle)*radius*swirlSpeed, lift, cos(angle)*radius*swirlSpeed);
size = rand(4, 11);
color = rgba(rand(1.4, 2.8), rand(0.15, 0.65), rand(0.35, 1.2), 1);
lifetime = rand(1.2, 2.4);`,
      update: `let pull = -1.6;
acceleration = vector((position.x - origin.x)*pull, lift, (position.z - origin.z)*pull);
drag = 0.35;
color.a = smoothstep(0, 0.12, t) * (1 - smoothstep(0.6, 1, t));
size = mix(size, 2, t);`,
      trail: { emitter: PARTICLE_EXAMPLE_EMITTER.DUST, interval: 0.12, count: 1 },
    },
    {
      id: PARTICLE_EXAMPLE_EMITTER.CORE, enabled: true, root: true, duration: 6, rate: 45,
      bursts: [], lifetime: 0.7, renderer: PARTICLE_RENDERER.RING,
      spawn: `let angle = rand(0, tau);
position = origin + vector(cos(angle)*rand(0, 20), rand(-20, 20), sin(angle)*rand(0, 20));
size = rand(8, 16); color = rgba(2.8, 0.45, 0.7, 1);`,
      update: 'size += 34 * dt; color.a = 1 - t;',
    },
    {
      id: PARTICLE_EXAMPLE_EMITTER.DUST, enabled: true, root: false, duration: 0, rate: 0,
      bursts: [], lifetime: 0.9, renderer: PARTICLE_RENDERER.SOFT,
      spawn: `velocity = parentVelocity*0.08;
size = rand(3, 7); color = rgba(0.55, 0.12, 0.7, 1);`,
      update: 'size += 8 * dt; color.a = (1 - t) * 0.22;',
    },
  ],
};

export const EXAMPLES = [
  { label: 'Impact / Ember Bloom', asset: EMBER_IMPACT },
  { label: 'Continuous / Solar Swirl', asset: SOLAR_SWIRL },
  { label: 'Materials / Prism Sprite Garden', asset: PRISM_SPRITE_GARDEN },
  { label: 'Lighting / Orbiting Lights', asset: ORBITING_LIGHTS },
];

/** Small insertion recipes never borrow an effect's parameters or child emitters. */
export const EMITTER_PRESETS = [
  {id: PARTICLE_EMITTER_PRESET.SCRATCH, label: 'Scratch', description: 'Empty scripts and no emission; build your own.'},
  {id: PARTICLE_EMITTER_PRESET.SPRAY, label: 'Soft Spray', description: 'Continuous particles moving outward in 3D.'},
  {id: PARTICLE_EMITTER_PRESET.SPARKS, label: 'Spark Burst', description: 'A short streak burst with gravity.'},
  {id: PARTICLE_EMITTER_PRESET.RING, label: 'Ring Flash', description: 'Expanding rings for an impact.'},
  {id: PARTICLE_EMITTER_PRESET.SMOKE, label: 'Smoke', description: 'Rising soft particles with a gentle fade.'}
];

/** Return a fully detached authored example. @param index */
export function copyExample(index: number) {
  const asset = EXAMPLES[index]?.asset ?? EXAMPLES[0].asset;
  return  ((
     ((structuredClone(asset)) as unknown)) as ParticleEffectAsset
  );
}

/** Default authoring template: isotropic motion in XYZ, not an XY fan.
 * @param id @param [preset] */
export function createEmitterTemplate(id: string, preset: string = PARTICLE_EMITTER_PRESET.SPRAY): ParticleEmitterAsset {

  const emitter: ParticleEmitterAsset = {
    id, enabled: true, root: true, duration: 1, rate: 20, bursts: [],
    lifetime: 1, renderer: PARTICLE_RENDERER.SOFT,
    spawn: `let angle = rand(0, tau);
let vertical = rand(-1, 1);
let radial = sqrt(1 - vertical * vertical);
velocity = vector(cos(angle) * radial, vertical, sin(angle) * radial) * 80;
size = 3;`,
    update: 'color.a = 1 - t;',
  };
  if(preset === PARTICLE_EMITTER_PRESET.SCRATCH) {
    emitter.rate = 0;
    emitter.spawn = '';
    emitter.update = '';
  } else if(preset === PARTICLE_EMITTER_PRESET.SPARKS) {
    emitter.duration = 0;
    emitter.rate = 0;
    emitter.renderer = PARTICLE_RENDERER.STREAK;
    emitter.bursts = [{time: 0, count: 40}];
    emitter.spawn+= '\ncolor = rgba(2.8, 0.6, 0.08, 1);';
    emitter.update = 'velocity.y-= 90*dt; color.a = 1 - t;';
  } else if(preset === PARTICLE_EMITTER_PRESET.RING) {
    emitter.duration = 0;
    emitter.rate = 0;
    emitter.renderer = PARTICLE_RENDERER.RING;
    emitter.lifetime = 0.5;
    emitter.bursts = [{time: 0, count: 3}];
    emitter.spawn = 'size = rand(6, 12); color = rgba(2.4, 0.7, 0.1, 1);';
    emitter.update = 'size+= 60*dt; color.a = 1 - t;';
  } else if(preset === PARTICLE_EMITTER_PRESET.SMOKE) {
    emitter.lifetime = 2;
    emitter.spawn = 'velocity = vector(rand(-6, 6), rand(12, 22), rand(-6, 6)); size = rand(6, 10); color = rgba(0.4, 0.45, 0.5, 0.7);';
    emitter.update = 'size+= 8*dt; color.a = (1 - t)*0.7;';
  }
  return  ((emitter) as ParticleEmitterAsset);
}
