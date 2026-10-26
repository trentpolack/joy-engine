// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`TerrainSurface`]: an immutable snapshot of the resident tiles of one terrain, shared by the renderer, colliders,
//! and gameplay.
//!
//! A surface is what everything downstream reads. The renderer selects and draws its tiles, terrain colliders build
//! heightfields from it, and gameplay samples it. Snapshots are cheap to clone and never change; a streamer publishes a
//! new snapshot whenever tiles arrive, leave, or are edited, keeping the same [`TerrainSurface::lineage`] so consumers
//! can keep per-tile caches keyed by tile and revision. A fully resident surface (for example one built from a version 1
//! asset) is simply a snapshot that never changes.
//!
//! Gameplay sampling reads only the collision level ([`TerrainSurface::height_at`]), never camera-selected detail, so
//! the result depends on which tiles are resident but never on the view. Positions are local X/Z world units.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use joy_core::math::{
    DVec3,
    deterministic,
};

use crate::terrain_asset::{
    TerrainAsset,
    TerrainBounds,
};
use crate::terrain_detail::TerrainMaterialRules;
use crate::terrain_error::TerrainError;
use crate::terrain_layout::{
    TerrainLayout,
    TileKey,
};
use crate::terrain_pyramid::build_pyramid_from_asset;
use crate::terrain_tile::TerrainTile;

/// Bisection steps that refine a ray hit after marching finds a crossing.
const RAY_REFINEMENT_STEPS: usize = 32;
/// Ray marching steps per lattice cell of the tile under the ray.
const RAY_STEPS_PER_CELL: f64 = 2.0;
/// Most marching steps one ray may take.
const MAXIMUM_RAY_STEPS: usize = 1 << 20;

/// Process-unique lineage identities for surfaces and the streams that publish them.
static NEXT_LINEAGE: AtomicU64 = AtomicU64::new(1);

/// The first hit of a ray on a surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainRayHit {
    /// Local hit position in world units.
    pub point: DVec3,
    /// Distance along the normalized ray direction.
    pub distance: f64,
}

/// An immutable snapshot of a terrain's resident tiles. Cloning shares everything.
#[derive(Clone, Debug)]
pub struct TerrainSurface {
    inner: Arc<SurfaceInner>,
}

#[derive(Debug)]
struct SurfaceInner {
    lineage: u64,
    layout: TerrainLayout,
    finest_level: i32,
    collision_level: i32,
    materials: TerrainMaterialRules,
    tiles: HashMap<TileKey, Arc<TerrainTile>>,
    bounds: TerrainBounds,
}

impl TerrainSurface {
    /// A fully resident one-level surface from a version 1 asset. Its leaves are the root tiles, so LOD selection and
    /// gameplay sampling match the asset's own functions.
    pub fn from_asset(asset: &TerrainAsset) -> Self {
        let (layout, tiles) =
            build_pyramid_from_asset(asset, 1).expect("a validated asset always forms a one-level pyramid");
        Self::from_tiles(layout, tiles, TerrainMaterialRules::default())
            .expect("a pyramid built from a validated asset is complete")
    }

    /// A fully resident surface from stored tiles (levels zero and up) of `layout`.
    ///
    /// Every root tile must be present; descendants are optional. Gameplay sampling reads level 0.
    pub fn from_tiles(
        layout: TerrainLayout,
        tiles: impl IntoIterator<Item = TerrainTile>,
        materials: TerrainMaterialRules,
    ) -> Result<Self, TerrainError> {
        layout.validate()?;
        materials.validate()?;
        let mut resident = HashMap::new();
        for tile in tiles {
            if tile.key().level < 0 || !layout.contains(tile.key(), 0) {
                return Err(TerrainError::InvalidTile("a stored tile lies outside the layout"));
            }
            resident.insert(tile.key(), Arc::new(tile));
        }
        Self::assemble(next_lineage(), layout, 0, 0, materials, resident, 0.0)
    }

    /// Build a snapshot from resolved tiles; every root must be resident. `height_margin` widens the bounds for detail
    /// that roots do not record.
    pub(crate) fn assemble(
        lineage: u64,
        layout: TerrainLayout,
        finest_level: i32,
        collision_level: i32,
        materials: TerrainMaterialRules,
        tiles: HashMap<TileKey, Arc<TerrainTile>>,
        height_margin: f64,
    ) -> Result<Self, TerrainError> {
        let mut minimum = f64::INFINITY;
        let mut maximum = f64::NEG_INFINITY;
        for root in layout.roots() {
            let tile = tiles.get(&root).ok_or(TerrainError::InvalidTile("every root tile must be resident"))?;
            minimum = minimum.min(tile.minimum());
            maximum = maximum.max(tile.maximum());
        }
        let bounds = layout.footprint_bounds(minimum - height_margin, maximum + height_margin);
        Ok(Self {
            inner: Arc::new(SurfaceInner {
                lineage,
                layout,
                finest_level,
                collision_level,
                materials,
                tiles,
                bounds,
            }),
        })
    }

