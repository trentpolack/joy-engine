// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`TerrainColliders`]: static heightfield bodies for a terrain surface's collision level, kept around moving
//! anchors.
//!
//! Each collider is one resident tile of the surface's collision level (`TerrainSurface::collision_level`), built from
//! the exact lattice the renderer draws there and gameplay samples, so bodies rest on the surface players see. A sync
//! creates colliders for tiles that enter an anchor's radius, rebuilds those whose tile revision changed (an edit such as
//! a crater), and removes the rest. Tiles are visited in key order, so the solver sees the same bodies in the same order
//! on every target and every run.
//!
//! The terrain's local space maps to the world by a translation only (`offset`); rotated or scaled terrain needs its
//! own collider transform. Colliders exist only where the collision level is resident: keep a streamer's collision
//! interests on the same anchors and radius.

use std::collections::{
    BTreeMap,
    BTreeSet,
};

use joy_core::math::Vec3;
use joy_terrain::{
    TerrainSurface,
    TileKey,
};

use super::physics_heightfield::HeightfieldOptions;
use super::{
    BodyHandle,
    PhysicsError,
    PhysicsWorld,
};

/// How terrain colliders sit in the world and which tiles they cover.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainColliderOptions {
    /// World translation of the terrain's local origin, in meters (the render instance's position).
    pub offset: Vec3,
    /// Tiles within this many meters of an anchor get colliders; finite and nonnegative.
    pub radius: f32,
    /// Friction coefficient; nonnegative.
    pub friction: f32,
    /// Restitution in `[0, 1]`.
    pub restitution: f32,
}

impl Default for TerrainColliderOptions {
    fn default() -> Self {
        Self {
            offset: Vec3::ZERO,
            radius: 32.0,
            friction: 0.8,
            restitution: 0.0,
        }
    }
}

/// What one sync changed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerrainColliderReport {
    /// Colliders after the sync.
    pub colliders: usize,
    /// Colliders created for new tiles or rebuilt for edited ones.
    pub built: usize,
    /// Colliders removed because their tile left every anchor's radius or the surface.
    pub removed: usize,
}

/// One tile's collider and the tile revision it was built from.
#[derive(Clone, Copy, Debug)]
struct TileCollider {
    body: BodyHandle,
    revision: u64,
}

/// Heightfield colliders for one terrain. Owns the bodies it creates in the world passed to [`TerrainColliders::sync`];
/// call [`TerrainColliders::clear`] (or drop the world) to release them.
#[derive(Debug)]
pub struct TerrainColliders {
    options: TerrainColliderOptions,
    lineage: Option<u64>,
    colliders: BTreeMap<TileKey, TileCollider>,
}

impl TerrainColliders {
    /// Validate options and start with no colliders.
    pub fn new(options: TerrainColliderOptions) -> Result<Self, PhysicsError> {
        if !options.offset.is_finite() {
            return Err(PhysicsError::NonFiniteVector("offset"));
        }
        let in_range = |value: f32, maximum: f64| value.is_finite() && value >= 0.0 && f64::from(value) <= maximum;
        if !in_range(options.radius, f64::INFINITY) {
            return Err(PhysicsError::OutOfRange {
                name: "radius",
                minimum: 0.0,
                maximum: f64::INFINITY,
            });
        }
        Ok(Self {
            options,
            lineage: None,
            colliders: BTreeMap::new(),
        })
    }

    /// Bring colliders in line with `surface` around world-space `anchors` (bodies the terrain must hold up).
    ///
    /// A surface of another lineage replaces every collider. Fails, leaving earlier colliders in place, if a tile cannot
    /// become a heightfield (for example with invalid friction).
    pub fn sync(
        &mut self,
        world: &mut PhysicsWorld,
        surface: &TerrainSurface,
        anchors: &[Vec3],
    ) -> Result<TerrainColliderReport, PhysicsError> {
        let mut report = TerrainColliderReport::default();
        if self.lineage != Some(surface.lineage()) {
            report.removed += self.clear(world);
            self.lineage = Some(surface.lineage());
        }

        // Resident collision-level tiles within reach of any anchor, in key order.
        let wanted = self.wanted_tiles(surface, anchors);
        let stale: Vec<TileKey> = self.colliders.keys().copied().filter(|key| !wanted.contains(key)).collect();
        for key in stale {
            if let Some(collider) = self.colliders.remove(&key) {
                world.remove_body(collider.body);
                report.removed += 1;
            }
        }
        for key in wanted {
            let tile = surface.tile(key).expect("wanted tiles are resident");
            if self.colliders.get(&key).is_some_and(|collider| collider.revision == tile.revision()) {
                continue;
            }
            let body = world.create_heightfield_body(&self.heightfield(surface, key))?;
            if let Some(previous) = self.colliders.insert(
                key,
                TileCollider {
                    body,
                    revision: tile.revision(),
                },
            ) {
                world.remove_body(previous.body);
            }
            report.built += 1;
        }
        report.colliders = self.colliders.len();
        Ok(report)
    }

