// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { AudioBank } from 'joy-engine';

class FakeNode {
  constructor() {
    this.disconnected = false;
    this.gain = { value: 1 };
    this.onended = null;
  }
  connect() {}
  disconnect() {
    this.disconnected = true;
  }
  start() {
    this.started = true;
  }
  stop() {
    this.stopped = true;
    this.onended?.();
  }
}
class FakeContext {
  state = 'suspended';
  destination = {};
  sources = [];
  gains = [];
  decoded = [];
  resumes = 0;
  closes = 0;
  async decodeAudioData(bytes) {
    const buffer = { bytes };
    this.decoded.push(buffer);
    return buffer;
  }
  createBufferSource() {
    const node = new FakeNode();
    this.sources.push(node);
    return node;
  }
  createGain() {
    const node = new FakeNode();
    this.gains.push(node);
    return node;
  }
  async resume() {
    this.resumes++;
    this.state = 'running';
  }
  async close() {
    this.closes++;
    this.state = 'closed';
  }
}
async function bank(t, options = {}) {
  const context = new FakeContext();
  let fetches = 0;
  t.mock.method(globalThis, 'fetch', async () => {
    fetches++;
    return new Response(new Uint8Array([1, 2, 3]));
  });
  const audio = await AudioBank.create({ fire: { url: '/fire.wav', volume: 0.18, maximumVoices: 2 }, boom: { url: '/boom.wav', volume: 0.22 } }, { contextFactory: () => context, ...options });
  return { audio, context, fetches: () => fetches };
}

test('repeated playback shares decoded buffers and creates no media elements or new loads', async t => {
  const { audio, context, fetches } = await bank(t);
  assert.equal(fetches(), 2);
  assert.equal(context.decoded.length, 2);
  assert.equal(audio.play('fire'), false, 'suspended playback is dropped');
  await audio.resume();
  assert.equal(audio.play('fire'), true);
  assert.equal(audio.play('fire'), true);
  assert.equal(context.sources[0].buffer, context.sources[1].buffer);
  assert.equal(context.sources[0].buffer, context.decoded[0]);
  assert.equal(fetches(), 2);
  assert.equal(context.gains[0].gain.value, 0.18);
  audio.destroy();
});

test('voice limits steal the oldest matching sound before unrelated effects', async t => {
  const { audio, context } = await bank(t, { maximumVoices: 3 });
  await audio.resume();
  audio.play('boom');
  audio.play('fire');
  audio.play('fire');
  audio.play('fire');
  assert.equal(context.sources[1].stopped, true);
  assert.equal(context.sources[0].stopped, undefined);
  assert.equal(audio.activeVoiceCount, 3);
  audio.play('boom');
  assert.equal(context.sources[0].stopped, true, 'global limit retires the oldest voice');
  assert.equal(audio.activeVoiceCount, 3);
  context.sources.at(-1).onended();
  assert.equal(audio.activeVoiceCount, 2);
  assert.equal(context.sources.at(-1).disconnected, true);
  audio.destroy();
});

test('destroy stops/disconnects voices and closes its context exactly once', async t => {
  const { audio, context } = await bank(t);
  await audio.resume();
  audio.play('fire');
  audio.destroy();
  audio.destroy();
  assert.equal(context.sources[0].stopped, true);
  assert.ok(context.gains.every(node => node.disconnected));
  assert.equal(context.closes, 1);
  assert.equal(audio.activeVoiceCount, 0);
  assert.equal(audio.play('fire'), false);
  assert.equal(await audio.resume(), false);
});

test('failed loading closes the allocated context and identifies the sound', async t => {
  const context = new FakeContext();
  t.mock.method(globalThis, 'fetch', async () => new Response(null, { status: 404 }));
  await assert.rejects(AudioBank.create({ fire: { url: '/missing.wav' } }, { contextFactory: () => context }), /fire.*404/);
  assert.equal(context.closes, 1);
});

test('interrupted contexts can resume again without queueing missed shots', async t => {
  const { audio, context } = await bank(t);
  await audio.resume();
  context.state = 'interrupted';
  assert.equal(audio.play('fire'), false);
  await audio.resume();
  assert.equal(context.sources.length, 0);
  assert.equal(audio.play('fire'), true);
  audio.destroy();
});

test('invalid voice limits are rejected before allocating a context', async () => {
  let allocations = 0;
  await assert.rejects(AudioBank.create({}, { maximumVoices: 0, contextFactory: () => {
    allocations++;
    return new FakeContext();
  } }), /maximumVoices/);
  assert.equal(allocations, 0);
});

test('a pending denied resume does not block a later activating gesture', async t => {
  const { audio, context } = await bank(t);
  let finishFirst;
  t.mock.method(context, 'resume', () => {
    context.resumes++;
    if(context.resumes === 1) {
      return new Promise(resolve => {
        finishFirst = resolve;
      });
    }
    context.state = 'running';
    finishFirst();
    return Promise.resolve();
  });
  const first = audio.resume();
  const second = audio.resume();
  assert.equal(context.resumes, 2, 'A later gesture must reach the native permission check.');
  assert.equal(await second, true);
  assert.equal(await first, true);
  audio.destroy();
});
