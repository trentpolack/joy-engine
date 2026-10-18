// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Grid navigation parity with results recorded from the TypeScript `searchGrid` and `traceGridRoute`.

use joy_gameplay::{
    DEFAULT_CELL_LIMIT,
    GridRoutes,
    GridSearchError,
    GridSearchOptions,
    search_grid,
    trace_grid_route,
};

/// Orthogonal grid rules from rows of step costs; `#` is impassable. Cells are row-major indices, and neighbors are
/// listed +X, -X, +Z, -Z exactly like the TypeScript test helper.
struct Grid {
    rows: Vec<Vec<char>>,
}

impl Grid {
    fn new(rows: &[&str]) -> Self {
        Self { rows: rows.iter().map(|row| row.chars().collect()).collect() }
    }

    fn width(&self) -> usize {
        self.rows[0].len()
    }

    fn neighbors(&self, index: usize) -> Vec<usize> {
        let (width, height) = (self.width() as i64, self.rows.len() as i64);
        let (x, z) = ((index as i64) % width, (index as i64) / width);
        [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .filter(|(dx, dz)| x + dx >= 0 && x + dx < width && z + dz >= 0 && z + dz < height)
            .map(|(dx, dz)| (((z + dz) * width) + x + dx) as usize)
            .collect()
    }

    fn step_cost(&self, to: usize) -> Option<f64> {
        let cost = self.rows[to / self.width()][to % self.width()];
        cost.to_digit(10).map(f64::from)
    }

    fn search(&self, start: usize, budget: f64) -> GridRoutes<usize> {
        search_grid(GridSearchOptions {
            start,
            budget,
            neighbors: |cell: &usize| self.neighbors(*cell),
            step_cost: |_: &usize, to: &usize| self.step_cost(*to),
            cell_limit: DEFAULT_CELL_LIMIT,
        })
        .unwrap()
    }
}

/// Flatten a result into `(cell, cost, previous)` in settle order.
fn dump(routes: &GridRoutes<usize>) -> Vec<(usize, f64, Option<usize>)> {
    routes.iter().map(|(cell, route)| (*cell, route.cost, route.previous)).collect()
}

#[test]
fn weighted_searches_match_typescript_results() {
    let weighted = Grid::new(&["1131", "1#11", "1111"]);
    let budget_four = weighted.search(0, 4.0);
    assert_eq!(
        dump(&budget_four),
        vec![
            (0, 0.0, None),
            (1, 1.0, Some(0)),
            (4, 1.0, Some(0)),
            (8, 2.0, Some(4)),
            (9, 3.0, Some(8)),
            (2, 4.0, Some(1)),
            (10, 4.0, Some(9)),
        ]
    );
    assert_eq!(trace_grid_route(&budget_four, &10).unwrap(), vec![0, 4, 8, 9, 10]);
    assert_eq!(trace_grid_route(&budget_four, &3).unwrap(), Vec::<usize>::new());
    assert_eq!(trace_grid_route(&budget_four, &0).unwrap(), vec![0]);

    let flood = weighted.search(0, f64::INFINITY);
    assert_eq!(
        dump(&flood),
        vec![
            (0, 0.0, None),
            (1, 1.0, Some(0)),
            (4, 1.0, Some(0)),
            (8, 2.0, Some(4)),
            (9, 3.0, Some(8)),
            (2, 4.0, Some(1)),
            (10, 4.0, Some(9)),
            (3, 5.0, Some(2)),
            (6, 5.0, Some(2)),
            (11, 5.0, Some(10)),
            (7, 6.0, Some(3)),
        ]
    );

    let terrain = Grid::new(&["12131", "2#2#1", "11912", "3#111"]);
    let routes = terrain.search(7, 9.0);
    assert_eq!(
        dump(&routes),
        vec![
            (7, 0.0, None),
            (2, 1.0, Some(7)),
            (1, 3.0, Some(2)),
            (3, 4.0, Some(2)),
            (0, 4.0, Some(1)),
            (4, 5.0, Some(3)),
            (5, 6.0, Some(0)),
            (9, 6.0, Some(4)),
            (10, 7.0, Some(5)),
            (14, 8.0, Some(9)),
            (11, 8.0, Some(10)),
            (12, 9.0, Some(7)),
            (13, 9.0, Some(14)),
            (19, 9.0, Some(14)),
        ]
    );
    assert_eq!(trace_grid_route(&routes, &19).unwrap(), vec![7, 2, 3, 4, 9, 14, 19]);
    assert_eq!(trace_grid_route(&routes, &0).unwrap(), vec![7, 2, 1, 0]);
}

#[test]
fn equal_cost_ties_resolve_by_neighbor_order() {
    let uniform = Grid::new(&["111", "111", "111"]);
    let first = uniform.search(0, f64::INFINITY);
    assert_eq!(first, uniform.search(0, f64::INFINITY));
    assert_eq!(
        dump(&first),
        vec![
            (0, 0.0, None),
            (1, 1.0, Some(0)),
            (3, 1.0, Some(0)),
            (2, 2.0, Some(1)),
            (4, 2.0, Some(1)),
            (6, 2.0, Some(3)),
            (5, 3.0, Some(2)),
            (7, 3.0, Some(4)),
            (8, 4.0, Some(5)),
        ]
    );
    assert_eq!(trace_grid_route(&first, &8).unwrap(), vec![0, 1, 2, 5, 8]);
}

#[test]
fn invalid_costs_and_unbounded_graphs_fail_instead_of_running_away() {
    let options = |budget: f64, cost: f64, cell_limit: usize| GridSearchOptions {
        start: 0_u64,
        budget,
        neighbors: |cell: &u64| [cell + 1],
        step_cost: move |_: &u64, _: &u64| Some(cost),
        cell_limit,
    };
    assert_eq!(search_grid(options(-1.0, 1.0, 10)).unwrap_err(), GridSearchError::InvalidBudget);
    assert_eq!(search_grid(options(f64::NAN, 1.0, 10)).unwrap_err(), GridSearchError::InvalidBudget);
    assert_eq!(search_grid(options(5.0, -1.0, 10)).unwrap_err(), GridSearchError::InvalidStepCost);
    assert_eq!(search_grid(options(5.0, 1.0, 0)).unwrap_err(), GridSearchError::InvalidCellLimit);

    // An endless corridor is bounded by the cell limit, not by caller luck.
    let error = search_grid(options(f64::INFINITY, 1.0, 500)).unwrap_err();
    assert_eq!(error, GridSearchError::CellLimitExceeded(500));
    assert!(error.to_string().contains("exceeded 500 cells"));
    assert_eq!(search_grid(options(10.0, 1.0, 500)).unwrap().len(), 11);
}
