// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { ENTITY_TYPE } from './constants.ts';
import type { EntityDefinition, EntityOptions } from './types.ts';
import {Entity} from './entity.ts';
import {Character, PlayerCharacter} from './character.ts';
import {copyEntityDefinition} from './entity-definition.ts';

export type EntityFactory = (definition: EntityDefinition) => Entity;

/** Trusted project factories interpret copied JSON; authored type names never evaluate code. */
export class EntityRegistry {
  private declare factories: Map<string, EntityFactory>;

  /** Register the built-in entity types for this independent factory collection. */
  constructor() {
    /** @private */
    this.factories = new Map();
    this.register(ENTITY_TYPE.ENTITY, (definition) => new Entity(definition));
    this.register(ENTITY_TYPE.CHARACTER, (definition) => new Character(definition));
    this.register(ENTITY_TYPE.PLAYER_CHARACTER, (definition) => new PlayerCharacter(definition));
  }

  /**
   * Register a trusted factory; duplicate names and invalid callbacks throw.
   * @param type Nonempty authored type name.
   * @param factory Borrowed creation callback.
   * @returns This registry for chained registration.
   */
  register(type: string, factory: EntityFactory): this {
    if(typeof type !== 'string' || !type.trim() || typeof factory !== 'function') {
      throw new TypeError('Entity registration requires a nonempty type and factory.');
    }
    if(this.factories.has(type)) {
      throw new Error(`Entity type ${type} is already registered.`);
    }
    this.factories.set(type, factory);
    return this;
  }

  /**
   * Return a new unowned entity. The caller must destroy it or transfer it to a world.
   * Factories must return a fresh instance with the requested id and acquire resources in onSpawn.
   * @param definition
   */
  create(definition: EntityOptions): Entity {
    const data = copyEntityDefinition(definition);
    const factory = this.factories.get(data.type);
    if(!factory) {
      throw new Error(`Unknown entity type: ${data.type}.`);
    }
    const expectedId = data.id;
    const entity = factory(data);
    if(!(entity instanceof Entity)) {
      throw new TypeError('Entity factory must return an Entity.');
    }
    if(entity.world || entity.destroyed) {
      throw new Error('Entity factory must return a fresh, unowned entity.');
    }
    if(entity.id !== expectedId) {
      try {
        entity.destroy();
      } catch(error) {
        throw new AggregateError([error], 'Entity factory returned the wrong id and cleanup failed.');
      }
      throw new Error('Entity factory returned the wrong id.');
    }
    return entity;
  }
}