    /// Identity shared by every snapshot of one terrain (one asset or one streamer), for consumer caches.
    pub fn lineage(&self) -> u64 {
        self.inner.lineage
    }

    /// Whether two handles are the same snapshot.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// The pyramid geometry.
    pub fn layout(&self) -> &TerrainLayout {
        &self.inner.layout
    }

    /// The finest level selection may draw: 0 without detail, negative with synthesized detail.
    pub fn finest_level(&self) -> i32 {
        self.inner.finest_level
    }

    /// The level gameplay sampling and colliders read.
    pub fn collision_level(&self) -> i32 {
        self.inner.collision_level
    }

    /// Material rules shared by detail synthesis and terrain shading.
    pub fn materials(&self) -> &TerrainMaterialRules {
        &self.inner.materials
    }

    /// A resident tile.
    pub fn tile(&self, key: TileKey) -> Option<&Arc<TerrainTile>> {
        self.inner.tiles.get(&key)
    }

    /// Every resident tile, in no particular order.
    pub fn tiles(&self) -> impl Iterator<Item = &Arc<TerrainTile>> {
        self.inner.tiles.values()
    }

    /// Resident tiles.
    pub fn tile_count(&self) -> usize {
        self.inner.tiles.len()
    }

    /// Local bounds of the whole footprint and every height the resident levels can reach.
    pub fn bounds(&self) -> TerrainBounds {
        self.inner.bounds
    }

    /// Gameplay height at local (`x`, `z`): the collision level's triangle surface, the same surface terrain colliders
    /// use. `None` outside the footprint or where that level is not resident.
    pub fn height_at(&self, x: f64, z: f64) -> Option<f64> {
        self.level_height(self.inner.collision_level, x, z)
    }

    /// The finest resident height at local (`x`, `z`), for cameras and placement rather than gameplay. `None` outside
    /// the footprint.
    pub fn resident_height_at(&self, x: f64, z: f64) -> Option<f64> {
        let tile = self.finest_tile_at(x, z)?;
        self.lattice_height(tile, x, z)
    }

    /// The first hit of a ray from local `origin` along `direction` within `maximum_distance`, against the finest
    /// resident surface. Marches about two steps per cell of the tile under the ray, then refines by bisection.
    pub fn raycast(&self, origin: DVec3, direction: DVec3, maximum_distance: f64) -> Option<TerrainRayHit> {
        let length = direction.length();
        if !origin.is_finite() || !direction.is_finite() || length <= 0.0 || !maximum_distance.is_finite() {
            return None;
        }
        let direction = direction / length;
        let (enter, exit) = ray_box(origin, direction, self.inner.bounds)?;
        let mut previous = enter.max(0.0);
        let end = exit.min(maximum_distance);
        let above = |distance: f64| {
            let point = origin + (direction * distance);
            self.resident_height_at(point.x, point.z).map(|height| point.y - height)
        };
        let starting_gap = above(previous)?;
        if starting_gap <= 0.0 {
            return Some(TerrainRayHit {
                point: origin + (direction * previous),
                distance: previous,
            });
        }

        for _ in 0..MAXIMUM_RAY_STEPS {
            if previous >= end {
                return None;
            }
            let point = origin + (direction * previous);
            let step = self.march_step(point.x, point.z);
            let distance = (previous + step).min(end);
            let gap = above(distance)?;
            if gap <= 0.0 {
                return Some(refine_hit(origin, direction, previous, distance, &above));
            }
            previous = distance;
        }
        None
    }

    /// Height on the triangle surface of `level` at local (`x`, `z`), when that level's tile is resident.
    pub(crate) fn level_height(&self, level: i32, x: f64, z: f64) -> Option<f64> {
        let key = self.tile_key_at(level, x, z)?;
        let tile = self.inner.tiles.get(&key)?;
        self.lattice_height(tile, x, z)
    }

