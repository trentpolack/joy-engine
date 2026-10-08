// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * DOM event names shared by listener registration and cleanup.
 */
export const BROWSER_EVENT = Object.freeze({
  CLICK: 'click',
  CHANGE: 'change',
  INPUT: 'input',
  KEYDOWN: 'keydown',
  KEYUP: 'keyup',
  POINTERDOWN: 'pointerdown',
  POINTERUP: 'pointerup',
  POINTERMOVE: 'pointermove',
  POINTERCANCEL: 'pointercancel',
  LOSTPOINTERCAPTURE: 'lostpointercapture',
  WHEEL: 'wheel',
  BLUR: 'blur',
  FOCUS: 'focus',
  FOCUSIN: 'focusin',
  FOCUSOUT: 'focusout',
  CONTEXTMENU: 'contextmenu',
  RESIZE: 'resize',
  PAGEHIDE: 'pagehide',
  BEFOREUNLOAD: 'beforeunload',
  VISIBILITYCHANGE: 'visibilitychange',
  MESSAGE: 'message'
} as const);
