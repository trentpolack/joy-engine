// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { createConfigDocumentValues, createConfigSchema, createConfigValues } from 'joy-engine';
import { ConfigSession } from 'joy-engine/development';

const fields = [
  { key: 'speed', label: 'Speed', group: 'Movement', description: 'Movement speed.', defaultValue: 4, min: 1, max: 10, step: 0.5, units: 'm/s', applyMode: 'live' },
  { key: 'count', label: 'Count', group: 'Spawning', description: 'Spawn count.', defaultValue: 3, min: 0, max: 8, step: 1, units: '', applyMode: 'next-spawn' },
];

test('config values validate metadata and return complete independent records', () => {
  assert.deepEqual(createConfigValues(fields, { speed: 6 }), { speed: 6, count: 3 });
  assert.throws(() => createConfigValues(fields, { missing: 2 }), /unknown.*missing/i);
  assert.throws(() => createConfigValues(fields, { speed: Number.NaN }), /finite/i);
  assert.throws(() => createConfigValues(fields, { speed: 11 }), /speed.*maximum/i);
  assert.throws(() => createConfigValues(fields, { count: 1.5 }), /count.*integer/i);
  assert.throws(() => createConfigValues([{ ...fields[0], min: Infinity }]), /finite/i);
  assert.throws(() => createConfigValues([{ ...fields[0], applyMode: 'later' }]), /applyMode/i);
});

test('config schema accepts strict versioned partial overrides', () => {
  assert.deepEqual(createConfigSchema(fields), {
    $schema: 'https://json-schema.org/draft/2020-12/schema',
    type: 'object',
    additionalProperties: false,
    required: ['version', 'values'],
    properties: {
      $schema: { type: 'string' },
      version: { const: 1 },
      values: {
        type: 'object',
        additionalProperties: false,
        properties: {
          speed: { type: 'number', minimum: 1, maximum: 10, description: 'Movement speed.' },
          count: { type: 'integer', minimum: 0, maximum: 8, description: 'Spawn count.' },
        },
      },
    },
  });
});

test('config documents share strict boundary validation and produce complete values', () => {
  assert.deepEqual(createConfigDocumentValues(fields, {
    $schema: './tuning-overrides.schema.json', version: 1, values: { speed: 8 },
  }), { speed: 8, count: 3 });
  assert.throws(() => createConfigDocumentValues(fields, { version: 2, values: {} }), /version must be 1/i);
  assert.throws(() => createConfigDocumentValues(fields, { version: 1, values: {}, extra: true }), /unknown.*extra/i);
  assert.throws(() => createConfigDocumentValues(fields, { version: 1, values: [] }), /values must be an object/i);
  assert.throws(() => createConfigDocumentValues(fields, { version: 1, values: {}, $schema: 2 }), /schema must be a string/i);
});

test('sessions are isolated, expose copies, and reject invalid edits atomically', () => {
  const changes = [];
  const first = new ConfigSession({ fields, values: { speed: 5 }, onChange: values => changes.push(values) });
  const second = new ConfigSession({ fields, values: { speed: 5 } });
  const borrowed = first.values;
  borrowed.speed = 9;
  assert.equal(first.values.speed, 5);

  first.setValue('speed', 7);
  assert.deepEqual(first.values, { speed: 7, count: 3 });
  assert.equal(second.values.speed, 5);
  assert.deepEqual(changes, [{ speed: 7, count: 3 }]);
  changes[0].speed = 10;
  assert.equal(first.values.speed, 7);
  assert.throws(() => first.setValue('count', 2.5), /integer/i);
  assert.throws(() => first.setValue('unknown', 2), /unknown/i);
  assert.deepEqual(first.values, { speed: 7, count: 3 });
});

test('undo, revert, and resets preserve clear saved/default semantics', () => {
  const session = new ConfigSession({ fields, values: { speed: 5, count: 4 } });
  session.setValue('speed', 6);
  session.setValue('count', 7);
  assert.equal(session.canUndo, true);
  session.undo();
  assert.deepEqual(session.values, { speed: 6, count: 4 });
  session.resetField('speed');
  assert.equal(session.values.speed, 4);
  session.undo();
  assert.equal(session.values.speed, 6);
  session.resetAll();
  assert.deepEqual(session.values, { speed: 4, count: 3 });
  session.revert();
  assert.deepEqual(session.values, { speed: 5, count: 4 });
  assert.equal(session.dirty, false);
});

