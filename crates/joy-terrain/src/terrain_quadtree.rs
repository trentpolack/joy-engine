// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Quadtree LOD selection over a surface's resident tiles, and the CPU reference of the morphed surface it draws.
//!
//! Every tile's fractional detail comes from its error table and the conservative distance to its bounds, exactly as a
//! version 1 leaf's does (`calculate_bounds_detail`). Detail is measured in global levels: detail `D` draws the
//! lattice at stride `2^floor(D)` finest samples and morphs toward the next coarser stride by `fract(D)`. A level-`L`
//! tile can draw `D` from `L` (its own lattice) to `L + log2(tile_cells)` (two triangles).
//!
//! Selection walks down from the roots. A tile whose detail falls below its own level subdivides when all four
//! children are resident; otherwise it draws at its finest and asks for the missing children. Children never draw
//! coarser than their parent's level, so the switch between a parent and its children is continuous: at the moment of
//! the switch both draw the parent's lattice. Roots keep their whole range, so a one-level pyramid selects exactly like
//! the version 1 fixed leaves.
//!
//! Neighbors differ by at most one level (a restricted quadtree): a coarse tile beside much finer ones subdivides, or,
//! when its children are not resident, the fine side merges back. Each edge then takes the coarser detail of the tiles
//! on both sides; a large tile's edge takes the coarsest of its two smaller neighbors, and they adopt its value. Both
//! sides of every edge therefore evaluate one function from one set of samples, and the corners where smaller tiles
//! meet a larger tile's edge are lattice points of every stride drawn there, so no cracks open.

use std::cmp::Reverse;
use std::collections::{
    HashMap,
    HashSet,
};

use joy_core::math::{
    DVec3,
    deterministic,
};

use crate::terrain_error::TerrainError;
use crate::terrain_layout::{
    TerrainLayout,
    TileKey,
};
use crate::terrain_selection::{
    TerrainView,
    calculate_bounds_detail,
    validate_terrain_view,
};
use crate::terrain_surface::TerrainSurface;

/// Tiles whose detail comes within this many levels of needing their children ask for them ahead of time.
pub const PREFETCH_DETAIL_MARGIN: f64 = 0.5;
/// Upper bound on balance passes; each pass fixes one level gap.
const MAXIMUM_BALANCE_PASSES: usize = 256;

/// Neighbor directions in edge order: -X, +X, -Z, +Z.
const DIRECTIONS: [(i64, i64); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];

/// One drawn tile: its key, its fractional detail, and the reconciled detail of each edge, all in global levels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainNode {
    /// The drawn tile.
    pub key: TileKey,
    /// Fractional detail in `[key.level, key.level + log2(tile_cells)]`.
    pub detail: f64,
    /// Edge details ordered -X, +X, -Z, +Z; each is at least `detail`.
    pub edges: [f64; 4],
}

impl TerrainNode {
    /// The tile-relative index range drawn: stride `2^lod` of the tile's own lattice.
    pub fn lod(&self) -> u32 {
        (self.detail.floor() as i32 - self.key.level).max(0) as u32
    }
}

/// A frame's selection: the tiles to draw and the tiles selection wanted but found missing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerrainNodeSelection {
    /// Drawn tiles, coarsest level first, then row-major.
    pub nodes: Vec<TerrainNode>,
    /// Missing tiles that would refine the view (children of tiles at or near their finest), coarsest first and then
    /// nearest to the eye.
    pub wanted: Vec<TileKey>,
}

/// Select the tiles that draw `surface` for `view` (in the terrain's local space).
///
/// Uses only resident tiles. Fails before any work for an invalid view.
pub fn select_terrain_nodes(
    surface: &TerrainSurface,
    view: &TerrainView,
) -> Result<TerrainNodeSelection, TerrainError> {
    validate_terrain_view(view)?;
    let mut selector = Selector {
        surface,
        view,
        candidates: Vec::new(),
        wanted: Vec::new(),
    };
    for root in surface.layout().roots() {
        if surface.tile(root).is_some() {
            selector.visit(root, f64::INFINITY);
        }
    }
    selector.balance();
    Ok(selector.finish())
}

