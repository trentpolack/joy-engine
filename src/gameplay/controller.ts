// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {Character} from './character.ts';

/** Owns exclusive possession, but borrows the character and its world. */
export class Controller {
  declare character: Character | null;
  declare destroyed: boolean;

  /** Start unpossessed; a later possess call borrows an existing character. */
  constructor() {

    this.character = null;
    this.destroyed = false;
  }

  /**
   * Transfer exclusive possession, clearing both sides of the previous relationships and all stale intent.
   * Passing null releases possession. Destroyed characters or controllers cannot be possessed.
   * Reset hooks observe committed ownership; a reentrant possession supersedes this transition.
   * Hook failures are reported after both intents are cleared and ownership is consistent.
   * @param character
   */
  possess(character: Character | null) {
    if(this.destroyed && character !== null) {
      throw new Error('Cannot possess with a destroyed controller.');
    }
    if(character !== null && (!(character instanceof Character) || character.destroyed || character.destroying)) {
      throw new Error('Cannot possess a destroyed or invalid character.');
    }
    if(this.character === character) {
      return;
    }
    const previous = this.character;
    const displaced = character?.controller;

    // Commit every backreference before invoking project hooks. A hook may make a
    // newer possession decision; no writes after these callbacks may overwrite it.
    this.character = character;
    if(previous) {
      previous.controller = null;
      previous.controlIntent = {};
    }
    if(displaced) {
      displaced.character = null;
    }
    if(character) {
      character.controller = this;
      character.controlIntent = {};
    }
    const errors = [];
    for(const changed of [previous, character]) {
      if(!changed) {
        continue;
      }
      try {
        changed.resetControl();
      } catch(error) {
        errors.push(error);
      }
    }
    if(errors.length) {
      throw new AggregateError(errors, 'Possession reset failed.');
    }
  }

  /**
   * Sample before EntityWorld.update; controller scheduling belongs to the runtime composition root.
   * @param elapsedSeconds Nonnegative simulation seconds.
   */
  update(elapsedSeconds: number) {
    if(!Number.isFinite(elapsedSeconds) || elapsedSeconds < 0) {
      this.character?.resetControl();
      throw new RangeError('Controller delta must be finite nonnegative seconds.');
    }
  }

  /** Release possession; the caller still owns borrowed input and the character. Repeated calls are harmless. */
  destroy() {
    if(this.destroyed) {
      return;
    }
    this.destroyed = true;
    this.possess(null);
  }
}
