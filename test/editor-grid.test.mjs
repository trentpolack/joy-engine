// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import {EditorGrid} from '../src/rendering/editor-grid.ts';

test('editor grids retain bounded, finite unlit geometry across world scales and camera changes', () => {
  const grid = new EditorGrid();
  const view = {target: [0, 0, 0], backward: [0.4, 0.7, 0.6], distance: 26, viewportHeightPixels: 800};
  const original = structuredClone(view);
  const first = grid.frame(view);
  assert.equal(grid.frame(view), first, 'steady views reuse the owned frame');
  assert.deepEqual(view, original, 'borrowed camera input is unchanged');
  assert(first.triangles.length > 0);
  const bound = first.triangles.length;
  for(const distance of [0.01, 1, 10, 100, 1000, 100000]) {
    for(const backward of [[0, 1, 0], [1, 0, 0], [0, -1, 0], [0.3, -0.8, 0.5]]) {
      const frame = grid.frame({...view, target: [1000, 4, -1000], backward, distance, height: -120});
      assert(frame.triangles.length > 0 && frame.triangles.length <= bound, 'zoom cannot grow payload');
      assert.equal(frame.triangles.length%21, 0);
      assert(frame.triangles.every(Number.isFinite));
      const power = 10**Math.floor(Math.log10(frame.spacing));
      assert([1, 2, 5].some(value => Math.abs(frame.spacing/power - value) < 1e-10));
      for(let offset = 6; offset < frame.triangles.length; offset+= 7) {
        assert(frame.triangles[offset] > 0 && frame.triangles[offset] < 1, 'reference lines never become opaque surfaces');
      }
    }
  }
  const second = new EditorGrid();
  assert.deepEqual(second.frame(view), first, 'independent owners reproduce the same world-unit grid');
  assert.notEqual(second.frame(view).triangles, first.triangles);
});

test('invalid grid camera inputs fail before replacing the cached frame', () => {
  const grid = new EditorGrid();
  const view = {target: [0, 0, 0], backward: [0.4, 0.7, 0.6], distance: 26, viewportHeightPixels: 800};
  const first = grid.frame(view);
  for(const changed of [{distance: 0}, {viewportHeightPixels: -1}, {height: NaN}, {target: [0, Infinity, 0]}, {target: Array(3)}, {backward: [0, 0, 0]}, {distance: Number.MAX_VALUE}, {target: [1e100, 0, 0]}]) {
    assert.throws(() => grid.frame({...view, ...changed}), RangeError);
    assert.equal(grid.frame(view), first);
  }
});
