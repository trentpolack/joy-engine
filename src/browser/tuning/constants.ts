// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * When a tuning edit affects a running game.
 */
export const TUNING_APPLY_MODE = Object.freeze({
  LIVE: 'live',
  NEXT_SPAWN: 'next-spawn',
  RESTART: 'restart'
} as const);
