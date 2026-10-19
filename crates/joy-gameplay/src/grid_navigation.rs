// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Weighted grid and graph navigation shared by turn-based games.
//!
//! Cells are caller-defined keys compared by equality and hash: tile indices, interned strings, or stable ids. The
//! engine never interprets coordinates, terrain, factions, or occupancy; callers express those rules through the
//! `neighbors` and `step_cost` callbacks. Searches are synchronous, allocate only their result and a private frontier,
//! and never retain the borrowed callbacks. Tie-breaking and settle order are fixed, so the same inputs always produce
//! the same results.

use std::hash::Hash;

use indexmap::IndexMap;

/// Default upper bound on settled cells per search. Generous for authored tactics maps, and small enough that an
/// unbounded implicit graph fails loudly instead of hanging.
pub const DEFAULT_CELL_LIMIT: usize = 65_536;

/// Cheapest known route into one cell.
#[derive(Clone, Debug, PartialEq)]
pub struct GridRoute<Cell> {
    /// Accumulated step cost from the search origin, in caller-defined cost units.
    pub cost: f64,
    /// Cell this route entered from; `None` only for the origin.
    pub previous: Option<Cell>,
}

/// Search result: every reachable cell with its cheapest route, in settle order (nondecreasing cost), origin first.
pub type GridRoutes<Cell> = IndexMap<Cell, GridRoute<Cell>>;

/// Rules for one search.
pub struct GridSearchOptions<Cell, Neighbors, StepCost> {
    /// Search origin; always reachable at zero cost.
    pub start: Cell,
    /// Inclusive maximum accumulated cost; `f64::INFINITY` floods the whole connected region.
    pub budget: f64,
    /// Adjacent candidates of a cell. Iteration order is the deterministic tie-break between equal-cost routes.
    pub neighbors: Neighbors,
    /// Finite nonnegative cost to step from the first cell into the second, or `None` when the step is not allowed.
    pub step_cost: StepCost,
    /// Maximum settled cells before the search fails; guards unbounded implicit graphs.
    pub cell_limit: usize,
}

/// Navigation failure.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GridSearchError {
    /// The budget was NaN or negative.
    #[error("Grid search budget must be zero, positive, or Infinity.")]
    InvalidBudget,
    /// The cell limit was zero.
    #[error("Grid search cell limit must be a positive integer.")]
    InvalidCellLimit,
    /// A step cost was negative or non-finite.
    #[error("Grid step cost must be a finite nonnegative number or null.")]
    InvalidStepCost,
    /// More cells settled than the limit allows.
    #[error("Grid search exceeded {0} cells; bound the neighbors or budget.")]
    CellLimitExceeded(usize),
    /// A route referenced itself; the routes were not an unmodified search result.
    #[error("Grid route contains a cycle; pass an unmodified search_grid result.")]
    RouteCycle,
}

/// Find every cell reachable within a cost budget, with its cheapest route.
///
/// Equal-cost alternatives keep the first route discovered, so identical inputs always produce identical routes and
/// identical result order (cells appear in the order they were settled: nondecreasing cost). Fails for an invalid
/// budget, cell limit, or step cost, or when more than `cell_limit` cells settle.
pub fn search_grid<Cell, Neighbors, Candidates, StepCost>(
    options: GridSearchOptions<Cell, Neighbors, StepCost>,
) -> Result<GridRoutes<Cell>, GridSearchError>
where
    Cell: Clone + Eq + Hash,
    Neighbors: FnMut(&Cell) -> Candidates,
    Candidates: IntoIterator<Item = Cell>,
    StepCost: FnMut(&Cell, &Cell) -> Option<f64>,
{
    let GridSearchOptions { start, budget, mut neighbors, mut step_cost, cell_limit } = options;
    if budget.is_nan() || budget < 0.0 {
        return Err(GridSearchError::InvalidBudget);
    }
    if cell_limit < 1 {
        return Err(GridSearchError::InvalidCellLimit);
    }

    // Best known routes, including tentative frontier entries.
    let mut known: IndexMap<Cell, GridRoute<Cell>> = IndexMap::new();
    known.insert(start.clone(), GridRoute { cost: 0.0, previous: None });
    // Settled routes in settle order; this becomes the result.
    let mut settled: GridRoutes<Cell> = IndexMap::new();
    let mut frontier = FrontierHeap::default();
    let mut order: u64 = 0;
    frontier.push(FrontierEntry { cell: start, cost: 0.0, order });
    order += 1;

    while let Some(entry) = frontier.pop() {
        // Lazy deletion: an improved route superseded this entry after it was queued.
        let current = known.get(&entry.cell).map(|route| route.cost);
        if settled.contains_key(&entry.cell) || current != Some(entry.cost) {
            continue;
        }

        let Some(route) = known.get(&entry.cell).cloned() else {
            continue;
        };
        settled.insert(entry.cell.clone(), route);
        if settled.len() > cell_limit {
            return Err(GridSearchError::CellLimitExceeded(cell_limit));
        }

        for next in neighbors(&entry.cell) {
            if settled.contains_key(&next) {
                continue;
            }

            let Some(step) = step_cost(&entry.cell, &next) else {
                continue;
            };
            if !step.is_finite() || step < 0.0 {
                return Err(GridSearchError::InvalidStepCost);
            }

            let cost = entry.cost + step;
            if cost > budget {
                continue;
            }

            // Strictly cheaper only: equal-cost alternatives keep the first discovered route.
            if known.get(&next).is_some_and(|existing| existing.cost <= cost) {
                continue;
            }
            known.insert(next.clone(), GridRoute { cost, previous: Some(entry.cell.clone()) });
            frontier.push(FrontierEntry { cell: next, cost, order });
            order += 1;
        }
    }

    Ok(settled)
}

