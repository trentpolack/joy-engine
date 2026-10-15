// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import { GameShell } from '../src/runtime/game-shell.ts';

test('GameShell creates resources, runs ordered phases, and disposes in dependency order', async () => {
  const originalWindow = globalThis.window;
  const originalDocument = globalThis.document;
  const originalRequestAnimationFrame = globalThis.requestAnimationFrame;
  const originalCancelAnimationFrame = globalThis.cancelAnimationFrame;
  const windowTarget = new EventTarget();
  const documentTarget = new EventTarget();
  const frames = [];
  const events = [];

  globalThis.window = windowTarget;
  globalThis.document = documentTarget;
  globalThis.requestAnimationFrame = callback => {
    frames.push(callback);
    return frames.length;
  };
  globalThis.cancelAnimationFrame = () => {};

  try {
    const shell = new GameShell({
      title: 'TEST',
      development: false,
      createConfiguration: () => {
        events.push('configuration');
        return {quality: 'high'};
      },
      createRenderer: async () => {
        events.push('renderer');
        return {destroy: () => events.push('destroy renderer')};
      },
      createInput: () => {
        events.push('input resource');
        return {destroy: () => events.push('destroy input')};
      },
      initialize: () => events.push('initialize'),
      input: () => events.push('input'),
      simulation: () => events.push('simulation'),
      presentation: () => events.push('presentation'),
      render: frame => events.push(`render ${frame.elapsedSeconds}`),
      afterFrame: () => events.push('after frame'),
      dispose: () => events.push('dispose application'),
      fail: error => assert.fail(error)
    });

    await shell.initialize();
    assert.deepEqual(events, ['configuration', 'renderer', 'input resource', 'initialize']);
    frames.shift()(1000);
    frames.shift()(1200);
    assert.deepEqual(events.slice(4), [
      'input', 'simulation', 'presentation', 'render 0', 'after frame',
      'input', 'simulation', 'presentation', 'render 0.1', 'after frame'
    ]);

    shell.dispose();
    shell.dispose();
    assert.deepEqual(events.slice(-3), ['dispose application', 'destroy input', 'destroy renderer']);
  } finally {
    globalThis.window = originalWindow;
    globalThis.document = originalDocument;
    globalThis.requestAnimationFrame = originalRequestAnimationFrame;
    globalThis.cancelAnimationFrame = originalCancelAnimationFrame;
  }
});
