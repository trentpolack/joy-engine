// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { ENTITY_TYPE, PARTICLE_RENDERER, PRIMITIVE_SHAPE } from 'joy-engine/constants';

/**
 * Select a specialist editor explicitly; unknown extensions never fall through to FORM.
 * @param path
 */
export function editorUrl(path: string) {
  if(/\.(joylevel|joyobject)$/i.test(path)) {
    return '/editor/level.html?workspace=1';
  }
  if(/\.joyfx$/i.test(path)) {
    return '/joy-editor/tools/particle-lab/?workspace=1';
  }
  if(/\.(form|formlab)$/i.test(path)) {
    return '/joy-editor/tools/form-lab/?workspace=1';
  }
  throw new Error(`Unsupported asset editor: ${path}`);
}

/**
 * Identify formats that the workspace can route to an owned editor.
 * @param path
 */
export function isSupportedAssetPath(path: string): boolean {
  return /\.(joylevel|joyobject|form|formlab|joyfx)$/i.test(path);
}

/**
 * Produce starter content accepted by the current authoring and runtime readers.
 * @param path
 */
export function createAssetText(path: string) {
  editorUrl(path);
  const name = path.split('/').at(-1)?.replace(/\.[^.]+$/, '') ?? 'New asset';
  const source = `project meta(name=${JSON.stringify(name)})\ncolor(0.93, 0.42, 0.22)\nbox(0, 0.5, 0, 1, 1, 1)\n`;
  if(path.endsWith('.form')) {
    return source;
  }
  let value;
  if(path.endsWith('.formlab')) {
    value = {version: 1, name, source, overrides: {}};
  } else if(path.endsWith('.joyfx')) {
    value = {version: 1, name, maxParticles: 128, parameters: {}, emitters: [{
      id: 'sparks', enabled: true, root: true, duration: 2, rate: 12,
      bursts: [{time: 0, count: 8}], lifetime: 1, renderer: PARTICLE_RENDERER.SOFT,
      spawn: 'velocity.x = rand(-0.5, 0.5); velocity.y = rand(1, 2); velocity.z = rand(-0.5, 0.5); size = 0.15; color.r = 2; color.g = 0.5; color.b = 0.1;',
      update: 'color.a = 1 - t;'
    }]};
  } else if(path.endsWith('.joyobject')) {
    value = {version: 1, name, entity: {id: 'object', name, type: ENTITY_TYPE.ENTITY, components: {render: {shape: PRIMITIVE_SHAPE.BOX, size: [1, 1, 1], color: [0.93, 0.42, 0.22]}}}};
  } else {
    value = {version: 1, name, entities: []};
  }
  return `${JSON.stringify(value, null, 2)}\n`;
}
