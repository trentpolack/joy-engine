// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Level viewport manipulation modes.
 */
export const LEVEL_TOOL = Object.freeze({
  SELECT: 'select',
  MOVE: 'move'
} as const);

/**
 * Add-component menu value for an unschematized record named by the author.
 * Preset ids from the component schema catalog fill the rest of the menu.
 */
export const CUSTOM_COMPONENT_OPTION = 'custom';
