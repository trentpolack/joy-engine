// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { EntityDefinition } from './types.ts';
import type { PostProcessingProfile } from '../rendering/postprocessor/profile.ts';

import { copyEntityDefinition } from './entity-definition.ts';
import { validatePostProcessingProfile } from '../rendering/postprocessor/profile.ts';

export interface LevelData {
  version: 1;
  name: string;
  entities: EntityDefinition[];
  postProcessing?: PostProcessingProfile;
}
const HISTORY_LIMIT = 80;

/**
 * Parse a versioned authored level. Returns an owned copy; never evaluates component data.
 * @param text
 */
export function parseLevel(text: string): LevelData {
  return copyLevel(JSON.parse(text));
}

/**
 * Validate and serialize a level without mutating its caller-owned data.
 * @param level
 */
export function serializeLevel(level: LevelData): string {
  return `${JSON.stringify(copyLevel(level), null, 2)}\n`;
}

/** Owns authored state, selection and up to 80 undo transactions. Simulation must use snapshot(). */
export class LevelDocument {
  private declare level: LevelData;
  private declare past: LevelData[];
  private declare future: LevelData[];
  declare selectedId: string | null;

  /**
   * @param level
   */
  constructor(level: LevelData) {
    /** @private */
    this.level = copyLevel(level);
    /** @private */
    this.past = [];
    /** @private */
    this.future = [];

    this.selectedId = null;
  }

  /** Return an owned copy suitable for rendering, simulation, or editing. */
  snapshot() {
    return copyLevel(this.level);
  }

  /** Whether an earlier authored snapshot is available. */
  get canUndo() {
    return this.past.length > 0;
  }

  /** Whether an undone authored snapshot is available. */
  get canRedo() {
    return this.future.length > 0;
  }

  /**
   * Selection is UI state, not an authored edit.
   * @param id
   */
  select(id: string | null) {
    this.selectedId = this.level.entities.some(entity => entity.id === id) ? id : null;
  }

  /**
   * Apply one atomic edit to an isolated draft. Invalid edits throw and retain prior state.
   * @param edit
   * @returns Whether authored data changed.
   */
  transact(edit: (draft: LevelData) => void): boolean {
    const draft = this.snapshot();
    edit(draft);
    const next = copyLevel(draft);
    if(JSON.stringify(next) === JSON.stringify(this.level)) {
      return false;
    }
    this.past.push(this.level);
    if(this.past.length > HISTORY_LIMIT) {
      this.past.shift();
    }
    this.level = next;
    this.future = [];
    this.select(this.selectedId);
    return true;
  }

  /** Restore one authored transaction; invalidated selections are cleared. */
  undo() {
    const previous = this.past.pop();
    if(!previous) {
      return false;
    }
    this.future.push(this.level);
    this.level = previous;
    this.select(this.selectedId);
    return true;
  }

  /** Reapply one undone transaction; invalidated selections are cleared. */
  redo() {
    const next = this.future.pop();
    if(!next) {
      return false;
    }
    this.past.push(this.level);
    this.level = next;
    this.select(this.selectedId);
    return true;
  }
}

/**
 * Validate the version, name, and unique entity IDs before returning an owned copy.
 * @param value
 */
function copyLevel(value: unknown): LevelData {
  if(!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new TypeError('A level must be an object.');
  }
  const level = ((value) as Record<string, unknown>);
  if(level.version !== 1) {
    throw new Error('Unsupported level version; expected version 1.');
  }
  if(typeof level.name !== 'string' || !level.name.trim()) {
    throw new TypeError('A level requires a name.');
  }
  if(!Array.isArray(level.entities)) {
    throw new TypeError('A level requires an entities array.');
  }
  const ids = new Set();
  const entities = level.entities.map(value => {
    const entity = copyEntityDefinition(value);
    if(ids.has(entity.id)) {
      throw new Error(`Duplicate entity ID: ${entity.id}`);
    }
    ids.add(entity.id);
    return entity;
  });
  const postProcessing = Object.hasOwn(level, 'postProcessing')
    ? validatePostProcessingProfile(level.postProcessing)
    : undefined;
  return postProcessing ? {version: 1, name: level.name, entities, postProcessing} : {version: 1, name: level.name, entities};
}