/// The tiles of a selection drawn no finer than `floor` asks (a global level for each node and its index), with every
/// edge reconciled again, so the coarser surface is as crack-free as the selection.
///
/// For views that need less detail than the camera selected, such as shadow views drawing casters the camera cannot
/// see: the tiles (and so their resident data) stay the same; only their details and edges change. Each detail stays
/// within its tile's range, the same rules as selection apply beside smaller tiles, and `nodes` keeps its order.
pub fn coarsen_terrain_nodes(
    surface: &TerrainSurface,
    nodes: &[TerrainNode],
    floor: impl Fn(usize, &TerrainNode) -> f64,
) -> Vec<TerrainNode> {
    let tiles: Vec<(TileKey, f64)> =
        nodes.iter().enumerate().map(|(index, node)| (node.key, node.detail.max(floor(index, node)))).collect();
    reconcile(surface.layout(), &tiles)
}

/// Height of the morphed surface a node draws at its tile-lattice coordinate (`x`, `z`) in `[0, tile_cells]`.
///
/// The CPU reference of the terrain vertex shader, used for seam and parity checks. `None` when the node's tile is not
/// resident in `surface`.
pub fn sample_node_height(surface: &TerrainSurface, node: &TerrainNode, x: f64, z: f64) -> Option<f64> {
    let tile = surface.tile(node.key)?;
    let level = f64::from(node.key.level);
    let morph = NodeMorph {
        cells: f64::from(surface.layout().tile_cells),
        detail: node.detail - level,
        edges: node.edges.map(|edge| edge - level),
        height: |px: f64, pz: f64| f64::from(tile.height(px as i32, pz as i32)),
    };
    Some(morph.surface(x, z))
}

/// One tile considered for drawing: its uncapped detail and the cap its parent's level imposes.
#[derive(Clone, Copy, Debug)]
struct Candidate {
    key: TileKey,
    raw: f64,
    cap: f64,
}

/// What lies across one edge of a candidate.
enum Neighbor {
    /// The terrain's edge, or a region with nothing resident.
    Boundary,
    /// One tile of the same level.
    Same(usize),
    /// One coarser tile.
    Larger(usize),
    /// Finer tiles along the edge.
    Smaller(Vec<usize>),
}

/// Selection state for one frame.
struct Selector<'a> {
    surface: &'a TerrainSurface,
    view: &'a TerrainView,
    candidates: Vec<Candidate>,
    wanted: Vec<TileKey>,
}

