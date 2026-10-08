// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Character } from './character.ts';
import type { ControlIntent } from './types.ts';

import {Controller} from './controller.ts';

export type IntentSampler = (elapsedSeconds: number, character: Character) => ControlIntent;

export interface PlayerControllerOptions {
  sampleInput: IntentSampler;
}

/** Samples borrowed keyboard/touch/gamepad adapters without owning their listeners. */
export class PlayerController extends Controller {
  declare sampleInput: IntentSampler;

  /**
   * @param options
   */
  constructor({sampleInput}: PlayerControllerOptions) {
    super();
    if(typeof sampleInput !== 'function') {
      throw new TypeError('PlayerController requires a sampleInput callback.');
    }
    this.sampleInput = sampleInput;
  }

  /**
   * @param elapsedSeconds Nonnegative simulation seconds. Samples replace previous intent.
   */
  update(elapsedSeconds: number) {
    super.update(elapsedSeconds);
    const character = this.character;
    if(this.destroyed || !character) {
      return;
    }
    character.resetControl();
    const intent = this.sampleInput(elapsedSeconds, character);
    // A sampler may transfer possession; never deliver a sample to its previous target.
    if(this.character === character && !character.destroyed) {
      character.receiveControl(intent);
    }
  }
}

export interface AIControllerOptions {
  sampleIntent: IntentSampler;
}

/** AI decisions use the same intent and possession contract as player input. */
export class AIController extends PlayerController {
  /**
   * @param options
   */
  constructor({sampleIntent}: AIControllerOptions) {
    super({sampleInput: sampleIntent});
  }
}
