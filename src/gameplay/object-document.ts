// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { JsonRecord, EntityDefinition } from './types.ts';
import type { LevelData } from './level-document.ts';

import { copyEntityDefinition } from './entity-definition.ts';
import { assetPath } from './asset-path.ts';
import { parseLevel } from './level-document.ts';

export interface ObjectData {
  version: 1;
  name: string;
  entity: EntityDefinition;
}
export type { JsonRecord };

/**
 * Parse a reusable single-object definition; linked definitions cannot nest.
 * @param text */
export function parseObject(text: string): ObjectData {
  const value = JSON.parse(text);
  if(!value || value.version !== 1 || typeof value.name !== 'string' || !value.name.trim()) {
    throw new TypeError('An object requires version 1, a name and an entity.');
  }
  const entity = copyEntityDefinition(value.entity);
  if(entity.template || entity.overrides) {
    throw new TypeError('Object definitions cannot contain nested templates or overrides.');
  }
  return {version: 1, name: value.name, entity};
}

/**
 * Serialize a validated owned object without retaining caller references.
 * @param object
 */
export function serializeObject(object: ObjectData) {
  return `${JSON.stringify(parseObject(JSON.stringify(object)), null, 2)}\n`;
}

/**
 * Create an independently placed instance linked to one reusable definition.
 * @param object @param path @param id
 */
export function createObjectInstance(object: ObjectData, path: string, id: string) {
  const source = parseObject(serializeObject(object));
  return copyEntityDefinition({
    id, name: source.name, template: assetPath(path, ['.joyobject']),
    transform: source.entity.transform, overrides: {}
  });
}

/**
 * Resolve explicit local overrides over current source data; identity and placement stay local.
 * Null component overrides remove that component. Arrays replace as a unit.
 * @param entity @param object
 */
export function resolveObjectInstance(entity: EntityDefinition, object: ObjectData) {
  const instance = copyEntityDefinition(entity);
  const definition = parseObject(serializeObject(object)).entity;
  const values = mergeRecords((((({type: definition.type, tags: definition.tags, components: definition.components}) as unknown)) as JsonRecord), instance.overrides ?? {});
  const components = ((values.components) as JsonRecord);
  for(const [key, value] of Object.entries(components)) {
    if(value === null) {
      delete components[key];
    }
  }
  return copyEntityDefinition({
    id: instance.id, name: instance.name, transform: instance.transform,
    type:  ((values.type) as string), tags:  ((values.tags) as string[]),
    components:  ((components) as Record<string, JsonRecord>)
  });
}

/**
 * Resolve all linked objects with one source read per path; input data remains authored.
 * @param level
 * @param readText
 */
export async function resolveLevelObjects(level: LevelData, readText: (path: string) => Promise<string>): Promise<LevelData> {
  const copy = parseLevel(JSON.stringify(level));

  const sources: Map<string, Promise<ObjectData>> = new Map();
  copy.entities = await Promise.all(copy.entities.map(async entity => {
    if(!entity.template) {
      return entity;
    }
    const path = assetPath(entity.template, ['.joyobject']);
    let source = sources.get(path);
    if(!source) {
      source = readText(path).then(parseObject);
      sources.set(path, source);
    }
    return resolveObjectInstance(entity, await source);
  }));
  return copy;
}

/**
 * Merge finite copied JSON records without invoking inherited setters.
 * @param base @param overrides */
function mergeRecords(base: JsonRecord, overrides: JsonRecord): JsonRecord {
  const result = structuredClone(base);
  for(const [key, value] of Object.entries(overrides)) {
    const original = Object.hasOwn(base, key) ? base[key] : undefined;
    const next = isRecord(value) && isRecord(original) ? mergeRecords(original, value) : structuredClone(value);
    Object.defineProperty(result, key, {value: next, enumerable: true, writable: true, configurable: true});
  }
  return result;
}

/** @param value */
function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}