impl Selector<'_> {
    /// Subdivide `key` while it needs finer detail and its children are resident.
    fn visit(&mut self, key: TileKey, cap: f64) {
        let raw = self.raw_detail(key);
        let level = f64::from(key.level);
        if key.level > self.surface.finest_level() {
            let children = key.children();
            let missing: Vec<TileKey> =
                children.iter().copied().filter(|child| self.surface.tile(*child).is_none()).collect();
            if raw < level + PREFETCH_DETAIL_MARGIN {
                self.wanted.extend(missing.iter().copied());
            }
            if raw.min(cap) < level && missing.is_empty() {
                for child in children {
                    self.visit(child, level);
                }
                return;
            }
        }
        self.candidates.push(Candidate {
            key,
            raw,
            cap,
        });
    }

    /// Uncapped detail of a resident tile.
    fn raw_detail(&self, key: TileKey) -> f64 {
        let layout = self.surface.layout();
        let tile = self.surface.tile(key).expect("selection only visits resident tiles");
        let (low, high) = layout.tile_footprint(key);
        let minimum = DVec3::new(low[0], tile.minimum(), low[1]);
        let maximum = DVec3::new(high[0], tile.maximum(), high[1]);
        calculate_bounds_detail(
            minimum,
            maximum,
            tile.errors(),
            tile.first_error_level(),
            layout.maximum_spacing(),
            self.view,
        )
    }

    /// Restrict neighboring levels to differ by at most one: split coarse tiles, or merge fine ones when the coarse
    /// tile's children are missing.
    ///
    /// Each pass fixes every gap it finds whose fix does not overlap another's (splits of distinct tiles, and merges
    /// into ancestors that neither contain each other nor a split tile), so the passes follow the depth of the level
    /// cascade rather than the number of gaps.
    fn balance(&mut self) {
        for _ in 0..MAXIMUM_BALANCE_PASSES {
            let index = self.index();
            let gaps = self.find_gaps(&index);
            if gaps.is_empty() {
                return;
            }

            // Decide every fix against this pass's candidates before changing them.
            let mut splits: Vec<usize> = Vec::new();
            let mut merges: Vec<TileKey> = Vec::new();
            for (coarse, fine) in gaps {
                let coarse_key = self.candidates[coarse].key;
                let missing: Vec<TileKey> =
                    coarse_key.children().iter().copied().filter(|child| self.surface.tile(*child).is_none()).collect();
                if missing.is_empty() {
                    let merging = merges.iter().any(|ancestor| covers(*ancestor, coarse_key));
                    if !splits.contains(&coarse) && !merging {
                        splits.push(coarse);
                    }
                    continue;
                }
                self.wanted.extend(missing);
                let ancestor = self.candidates[fine].key.ancestor(coarse_key.level - 1);
                let related = |other: &TileKey| covers(*other, ancestor) || covers(ancestor, *other);
                let touches_split = splits.iter().any(|split| covers(ancestor, self.candidates[*split].key));
                if !merges.iter().any(related) && !touches_split {
                    merges.push(ancestor);
                }
            }

            // Split: children are capped at the coarse tile's level, like any subdivision.
            let mut added = Vec::new();
            for &coarse in &splits {
                let coarse_key = self.candidates[coarse].key;
                for child in coarse_key.children() {
                    added.push(Candidate {
                        key: child,
                        raw: self.raw_detail(child),
                        cap: f64::from(coarse_key.level),
                    });
                }
            }
            let split_keys: Vec<TileKey> = splits.iter().map(|split| self.candidates[*split].key).collect();

            // Merge: draw the fine tiles' ancestor one level below the coarse tile instead.
            for &ancestor in &merges {
                added.push(Candidate {
                    key: ancestor,
                    raw: self.raw_detail(ancestor),
                    cap: f64::from(ancestor.level + 1),
                });
            }
            self.candidates.retain(|candidate| {
                let split = split_keys.contains(&candidate.key);
                let merged = merges.iter().any(|ancestor| covers(*ancestor, candidate.key));
                !split && !merged
            });
            self.candidates.extend(added);
        }
    }

    /// Every coarse candidate more than one level above a neighboring fine candidate, as `(coarse, fine)` indices, in
    /// candidate order.
    fn find_gaps(&self, index: &HashMap<TileKey, usize>) -> Vec<(usize, usize)> {
        let top = self.surface.layout().top_level();
        let mut gaps = Vec::new();
        for (fine, candidate) in self.candidates.iter().enumerate() {
            for direction in DIRECTIONS {
                let Some(beside) = self.offset(candidate.key, direction) else {
                    continue;
                };
                for level in (candidate.key.level + 2)..=top {
                    if let Some(&coarse) = index.get(&beside.ancestor(level)) {
                        gaps.push((coarse, fine));
                        break;
                    }
                }
            }
        }
        gaps
    }

    /// Final details and reconciled edges, coarsest level first, then row-major.
    fn finish(mut self) -> TerrainNodeSelection {
        self.candidates.sort_by_key(|candidate| (Reverse(candidate.key.level), candidate.key.z, candidate.key.x));
        let tiles: Vec<(TileKey, f64)> =
            self.candidates.iter().map(|candidate| (candidate.key, candidate.raw.min(candidate.cap))).collect();
        let nodes = reconcile(self.surface.layout(), &tiles);

        // Coarse tiles first (they unlock their children), then the nearest to the eye.
        let mut seen = HashSet::new();
        let mut wanted: Vec<TileKey> = self.wanted.into_iter().filter(|key| seen.insert(*key)).collect();
        let layout = self.surface.layout();
        let eye = self.view.eye;
        let distance = |key: &TileKey| {
            let (low, high) = layout.tile_footprint(*key);
            let (dx, dz) = ((low[0] + high[0]) * 0.5 - eye.x, (low[1] + high[1]) * 0.5 - eye.z);
            (dx * dx) + (dz * dz)
        };
        wanted.sort_by(|a, b| b.level.cmp(&a.level).then(distance(a).total_cmp(&distance(b))));
        TerrainNodeSelection {
            nodes,
            wanted,
        }
    }

    /// Candidate positions by key.
    fn index(&self) -> HashMap<TileKey, usize> {
        self.candidates.iter().enumerate().map(|(position, candidate)| (candidate.key, position)).collect()
    }

    /// The same-level tile one step in `direction`, when it lies inside the pyramid.
    fn offset(&self, key: TileKey, direction: (i64, i64)) -> Option<TileKey> {
        offset(self.surface.layout(), key, direction)
    }
}

