// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Weighted grid and graph navigation shared by turn-based games.
 *
 * Cells are caller-defined keys compared with `Map` identity: tile indices,
 * interned strings, or stable record references. The engine never interprets
 * coordinates, terrain, factions, or occupancy; callers express those rules
 * through `neighbors` and `stepCost`. Searches are synchronous, allocate only
 * their result and a private frontier, and never retain the borrowed callbacks.
 */

// Upper bound on settled cells per search. Generous for authored tactics maps,
// and small enough that an unbounded implicit graph fails loudly instead of hanging.
const DEFAULT_CELL_LIMIT = 65536;

/** Cheapest known route into one cell. */
export interface GridRoute<Cell> {
  /** Accumulated step cost from the search origin, in caller-defined cost units. */
  cost: number;
  /** Cell this route entered from; null only for the origin. */
  previous: Cell | null;
}

/** Borrowed rules for one search. Callbacks must not mutate the grid during the search. */
export interface GridSearchOptions<Cell> {
  /** Search origin. Always reachable at zero cost. */
  start: Cell;
  /** Inclusive maximum accumulated cost. Use Infinity to flood the whole connected region. */
  budget: number;
  /** Adjacent candidates. Iteration order is the deterministic tie-break between equal-cost routes. */
  neighbors: (cell: Cell) => Iterable<Cell>;
  /** Finite nonnegative cost to step from `from` into `to`, or null when the step is not allowed. */
  stepCost: (from: Cell, to: Cell) => number | null;
  /** Maximum settled cells before the search throws; guards unbounded implicit graphs. */
  cellLimit?: number;
}

/** Frontier record; `order` keeps equal-cost expansion stable across runs. */
interface FrontierEntry<Cell> {
  cell: Cell;
  cost: number;
  order: number;
}

/**
 * Find every cell reachable within a cost budget, with its cheapest route.
 * Equal-cost alternatives keep the first route discovered, so identical inputs
 * always produce identical routes and identical `Map` iteration order
 * (cells appear in the order they were settled: nondecreasing cost).
 * @param options Borrowed search rules.
 * @returns Caller-owned map from each reachable cell to its cheapest route, origin included.
 * @throws RangeError for an invalid budget or step cost, or when the cell limit is exceeded.
 */
export function searchGrid<Cell>(options: GridSearchOptions<Cell>): Map<Cell, GridRoute<Cell>> {
  const {start, budget, neighbors, stepCost} = options;
  const cellLimit = options.cellLimit ?? DEFAULT_CELL_LIMIT;
  if(Number.isNaN(budget) || (budget < 0)) {
    throw new RangeError('Grid search budget must be zero, positive, or Infinity.');
  }
  if(!Number.isInteger(cellLimit) || (cellLimit < 1)) {
    throw new RangeError('Grid search cell limit must be a positive integer.');
  }

  // Best known routes, including tentative frontier entries.
  const known = new Map<Cell, GridRoute<Cell>>([[start, {cost: 0, previous: null}]]);
  // Settled routes in settle order; this becomes the result.
  const settled = new Map<Cell, GridRoute<Cell>>();
  const frontier = new FrontierHeap<Cell>();
  let order = 0;
  frontier.push({cell: start, cost: 0, order: order++});

  while(frontier.size > 0) {
    const entry = frontier.pop();

    // Lazy deletion: an improved route superseded this entry after it was queued.
    if(settled.has(entry.cell) || (known.get(entry.cell)?.cost !== entry.cost)) {
      continue;
    }

    const route = known.get(entry.cell)!;
    settled.set(entry.cell, route);
    if(settled.size > cellLimit) {
      throw new RangeError(`Grid search exceeded ${cellLimit} cells; bound the neighbors or budget.`);
    }

    for(const next of neighbors(entry.cell)) {
      if(settled.has(next)) {
        continue;
      }

      const step = stepCost(entry.cell, next);
      if(step === null) {
        continue;
      }
      if(!Number.isFinite(step) || (step < 0)) {
        throw new RangeError('Grid step cost must be a finite nonnegative number or null.');
      }

      const cost = entry.cost + step;
      if(cost > budget) {
        continue;
      }

      // Strictly cheaper only: equal-cost alternatives keep the first discovered route.
      const existing = known.get(next);
      if(existing && (existing.cost <= cost)) {
        continue;
      }
      known.set(next, {cost, previous: entry.cell});
      frontier.push({cell: next, cost, order: order++});
    }
  }

  return settled;
}

/**
 * Rebuild the cheapest route to a target from a `searchGrid` result.
 * @param routes Borrowed search result; not mutated.
 * @param target Destination cell.
 * @returns New array from origin to target inclusive, or an empty array when unreachable.
 */
export function traceGridRoute<Cell>(routes: ReadonlyMap<Cell, GridRoute<Cell>>, target: Cell): Cell[] {
  if(!routes.has(target)) {
    return [];
  }

  const path: Cell[] = [];
  let cursor: Cell | null = target;
  while(cursor !== null) {
    path.push(cursor);

    // A result can only reference cells it contains; guard against foreign maps anyway.
    if(path.length > routes.size) {
      throw new RangeError('Grid route contains a cycle; pass an unmodified searchGrid result.');
    }
    cursor = routes.get(cursor)?.previous ?? null;
  }

  return path.reverse();
}

/** Binary min-heap ordered by cost, then by insertion order for stable ties. */
class FrontierHeap<Cell> {
  private readonly entries: FrontierEntry<Cell>[] = [];

  get size(): number {
    return this.entries.length;
  }

  push(entry: FrontierEntry<Cell>): void {
    const entries = this.entries;
    entries.push(entry);

    let index = entries.length - 1;
    while(index > 0) {
      const parent = (index - 1) >> 1;
      if(!precedes(entries[index], entries[parent])) {
        break;
      }
      [entries[index], entries[parent]] = [entries[parent], entries[index]];
      index = parent;
    }
  }

  /** Remove the cheapest entry; callers check `size` first. */
  pop(): FrontierEntry<Cell> {
    const entries = this.entries;
    const top = entries[0];
    const last = entries.pop()!;
    if(entries.length === 0) {
      return top;
    }

    entries[0] = last;
    let index = 0;
    while(true) {
      const left = (index*2) + 1;
      const right = left + 1;
      let smallest = index;
      if((left < entries.length) && precedes(entries[left], entries[smallest])) {
        smallest = left;
      }
      if((right < entries.length) && precedes(entries[right], entries[smallest])) {
        smallest = right;
      }
      if(smallest === index) {
        break;
      }
      [entries[index], entries[smallest]] = [entries[smallest], entries[index]];
      index = smallest;
    }

    return top;
  }
}

function precedes<Cell>(a: FrontierEntry<Cell>, b: FrontierEntry<Cell>): boolean {
  return (a.cost < b.cost) || ((a.cost === b.cost) && (a.order < b.order));
}
