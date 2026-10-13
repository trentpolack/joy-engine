// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import test from 'node:test';
import assert from 'node:assert/strict';
import { searchGrid, traceGridRoute } from '../src/gameplay/grid-navigation.ts';

/**
 * Build orthogonal grid rules from rows of step costs; '#' is impassable.
 * Cells are row-major indices, matching how tactics maps usually key tiles.
 */
function createGrid(rows) {
  const height = rows.length;
  const width = rows[0].length;
  const costAt = index => rows[Math.floor(index/width)][index%width];
  return {
    width,
    neighbors(index) {
      const x = index%width;
      const z = Math.floor(index/width);
      const result = [];
      for(const [dx, dz] of [[1, 0], [-1, 0], [0, 1], [0, -1]]) {
        if((x + dx >= 0) && (x + dx < width) && (z + dz >= 0) && (z + dz < height)) {
          result.push(((z + dz)*width) + x + dx);
        }
      }
      return result;
    },
    stepCost(from, to) {
      const cost = costAt(to);
      return cost === '#' ? null : Number(cost);
    }
  };
}

test('weighted search respects budgets and walls, and traces the cheapest route', () => {
  const grid = createGrid([
    '1131',
    '1#11',
    '1111'
  ]);
  const routes = searchGrid({start: 0, budget: 4, neighbors: grid.neighbors, stepCost: grid.stepCost});

  // Entering the cost-3 cell spends the whole budget; the wall forces the long way around below.
  assert.equal(routes.get(2).cost, 4);
  assert.equal(routes.get(10).cost, 4);
  assert.equal(routes.has(5), false, 'walls are never entered');
  assert.equal(routes.has(3), false, 'cells beyond the inclusive budget are excluded');
  assert.deepEqual(traceGridRoute(routes, 10), [0, 4, 8, 9, 10]);
  assert.deepEqual(traceGridRoute(routes, 3), []);
  assert.deepEqual(traceGridRoute(routes, 0), [0]);

  // Unbounded floods reach the far corner through the cheapest detour.
  const flood = searchGrid({start: 0, budget: Infinity, neighbors: grid.neighbors, stepCost: grid.stepCost});
  assert.equal(flood.get(3).cost, 5);
});

test('equal-cost ties resolve by neighbor order, so repeated searches are identical', () => {
  const grid = createGrid([
    '111',
    '111',
    '111'
  ]);
  const first = searchGrid({start: 0, budget: Infinity, neighbors: grid.neighbors, stepCost: grid.stepCost});
  const second = searchGrid({start: 0, budget: Infinity, neighbors: grid.neighbors, stepCost: grid.stepCost});
  assert.deepEqual([...first.entries()], [...second.entries()]);

  // Neighbors list +X before +Z, so the first discovered route runs along X first.
  assert.deepEqual(traceGridRoute(first, 8), [0, 1, 2, 5, 8]);
  const costs = [...first.values()].map(route => route.cost);
  assert.deepEqual(costs, [...costs].sort((a, b) => a - b), 'results iterate in settle order');
});

test('invalid costs and unbounded implicit graphs fail instead of running away', () => {
  assert.throws(() => searchGrid({start: 0, budget: -1, neighbors: () => [], stepCost: () => 1}), RangeError);
  assert.throws(() => searchGrid({start: 0, budget: 5, neighbors: cell => [cell + 1], stepCost: () => -1}), RangeError);

  // An endless corridor must be bounded by the cell limit, not by caller luck.
  assert.throws(() => searchGrid({start: 0, budget: Infinity, neighbors: cell => [cell + 1], stepCost: () => 1, cellLimit: 500}),
    /exceeded 500 cells/);
  const corridor = searchGrid({start: 0, budget: 10, neighbors: cell => [cell + 1], stepCost: () => 1, cellLimit: 500});
  assert.equal(corridor.size, 11);
});