    /// Remove every collider from `world`; returns how many were removed.
    pub fn clear(&mut self, world: &mut PhysicsWorld) -> usize {
        let count = self.colliders.len();
        for collider in std::mem::take(&mut self.colliders).into_values() {
            world.remove_body(collider.body);
        }
        count
    }

    /// Live colliders.
    pub fn collider_count(&self) -> usize {
        self.colliders.len()
    }

    /// The collider body of a tile, when it has one.
    pub fn body(&self, key: TileKey) -> Option<BodyHandle> {
        self.colliders.get(&key).map(|collider| collider.body)
    }

    /// Resident collision-level tiles whose footprint lies within the radius of an anchor.
    fn wanted_tiles(&self, surface: &TerrainSurface, anchors: &[Vec3]) -> BTreeSet<TileKey> {
        let layout = surface.layout();
        let level = surface.collision_level();
        let tiles = layout.tiles_at(level);
        let span = layout.tile_span_samples(level);
        let radius = f64::from(self.options.radius);
        let mut wanted = BTreeSet::new();
        for anchor in anchors.iter().filter(|anchor| anchor.is_finite()) {
            let local_x = f64::from(anchor.x - self.options.offset.x);
            let local_z = f64::from(anchor.z - self.options.offset.z);
            let low = layout.local_to_sample(local_x - radius, local_z - radius);
            let high = layout.local_to_sample(local_x + radius, local_z + radius);
            let range = |from: f64, to: f64, count: u64| {
                let first = (from / span).floor().clamp(0.0, count as f64 - 1.0) as u32;
                let last = (to / span).floor().clamp(0.0, count as f64 - 1.0) as u32;
                first..=last
            };
            for z in range(low[1], high[1], tiles[1]) {
                for x in range(low[0], high[0], tiles[0]) {
                    let key = TileKey::new(level, x, z);
                    let (minimum, maximum) = layout.tile_footprint(key);
                    let near_x = local_x.clamp(minimum[0], maximum[0]) - local_x;
                    let near_z = local_z.clamp(minimum[1], maximum[1]) - local_z;
                    let within = ((near_x * near_x) + (near_z * near_z)) <= (radius * radius);
                    if within && surface.tile(key).is_some() {
                        wanted.insert(key);
                    }
                }
            }
        }
        wanted
    }

    /// The heightfield of one resident tile's interior lattice.
    fn heightfield(&self, surface: &TerrainSurface, key: TileKey) -> HeightfieldOptions {
        let layout = surface.layout();
        let tile = surface.tile(key).expect("collider tiles are resident");
        let cells = layout.tile_cells as i32;
        let mut heights = Vec::with_capacity(((cells + 1) * (cells + 1)) as usize);
        for z in 0..=cells {
            for x in 0..=cells {
                heights.push(tile.height(x, z));
            }
        }
        let (minimum, _) = layout.tile_footprint(key);
        let step = layout.tile_span_samples(key.level) / f64::from(layout.tile_cells);
        HeightfieldOptions {
            origin: self.options.offset + Vec3::new(minimum[0] as f32, 0.0, minimum[1] as f32),
            spacing: [(layout.spacing[0] * step) as f32, (layout.spacing[1] * step) as f32],
            columns: (cells + 1) as usize,
            rows: (cells + 1) as usize,
            heights,
            friction: self.options.friction,
            restitution: self.options.restitution,
        }
    }
}
