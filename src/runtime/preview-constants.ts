// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Same-origin preview transport channels shared by editor and runtime.
 */
export const PREVIEW_CHANNEL = Object.freeze({
  RUNTIME: 'joy-editor-runtime',
  SESSION: 'joy-editor-session',
  COMMAND: 'joy-editor-preview'
} as const);

/**
 * Commands sent by the editor to a game preview.
 */
export const PREVIEW_COMMAND = Object.freeze({
  PAUSE: 'pause',
  REGENERATE: 'regenerate',
  ASSET: 'asset'
} as const);

/**
 * Connection and content states reported by preview runtimes.
 */
export const PREVIEW_STATE = Object.freeze({
  READY: 'READY',
  CONNECTED: 'CONNECTED',
  DISCONNECTED: 'DISCONNECTED',
  CONTENT_ERROR: 'CONTENT ERROR'
} as const);
