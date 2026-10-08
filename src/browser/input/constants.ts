// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * KeyboardEvent codes and named navigation keys used by input bindings.
 */
export const KEY_CODE = Object.freeze({
  ESCAPE: 'Escape',
  ENTER: 'Enter',
  TAB: 'Tab',
  SPACE: 'Space',
  BACKSPACE: 'Backspace',
  DELETE: 'Delete',
  ARROW_UP: 'ArrowUp',
  ARROW_DOWN: 'ArrowDown',
  ARROW_LEFT: 'ArrowLeft',
  ARROW_RIGHT: 'ArrowRight',
  HOME: 'Home',
  END: 'End',
  PAGE_UP: 'PageUp',
  PAGE_DOWN: 'PageDown',
  SHIFT_LEFT: 'ShiftLeft',
  SHIFT_RIGHT: 'ShiftRight',
  CONTROL_LEFT: 'ControlLeft',
  CONTROL_RIGHT: 'ControlRight',
  KEY_A: 'KeyA',
  KEY_B: 'KeyB',
  KEY_C: 'KeyC',
  KEY_D: 'KeyD',
  KEY_E: 'KeyE',
  KEY_F: 'KeyF',
  KEY_G: 'KeyG',
  KEY_H: 'KeyH',
  KEY_I: 'KeyI',
  KEY_J: 'KeyJ',
  KEY_K: 'KeyK',
  KEY_L: 'KeyL',
  KEY_M: 'KeyM',
  KEY_N: 'KeyN',
  KEY_O: 'KeyO',
  KEY_P: 'KeyP',
  KEY_Q: 'KeyQ',
  KEY_R: 'KeyR',
  KEY_S: 'KeyS',
  KEY_T: 'KeyT',
  KEY_U: 'KeyU',
  KEY_V: 'KeyV',
  KEY_W: 'KeyW',
  KEY_X: 'KeyX',
  KEY_Y: 'KeyY',
  KEY_Z: 'KeyZ',
  DIGIT1: 'Digit1',
  DIGIT2: 'Digit2',
  DIGIT3: 'Digit3',
  DIGIT4: 'Digit4',
  DIGIT5: 'Digit5',
  DIGIT6: 'Digit6',
  DIGIT7: 'Digit7',
  DIGIT8: 'Digit8',
  DIGIT9: 'Digit9',
  DIGIT0: 'Digit0'
} as const);

/**
 * Default input routing context.
 */
export const INPUT_CONTEXT = Object.freeze({
  GAMEPLAY: 'gameplay'
} as const);

/**
 * Input binding discriminants used during device-state evaluation.
 */
export const INPUT_BINDING_TYPE = Object.freeze({
  KEY: 'key',
  POINTER: 'pointer',
  BUTTON: 'button',
  VIRTUAL: 'virtual'
} as const);
