// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Same-origin channels between workspace and hosted asset editors.
 */
export const WORKSPACE_CHANNEL = Object.freeze({
  TOOL: 'joy-editor-tool',
  HOST: 'joy-editor-host'
} as const);

/**
 * Workspace document transport message types.
 */
export const WORKSPACE_MESSAGE = Object.freeze({
  READY: 'ready',
  LOADED: 'loaded',
  CHANGED: 'changed',
  CAPTURED: 'captured',
  ERROR: 'error',
  SAVE: 'save',
  REQUEST: 'request',
  RESPONSE: 'response',
  LOAD: 'load',
  ACTIVE: 'active',
  CAPTURE: 'capture',
  ASSETS_CHANGED: 'assets-changed',
  WORKSPACE_MAXIMIZED: 'workspace-maximized',
  MAXIMIZE_WORKSPACE: 'maximize-workspace',
  FOCUS_SEARCH: 'focus-search'
} as const);

/**
 * Operations requested by hosted asset editors.
 */
export const WORKSPACE_OPERATION = Object.freeze({
  CATALOG: 'catalog',
  READ: 'read',
  CREATE: 'create',
  OPEN: 'open',
  FOCUS_SEARCH: 'focus-search'
} as const);

/**
 * Messages exchanged with separate preview windows.
 */
export const PREVIEW_HOST_MESSAGE = Object.freeze({
  ALIVE: 'alive',
  DISCONNECTED: 'disconnected',
  RUNTIME: 'runtime',
  COMMAND: 'command'
} as const);

/**
 * Versioned browser preferences; values preserve existing saved layouts.
 */
export const WORKSPACE_STORAGE_KEY = Object.freeze({
  LAYOUT: 'joy-editor-workspace-v1',
  ASSETS: 'joy-editor-assets-v2',
  PLAY: 'joy-editor-play-v2',
  CUSTOM_SIZE: 'joy-editor-custom-size'
} as const);