test('save snapshots before awaiting and later edits remain dirty', async () => {
  let resolveSave;
  const saved = [];
  const session = new ConfigSession({
    fields,
    values: { speed: 5 },
    save: values => new Promise(resolve => {
      saved.push(values);
      resolveSave = resolve;
    }),
  });
  session.setValue('speed', 6);
  const pending = session.save();
  session.setValue('speed', 7);
  resolveSave();
  await pending;
  assert.deepEqual(saved, [{ speed: 6, count: 3 }]);
  assert.equal(session.dirty, true);
  session.revert();
  assert.equal(session.values.speed, 6);
});

test('older save completion cannot replace a newer successful baseline', async () => {
  const completions = [];
  const session = new ConfigSession({
    fields,
    values: { speed: 5 },
    save: () => new Promise(resolve => {
      completions.push(resolve);
    }),
  });
  session.setValue('speed', 6);
  const olderSave = session.save();
  session.setValue('speed', 7);
  const newerSave = session.save();

  completions[1]();
  await newerSave;
  completions[0]();
  await olderSave;

  assert.equal(session.dirty, false);
  session.setValue('speed', 8);
  session.revert();
  assert.equal(session.values.speed, 7);
});

test('runtime callback failure leaves values and history unchanged', () => {
  const error = new Error('runtime rejected value');
  const session = new ConfigSession({
    fields,
    values: { speed: 5 },
    onChange: values => {
      if(values.speed === 7) {
        throw error;
      }
    },
  });
  assert.throws(() => session.setValue('speed', 7), error);
  assert.deepEqual(session.values, { speed: 5, count: 3 });
  assert.equal(session.canUndo, false);

  session.setValue('speed', 6);
  assert.equal(session.canUndo, true);
  session.undo();
  assert.equal(session.values.speed, 5);
  assert.equal(session.canUndo, false);
});

test('last undo notifies subscribers after canUndo becomes false', () => {
  const session = new ConfigSession({ fields, values: {} });
  session.setValue('speed', 5);
  const observedCanUndo = [];
  session.subscribe(() => {
    observedCanUndo.push(session.canUndo);
  });
  session.undo();
  assert.deepEqual(observedCanUndo, [false]);
});

test('failed saves retain edits and subscriptions stop after unsubscribe or destroy', async () => {
  const error = new Error('disk unavailable');
  let notifications = 0;
  const session = new ConfigSession({ fields, values: {}, save: async () => {
    throw error;
  } });
  const unsubscribe = session.subscribe(() => {
    notifications++;
  });
  session.setValue('speed', 5);
  await assert.rejects(session.save(), error);
  assert.equal(session.values.speed, 5);
  assert.equal(session.dirty, true);
  unsubscribe();
  session.setValue('speed', 6);
  assert.equal(notifications, 3);
  session.subscribe(() => {
    notifications++;
  });
  session.destroy();
  assert.throws(() => session.setValue('speed', 7), /destroyed/i);
  assert.equal(notifications, 3);
});

test('continuous edits undo and redo as one accepted transaction; new edits discard redo', () => {
  const session = new ConfigSession({fields});
  session.beginEdit();
  session.setValue('speed', 5);
  session.setValue('speed', 6);
  session.setValue('speed', 7);
  session.endEdit();
  session.undo();
  assert.equal(session.values.speed, 4);
  assert.equal(session.canUndo, false);
  assert.equal(session.canRedo, true);
  session.redo();
  assert.equal(session.values.speed, 7);
  session.undo();
  session.setValue('count', 6);
  assert.equal(session.canRedo, false);
});

test('preset validation and rejected redo preserve values and both history stacks', () => {
  let reject = false;
  const session = new ConfigSession({fields, onChange: () => {
    if(reject) throw new Error('rejected');
  }});
  session.setValues({speed: 8, count: 4});
  session.undo();
  reject = true;
  assert.throws(() => session.redo(), /rejected/);
  assert.equal(session.values.speed, 4);
  assert.equal(session.canRedo, true);
  assert.equal(session.canUndo, false);
  reject = false;
  assert.throws(() => session.setValues({speed: 8, count: 100}), /maximum/);
  assert.equal(session.canRedo, true);
  session.redo();
  assert.deepEqual(session.values, {speed: 8, count: 4});
});