/// Clamp each tile's detail into its range and reconcile every edge, keeping the tiles' order.
///
/// Details stay within the tile's range, and a tile beside smaller ones can draw no coarser than they can. Each edge
/// takes the coarsest detail along it, and a smaller tile adopts its larger neighbor's whole edge.
fn reconcile(layout: &TerrainLayout, tiles: &[(TileKey, f64)]) -> Vec<TerrainNode> {
    let index: HashMap<TileKey, usize> =
        tiles.iter().enumerate().map(|(position, (key, _))| (*key, position)).collect();
    let cells_log2 = f64::from(layout.cells_log2());
    let neighbors: Vec<[Neighbor; 4]> =
        tiles.iter().map(|(key, _)| DIRECTIONS.map(|direction| neighbor(layout, &index, *key, direction))).collect();

    let details: Vec<f64> = tiles
        .iter()
        .zip(&neighbors)
        .map(|((key, detail), around)| {
            let level = f64::from(key.level);
            let mut detail = detail.clamp(level, level + cells_log2);
            if around.iter().any(|neighbor| matches!(neighbor, Neighbor::Smaller(_))) {
                detail = detail.min((level - 1.0) + cells_log2).max(level);
            }
            detail
        })
        .collect();

    let own_edge = |node: usize, side: usize| match &neighbors[node][side] {
        Neighbor::Same(other) => details[node].max(details[*other]),
        Neighbor::Smaller(others) => others.iter().fold(details[node], |edge, other| edge.max(details[*other])),
        Neighbor::Boundary | Neighbor::Larger(_) => details[node],
    };
    (0..tiles.len())
        .map(|node| {
            let edges = [0, 1, 2, 3].map(|side| match &neighbors[node][side] {
                Neighbor::Larger(other) => own_edge(*other, side ^ 1),
                _ => own_edge(node, side),
            });
            TerrainNode {
                key: tiles[node].0,
                detail: details[node],
                edges,
            }
        })
        .collect()
}

/// The same-level tile one step in `direction`, when it lies inside the pyramid.
fn offset(layout: &TerrainLayout, key: TileKey, direction: (i64, i64)) -> Option<TileKey> {
    let tiles = layout.tiles_at(key.level);
    let x = i64::from(key.x) + direction.0;
    let z = i64::from(key.z) + direction.1;
    let inside = (0..tiles[0] as i64).contains(&x) && (0..tiles[1] as i64).contains(&z);
    inside.then(|| TileKey::new(key.level, x as u32, z as u32))
}

/// What lies across one edge of `key` among the indexed tiles.
fn neighbor(layout: &TerrainLayout, index: &HashMap<TileKey, usize>, key: TileKey, direction: (i64, i64)) -> Neighbor {
    let Some(beside) = offset(layout, key, direction) else {
        return Neighbor::Boundary;
    };
    if let Some(&same) = index.get(&beside) {
        return Neighbor::Same(same);
    }
    for level in (key.level + 1)..=layout.top_level() {
        if let Some(&larger) = index.get(&beside.ancestor(level)) {
            return Neighbor::Larger(larger);
        }
    }

    // The children of the same-level neighbor that touch the shared edge.
    let touching: Vec<usize> = beside
        .children()
        .iter()
        .filter(|child| match direction {
            (-1, _) => child.x % 2 == 1,
            (1, _) => child.x.is_multiple_of(2),
            (_, -1) => child.z % 2 == 1,
            _ => child.z.is_multiple_of(2),
        })
        .filter_map(|child| index.get(child).copied())
        .collect();
    if touching.is_empty() { Neighbor::Boundary } else { Neighbor::Smaller(touching) }
}

