// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { EntityOptions, ControlIntent } from './types.ts';
import type { Controller } from './controller.ts';

import {Entity} from './entity.ts';

/** Gameplay owner that responds to controller intent; movement policy belongs to subclasses. */
export class Character extends Entity {
  declare controller: Controller | null;
  declare destroying: boolean;
  declare controlIntent: ControlIntent;

  /**
   * @param definition
   */
  constructor(definition: EntityOptions) {
    super(definition);
    /** Borrowed exclusive controller; changed by Controller.possess. */
    this.controller = null;
    /** Blocks reacquisition while project cleanup hooks execute. */
    this.destroying = false;
    /** Copied current actions, replaced by each sample. */
    this.controlIntent = {};
  }

  /**
   * Replace all actions, copying the caller's record. Invalid samples clear previous intent and throw.
   * @param intent Project-defined numeric or boolean actions.
   */
  receiveControl(intent: ControlIntent) {
    this.resetControl();
    if(this.destroyed || this.destroying) {
      throw new Error('Cannot control a destroyed character.');
    }
    if(!intent || typeof intent !== 'object' || Array.isArray(intent)) {
      throw new TypeError('Control intent must be a record.');
    }
    const entries = Object.entries(intent);
    for(const [, value] of entries) {
      if(typeof value !== 'boolean' && !(typeof value === 'number' && Number.isFinite(value))) {
        throw new TypeError('Control intent values must be finite numbers or booleans.');
      }
    }
    this.controlIntent = Object.fromEntries(entries);
  }

  /** Clear transient actions when possession changes, input fails, or simulation stops. */
  resetControl() {
    this.controlIntent = {};
  }

  /** Release possession and all resources even if project reset or cleanup hooks throw. */
  destroy() {
    if(this.destroyed || this.destroying) {
      return;
    }
    if(this.world) {
      this.world.remove(this.id);
      return;
    }
    this.destroying = true;
    this.controlIntent = {};
    const errors = [];
    try {
      if(this.controller) {
        this.controller.possess(null);
      } else {
        this.resetControl();
      }
    } catch(error) {
      errors.push(error);
    }
    // Ownership is already detached and reacquisition is blocked, regardless of
    // reset-hook errors. Always proceed to owned-resource cleanup.
    this.controlIntent = {};
    try {
      super.destroy();
    } catch(error) {
      errors.push(error);
    } finally {
      this.destroying = false;
    }
    if(errors.length) {
      throw new AggregateError(errors, 'Character cleanup failed.');
    }
  }
}

/** Optional project extension point for player-specific character behavior. AI may also possess it. */
export class PlayerCharacter extends Character {}