    /// The tile of `level` containing local (`x`, `z`); points on the far edges belong to the last tile.
    pub(crate) fn tile_key_at(&self, level: i32, x: f64, z: f64) -> Option<TileKey> {
        let layout = &self.inner.layout;
        let sample = layout.local_to_sample(x, z);
        let cells = layout.finest_cells();
        let inside = (0.0..=cells[0] as f64).contains(&sample[0]) && (0.0..=cells[1] as f64).contains(&sample[1]);
        if !inside || level < self.inner.finest_level {
            return None;
        }
        let span = layout.tile_span_samples(level);
        let tiles = layout.tiles_at(level);
        let column = ((sample[0] / span).floor() as u64).min(tiles[0].checked_sub(1)?) as u32;
        let row = ((sample[1] / span).floor() as u64).min(tiles[1].checked_sub(1)?) as u32;
        Some(TileKey::new(level, column, row))
    }

    /// The finest resident tile containing local (`x`, `z`).
    fn finest_tile_at(&self, x: f64, z: f64) -> Option<&Arc<TerrainTile>> {
        let layout = &self.inner.layout;
        let mut found = None;
        for level in (self.inner.finest_level..=layout.top_level()).rev() {
            let key = self.tile_key_at(level, x, z)?;
            match self.inner.tiles.get(&key) {
                Some(tile) => found = Some(tile),
                None => break,
            }
        }
        found
    }

    /// Triangle-surface height of one tile's lattice at local (`x`, `z`), with the renderer's diagonal split.
    fn lattice_height(&self, tile: &TerrainTile, x: f64, z: f64) -> Option<f64> {
        let layout = &self.inner.layout;
        let sample = layout.local_to_sample(x, z);
        let origin = layout.tile_origin_samples(tile.key());
        let step = deterministic::exp2(f64::from(tile.key().level));
        let cells = f64::from(layout.tile_cells);
        let lattice_x = ((sample[0] - origin[0]) / step).clamp(0.0, cells);
        let lattice_z = ((sample[1] - origin[1]) / step).clamp(0.0, cells);

        let base_x = lattice_x.floor().min(cells - 1.0);
        let base_z = lattice_z.floor().min(cells - 1.0);
        let (fraction_x, fraction_z) = (lattice_x - base_x, lattice_z - base_z);
        let (column, row) = (base_x as i32, base_z as i32);
        let a = f64::from(tile.height(column, row));
        let b = f64::from(tile.height(column + 1, row));
        let c = f64::from(tile.height(column, row + 1));
        let d = f64::from(tile.height(column + 1, row + 1));
        Some(if fraction_x >= fraction_z {
            a + ((b - a) * fraction_x) + ((d - b) * fraction_z)
        } else {
            a + ((d - c) * fraction_x) + ((c - a) * fraction_z)
        })
    }

    /// A marching step: a fraction of the lattice spacing of the finest tile under the point.
    fn march_step(&self, x: f64, z: f64) -> f64 {
        let layout = &self.inner.layout;
        let level = self.finest_tile_at(x, z).map(|tile| tile.key().level).unwrap_or(layout.top_level());
        (deterministic::exp2(f64::from(level)) * layout.spacing[0].min(layout.spacing[1])) / RAY_STEPS_PER_CELL
    }
}

/// A fresh lineage identity.
pub(crate) fn next_lineage() -> u64 {
    NEXT_LINEAGE.fetch_add(1, Ordering::Relaxed)
}

/// Bisect between a point above the surface and one at or below it.
fn refine_hit(
    origin: DVec3,
    direction: DVec3,
    mut above_distance: f64,
    mut below_distance: f64,
    gap: &impl Fn(f64) -> Option<f64>,
) -> TerrainRayHit {
    for _ in 0..RAY_REFINEMENT_STEPS {
        let middle = 0.5 * (above_distance + below_distance);
        match gap(middle) {
            Some(value) if value > 0.0 => above_distance = middle,
            _ => below_distance = middle,
        }
    }
    TerrainRayHit {
        point: origin + (direction * below_distance),
        distance: below_distance,
    }
}

/// Entry and exit distances of a normalized ray through a box, when it crosses it.
fn ray_box(origin: DVec3, direction: DVec3, bounds: TerrainBounds) -> Option<(f64, f64)> {
    let mut enter = f64::NEG_INFINITY;
    let mut exit = f64::INFINITY;
    for axis in 0..3 {
        let (start, step) = (origin[axis], direction[axis]);
        let (low, high) = (bounds.min[axis], bounds.max[axis]);
        if step.abs() < 1e-12 {
            if start < low || start > high {
                return None;
            }
            continue;
        }
        let first = (low - start) / step;
        let second = (high - start) / step;
        enter = enter.max(first.min(second));
        exit = exit.min(first.max(second));
    }
    (exit >= enter.max(0.0)).then_some((enter, exit))
}