/// Whether `outer` is `inner` or one of its ancestors.
fn covers(outer: TileKey, inner: TileKey) -> bool {
    inner.level <= outer.level && inner.ancestor(outer.level) == outer
}

/// The morph a node's vertices follow, in tile-lattice coordinates, with details relative to the node's level.
struct NodeMorph<F: Fn(f64, f64) -> f64> {
    cells: f64,
    detail: f64,
    edges: [f64; 4],
    height: F,
}

impl<F: Fn(f64, f64) -> f64> NodeMorph<F> {
    /// The vertex shader's `terrainSurface` at one lattice coordinate.
    fn surface(&self, x: f64, z: f64) -> f64 {
        let mut detail = self.detail;
        if x == 0.0 {
            detail = self.edges[0];
        }
        if x == self.cells {
            detail = self.edges[1];
        }
        if z == 0.0 {
            detail = detail.max(self.edges[2]);
        }
        if z == self.cells {
            detail = detail.max(self.edges[3]);
        }
        let stride = deterministic::exp2(detail.floor());
        let morphed = self.morphed(x, z, detail);
        let interior = x > 0.0 && x < self.cells && z > 0.0 && z < self.cells;
        if !interior {
            return morphed;
        }
        let parent = self.parent_surface(x, z, self.cells.min(stride * 2.0));
        let own = (self.height)(x, z);
        own + ((parent - own) * (detail - detail.floor()))
    }

    /// Blend between a detail's lattice and the next coarser one.
    fn morphed(&self, x: f64, z: f64, detail: f64) -> f64 {
        let stride = deterministic::exp2(detail.floor());
        let fine = self.triangle(x, z, stride, &|px, pz| (self.height)(px, pz));
        let coarse = self.triangle(x, z, self.cells.min(stride * 2.0), &|px, pz| (self.height)(px, pz));
        fine + ((coarse - fine) * (detail - detail.floor()))
    }

    /// The stitched parent triangle surface that removed vertices converge on.
    fn parent_surface(&self, x: f64, z: f64, stride: f64) -> f64 {
        self.triangle(x, z, stride, &|px, pz| self.parent_vertex(px, pz, stride))
    }

    /// A parent-lattice corner, following the shared edge morph on the tile's boundary.
    fn parent_vertex(&self, x: f64, z: f64, stride: f64) -> f64 {
        let mut detail = deterministic::log2(stride);
        let mut boundary = false;
        for (on_edge, edge) in [(x == 0.0, 0), (x == self.cells, 1), (z == 0.0, 2), (z == self.cells, 3)] {
            if on_edge {
                detail = detail.max(self.edges[edge]);
                boundary = true;
            }
        }
        if boundary { self.morphed(x, z, detail) } else { (self.height)(x, z) }
    }

    /// The diagonal-split triangle surface of `corner` heights at `stride`, inside the tile.
    fn triangle(&self, x: f64, z: f64, stride: f64, corner: &dyn Fn(f64, f64) -> f64) -> f64 {
        let base_x = ((x / stride).floor() * stride).min(self.cells - stride);
        let base_z = ((z / stride).floor() * stride).min(self.cells - stride);
        let fraction_x = (x - base_x) / stride;
        let fraction_z = (z - base_z) / stride;
        let a = corner(base_x, base_z);
        let b = corner(base_x + stride, base_z);
        let c = corner(base_x, base_z + stride);
        let d = corner(base_x + stride, base_z + stride);
        if fraction_x >= fraction_z {
            a + ((b - a) * fraction_x) + ((d - b) * fraction_z)
        } else {
            a + ((d - c) * fraction_x) + ((c - a) * fraction_z)
        }
    }
}
