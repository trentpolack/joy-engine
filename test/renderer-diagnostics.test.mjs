// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { DevTools } from '../src/browser/diagnostics/dev-tools.ts';
import { readGpuDiagnostics } from '../src/rendering/gpu/gpu-diagnostics.ts';

// These are the public counter fields exposed by installed luma 9.4.2. The
// manager is intentionally shared: opening one view cannot reset another's data.
test('renderer diagnostics preserve shared counters, sparse availability and snapshot ownership', () => {
  const memory = {stats: {'GPU Memory': {count: 2048}, 'Buffer Memory': {count: 0}}};
  const resources = {stats: {'Resources Active': {count: 2}, 'Resources Created': {count: 5}}};
  const statsManager = {
    stats: new Map([['GPU Time and Memory', memory], ['GPU Resource Counts', resources]]),
    getStats() { throw new Error('Diagnostics must not create or reset counters'); }
  };
  const firstDevice = {statsManager};
  const secondDevice = {statsManager};
  const first = readGpuDiagnostics(firstDevice, {memory: true, resources: true});
  assert.equal(first.scope, 'all-luma-devices');
  assert.deepEqual(first.memory, {allocatedBytes: 2048, bufferBytes: 0, textureBytes: null});
  assert.deepEqual(first.resources.Resources, {active: 2, created: 5});
  assert.deepEqual(first.resources.Textures, {active: null, created: null});
  assert.deepEqual(readGpuDiagnostics(secondDevice, {memory: true, resources: true}), first);

  first.memory.allocatedBytes = 99;
  first.resources.Resources.active = 99;
  resources.stats['Resources Active'].count = 1;
  const next = readGpuDiagnostics(secondDevice, {memory: true, resources: true});
  assert.equal(next.memory.allocatedBytes, 2048);
  assert.deepEqual(next.resources.Resources, {active: 1, created: 5});
  assert.equal(statsManager.stats.size, 2);
  assert.deepEqual(Object.keys(memory.stats), ['GPU Memory', 'Buffer Memory']);
});

test('unrequested diagnostic sections never touch their counter storage', () => {
  const reads = [];
  const device = {statsManager: {stats: {get(name) { reads.push(name); return undefined; }}}};
  assert.deepEqual(readGpuDiagnostics(device), {scope: 'all-luma-devices', memory: null, resources: null});
  assert.deepEqual(reads, []);
  assert.deepEqual(readGpuDiagnostics(device, {memory: true}).memory, {
    allocatedBytes: null, bufferBytes: null, textureBytes: null
  });
  assert.deepEqual(reads, ['GPU Time and Memory']);
  reads.length = 0;
  const sample = readGpuDiagnostics(device, {resources: true});
  assert.deepEqual(reads, ['GPU Resource Counts']);
  assert.equal(sample.memory, null);
  assert.deepEqual(sample.resources.Resources, {active: null, created: null});
});

test('disabled diagnostics need no browser globals and never call a supplied renderer', () => {
  const diagnostics = new DevTools({enabled: false});
  diagnostics.setRenderer({getDiagnostics() { throw new Error('Disabled diagnostics polled a renderer'); }});
  diagnostics.set('renderer', 'webgpu');
  diagnostics.frame(0);
  diagnostics.frame(1000);
  diagnostics.togglePanel();
  diagnostics.log('Must remain silent');
  diagnostics.destroy();
  diagnostics.destroy();
  assert.equal(diagnostics.view, null);
  assert.equal(diagnostics.rows.size, 0);
});
