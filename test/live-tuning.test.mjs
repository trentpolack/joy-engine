import assert from 'node:assert/strict';
import test from 'node:test';
import { LiveTuning } from '../src/browser/tuning/live-tuning.ts';

const speed = {key: 'speed', label: 'Speed', description: 'Movement speed.', min: 0, max: 10, step: 0.1, units: 'm/s'};

test('live objects share validated history, reset, presets and explicit persistence', async () => {
  const first = {speed: 4, unrelated: {keep: true}};
  const second = {speed: 6};
  const saved = [];
  const tuning = new LiveTuning({save: async values => { saved.push(values); }});
  tuning.register('player', first, [speed]);
  tuning.register('enemy', second, [speed]);
  tuning.session.beginEdit();
  tuning.session.setValue('player.speed', 5);
  tuning.session.setValue('player.speed', 7);
  tuning.session.endEdit();
  assert.equal(first.speed, 7);
  tuning.session.undo();
  assert.equal(first.speed, 4);
  tuning.session.redo();
  assert.equal(first.speed, 7);
  tuning.applyPreset({'player.speed': 8, 'enemy.speed': 9});
  assert.deepEqual([first.speed, second.speed], [8, 9]);
  assert.throws(() => tuning.applyPreset({'player.speed': 3, 'enemy.speed': 100}), /maximum/);
  assert.deepEqual([first.speed, second.speed], [8, 9]);
  await tuning.session.save();
  assert.deepEqual(saved, [{'player.speed': 8, 'enemy.speed': 9}]);
  tuning.session.resetAll();
  assert.deepEqual([first.speed, second.speed], [4, 6]);
  assert.deepEqual(first.unrelated, {keep: true});
  tuning.destroy();
  assert.throws(() => tuning.session.setValue('player.speed', 2), /destroyed/);
});

test('registration rejects unsafe targets, duplicates and late additions; instances are independent', () => {
  const target = {speed: 3};
  const tuning = new LiveTuning();
  assert.throws(() => tuning.register('bad', Object.freeze({speed: 1}), [speed]), /writable/);
  tuning.register('player', target, [speed]);
  assert.throws(() => tuning.register('player', target, [speed]), /Duplicate/);
  assert.equal(tuning.session.canSave, false);
  assert.throws(() => tuning.register('late', target, [speed]), /before/);
  const other = new LiveTuning();
  other.register('other', {speed: 9}, [speed]);
  tuning.applyPreset({'player.speed': 2});
  assert.equal(other.session.values['other.speed'], 9);
  tuning.destroy();
  other.destroy();
});