/// Rebuild the cheapest route to a target from a [`search_grid`] result.
///
/// Returns a new list from origin to target inclusive, or an empty list when the target is unreachable. Fails if the
/// routes contain a cycle (they were not an unmodified search result).
pub fn trace_grid_route<Cell>(routes: &GridRoutes<Cell>, target: &Cell) -> Result<Vec<Cell>, GridSearchError>
where
    Cell: Clone + Eq + Hash,
{
    if !routes.contains_key(target) {
        return Ok(Vec::new());
    }

    let mut path = Vec::new();
    let mut cursor = Some(target.clone());
    while let Some(cell) = cursor {
        // A result can only reference cells it contains; guard against foreign maps anyway.
        if path.len() >= routes.len() {
            return Err(GridSearchError::RouteCycle);
        }
        cursor = routes.get(&cell).and_then(|route| route.previous.clone());
        path.push(cell);
    }

    path.reverse();
    Ok(path)
}

/// Frontier record; `order` keeps equal-cost expansion stable across runs.
struct FrontierEntry<Cell> {
    cell: Cell,
    cost: f64,
    order: u64,
}

/// Binary min-heap ordered by cost, then insertion order, so equal-cost entries pop in a deterministic order.
struct FrontierHeap<Cell> {
    entries: Vec<FrontierEntry<Cell>>,
}

impl<Cell> Default for FrontierHeap<Cell> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<Cell> FrontierHeap<Cell> {
    /// Insert an entry and sift it up.
    fn push(&mut self, entry: FrontierEntry<Cell>) {
        self.entries.push(entry);
        let mut index = self.entries.len() - 1;
        while index > 0 {
            let parent = (index - 1) >> 1;
            if !precedes(&self.entries[index], &self.entries[parent]) {
                break;
            }
            self.entries.swap(index, parent);
            index = parent;
        }
    }

    /// Remove the cheapest entry, or `None` when empty.
    fn pop(&mut self) -> Option<FrontierEntry<Cell>> {
        if self.entries.is_empty() {
            return None;
        }
        let last = self.entries.len() - 1;
        self.entries.swap(0, last);
        let top = self.entries.pop();

        let mut index = 0;
        loop {
            let left = (index * 2) + 1;
            let right = left + 1;
            let mut smallest = index;
            if left < self.entries.len() && precedes(&self.entries[left], &self.entries[smallest]) {
                smallest = left;
            }
            if right < self.entries.len() && precedes(&self.entries[right], &self.entries[smallest]) {
                smallest = right;
            }
            if smallest == index {
                break;
            }
            self.entries.swap(index, smallest);
            index = smallest;
        }
        top
    }
}

/// Whether `a` pops before `b`: cheaper first, then earlier insertion.
fn precedes<Cell>(a: &FrontierEntry<Cell>, b: &FrontierEntry<Cell>) -> bool {
    (a.cost < b.cost) || ((a.cost == b.cost) && (a.order < b.order))
}
