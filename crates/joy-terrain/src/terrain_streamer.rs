// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`TerrainStreamer`]: keeps the tiles that views and colliders need resident, synthesizes detail, applies craters,
//! and publishes [`TerrainSurface`] snapshots.
//!
//! One update, in order:
//! 1. Finished loads become resident (with every crater applied).
//! 2. Each interest marks the tiles it uses and lists the ones it is missing: a view runs LOD selection on the current
//!    snapshot, and a collision region needs its collision-level tiles and their ancestors. Collision comes first.
//! 3. Missing stored tiles are requested, coarsest first, up to the pending-load budget; missing detail tiles are
//!    synthesized from their resident level-0 ancestor, up to the per-update budget.
//! 4. Over the residency budget, the least recently used tiles with no resident children leave (roots never do).
//! 5. When anything changed and every root is resident, a new snapshot is published.
//!
//! Loading is never awaited: a view draws what is resident and refines as tiles arrive. The streamer owns its loader
//! and every resident tile; dropping it releases them, while published snapshots keep the tiles they hold.

use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::Arc;

use crate::terrain_crater::{
    TerrainCrater,
    apply_craters,
};
use crate::terrain_detail::{
    TerrainDetail,
    TerrainMaterialRules,
    effective_errors,
    offset_errors,
    synthesize_tile,
};
use crate::terrain_error::TerrainError;
use crate::terrain_layout::{
    TerrainLayout,
    TileKey,
};
use crate::terrain_loader::TerrainTileLoader;
use crate::terrain_quadtree::select_terrain_nodes;
use crate::terrain_selection::TerrainView;
use crate::terrain_surface::{
    TerrainSurface,
    next_lineage,
};
use crate::terrain_tile::TerrainTile;

/// Updates to wait before asking again for a tile whose load failed, or that left before any interest used it (demand
/// beyond the residency budget would otherwise fetch and release the same tiles every update).
const RETRY_AFTER_UPDATES: u64 = 120;

/// Limits that keep streaming work and memory bounded.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TerrainStreamingBudget {
    /// Resident tiles kept before least recently used ones leave. Tiles in use this update always stay.
    pub maximum_resident_tiles: usize,
    /// Stored-tile loads in flight at once.
    pub maximum_pending_loads: usize,
    /// Detail tiles synthesized (or re-synthesized after an edit) per update.
    pub maximum_syntheses_per_update: usize,
}

impl Default for TerrainStreamingBudget {
    fn default() -> Self {
        Self {
            maximum_resident_tiles: 1024,
            maximum_pending_loads: 32,
            maximum_syntheses_per_update: 8,
        }
    }
}

/// How a streamer shapes its surface.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TerrainStreamerOptions {
    /// Synthesized detail below the authored lattice; `None` keeps the authored resolution.
    pub detail: Option<TerrainDetail>,
    /// Material rules for detail and shading.
    pub materials: TerrainMaterialRules,
    /// Work and memory limits.
    pub budget: TerrainStreamingBudget,
}

/// What a streamer should keep resident this update. Positions are the terrain's local space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TerrainInterest {
    /// A camera: keep what its LOD selection draws and fetch what would refine it.
    View(TerrainView),
    /// A gameplay region: keep the collision level resident within `radius` world units of local X/Z `center`.
    Collision {
        /// Local X/Z center.
        center: [f64; 2],
        /// Radius in world units; finite and nonnegative.
        radius: f64,
    },
}

/// Counts from one update, for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TerrainStreamingReport {
    /// Resident tiles after the update.
    pub resident_tiles: usize,
    /// Loads still in flight.
    pub pending_loads: usize,
    /// Tiles that arrived this update.
    pub loaded: usize,
    /// Detail tiles synthesized this update.
    pub synthesized: usize,
    /// Tiles that left this update.
    pub evicted: usize,
    /// Loads that failed this update.
    pub failed: usize,
    /// Missing tiles still wanted after this update's requests.
    pub waiting: usize,
    /// Sample and error bytes held by resident tiles.
    pub resident_bytes: usize,
    /// Whether this update published a new snapshot.
    pub published: bool,
}

/// Streams one terrain's tiles. See the module documentation for the update order.
pub struct TerrainStreamer {
    // Configuration.
    lineage: u64,
    layout: TerrainLayout,
    options: TerrainStreamerOptions,
    loader: Box<dyn TerrainTileLoader>,

    // Stored tiles as loaded, before craters, and every resident tile as published.
    sources: HashMap<TileKey, Arc<TerrainTile>>,
    resident: HashMap<TileKey, Arc<TerrainTile>>,
    last_used: HashMap<TileKey, u64>,
    pending: HashSet<TileKey>,
    arrived: HashMap<TileKey, u64>,
    backoff: HashMap<TileKey, u64>,

    // Edits, indexed by the root tiles they reach.
    craters: Vec<TerrainCrater>,
    craters_by_root: HashMap<TileKey, Vec<usize>>,

    // Counters and the latest snapshot.
    update: u64,
    next_revision: u64,
    surface: Option<TerrainSurface>,
    changed: bool,
}

impl TerrainStreamer {
    /// A streamer for `layout` that loads stored tiles through `loader`. Requests every root immediately; the first
    /// snapshot appears once they are all resident.
    pub fn new(
        layout: TerrainLayout,
        loader: Box<dyn TerrainTileLoader>,
        options: TerrainStreamerOptions,
    ) -> Result<Self, TerrainError> {
        layout.validate()?;
        options.materials.validate()?;
        if let Some(detail) = &options.detail {
            detail.validate()?;
        }
        let budget = options.budget;
        if budget.maximum_resident_tiles == 0 || budget.maximum_pending_loads == 0 {
            return Err(TerrainError::InvalidBudget("resident tiles and pending loads must be positive"));
        }
        let mut streamer = Self {
            lineage: next_lineage(),
            layout,
            options,
            loader,
            sources: HashMap::new(),
            resident: HashMap::new(),
            last_used: HashMap::new(),
            pending: HashSet::new(),
            arrived: HashMap::new(),
            backoff: HashMap::new(),
            craters: Vec::new(),
            craters_by_root: HashMap::new(),
            update: 0,
            next_revision: 1,
            surface: None,
            changed: false,
        };
        let roots: Vec<TileKey> = streamer.layout.roots().collect();
        for root in roots {
            streamer.loader.request(root);
            streamer.pending.insert(root);
        }
        Ok(streamer)
    }

    /// The latest snapshot; `None` until every root tile is resident.
    pub fn surface(&self) -> Option<&TerrainSurface> {
        self.surface.as_ref()
    }

    /// The pyramid geometry.
    pub fn layout(&self) -> &TerrainLayout {
        &self.layout
    }

    /// Every crater applied so far, in the order added.
    pub fn craters(&self) -> &[TerrainCrater] {
        &self.craters
    }

    /// Run one streaming update for this frame's interests. Fails before any change for an invalid interest.
    pub fn update(&mut self, interests: &[TerrainInterest]) -> Result<TerrainStreamingReport, TerrainError> {
        for interest in interests {
            if let TerrainInterest::Collision {
                center,
                radius,
            } = interest
                && (!center.iter().all(|value| value.is_finite()) || !radius.is_finite() || *radius < 0.0)
            {
                return Err(TerrainError::InvalidView);
            }
        }
        self.update += 1;
        let mut report = TerrainStreamingReport::default();
        self.receive_loads(&mut report);

        // Collision first: gameplay correctness outranks view refinement.
        let mut wanted = Vec::new();
        self.touch_roots();
        for interest in interests {
            if let TerrainInterest::Collision {
                center,
                radius,
            } = interest
            {
                self.collect_collision(*center, *radius, &mut wanted);
            }
        }
        if let Some(surface) = self.surface.clone() {
            for interest in interests {
                if let TerrainInterest::View(view) = interest {
                    self.collect_view(&surface, view, &mut wanted)?;
                }
            }
        }

        let waiting = self.fetch(&wanted, &mut report);
        self.evict(waiting, &mut report);
        report.published = self.publish();
        report.waiting = waiting;
        report.resident_tiles = self.resident.len();
        report.pending_loads = self.pending.len();
        report.resident_bytes = self.resident.values().map(|tile| tile.payload_bytes()).sum();
        Ok(report)
    }

    /// Add a crater and re-resolve every resident tile it reaches; the next update publishes the result.
    pub fn add_crater(&mut self, crater: TerrainCrater) -> Result<(), TerrainError> {
        crater.validate()?;
        let index = self.craters.len();
        self.craters.push(crater);
        let roots: Vec<TileKey> = self.layout.roots().filter(|root| self.reaches(&crater, *root)).collect();
        for root in roots {
            self.craters_by_root.entry(root).or_default().push(index);
        }

        // Stored tiles first, so detail tiles synthesize from the edited level 0.
        let mut touched: Vec<TileKey> =
            self.resident.keys().copied().filter(|key| self.reaches(&crater, *key)).collect();
        touched.sort_by_key(|key| std::cmp::Reverse(key.level));
        for key in touched {
            if key.level >= 0 {
                let source = self.sources.get(&key).cloned().expect("every stored resident tile has a source");
                let resolved = self.resolve_stored(&source)?;
                self.resident.insert(key, resolved);
            } else {
                // Re-synthesized at once (coarser detail levels first), so the edit never leaves a hole.
                match self.synthesize(key) {
                    Some(tile) => {
                        self.resident.insert(key, tile);
                    }
                    None => {
                        self.resident.remove(&key);
                        self.last_used.remove(&key);
                    }
                }
            }
        }
        self.changed = true;
        Ok(())
    }

    /// Mark every root as used.
    fn touch_roots(&mut self) {
        let roots: Vec<TileKey> = self.layout.roots().collect();
        for root in roots {
            self.touch(root);
        }
    }

    /// Resolve finished loads.
    fn receive_loads(&mut self, report: &mut TerrainStreamingReport) {
        for load in self.loader.poll() {
            if !self.pending.remove(&load.key) {
                continue;
            }
            let accepted = load.result.and_then(|tile| {
                if tile.key() != load.key || tile.key().level < 0 || tile.side() != self.layout.stored_tile_side() {
                    return Err(TerrainError::InvalidTile("the loader returned a different tile"));
                }
                Ok(Arc::new(tile))
            });
            let resolved = accepted.and_then(|source| self.resolve_stored(&source).map(|tile| (source, tile)));
            match resolved {
                Ok((source, tile)) => {
                    self.sources.insert(load.key, source);
                    self.resident.insert(load.key, tile);
                    self.last_used.insert(load.key, self.update);
                    self.arrived.insert(load.key, self.update);
                    self.changed = true;
                    report.loaded += 1;
                }
                Err(_) => {
                    self.backoff.insert(load.key, self.update);
                    report.failed += 1;
                }
            }
        }
    }

    /// Tiles a collision region needs: its collision-level tiles and their ancestors, coarsest missing first.
    fn collect_collision(&mut self, center: [f64; 2], radius: f64, wanted: &mut Vec<TileKey>) {
        let level = self.collision_level();
        let minimum = self.layout.local_to_sample(center[0] - radius, center[1] - radius);
        let maximum = self.layout.local_to_sample(center[0] + radius, center[1] + radius);
        let span = self.layout.tile_span_samples(level);
        let tiles = self.layout.tiles_at(level);
        let range = |low: f64, high: f64, count: u64| {
            let first = (low / span).floor().clamp(0.0, count as f64 - 1.0) as u32;
            let last = (high / span).floor().clamp(0.0, count as f64 - 1.0) as u32;
            first..=last
        };
        for z in range(minimum[1], maximum[1], tiles[1]) {
            for x in range(minimum[0], maximum[0], tiles[0]) {
                let key = TileKey::new(level, x, z);
                let (low, high) = self.layout.tile_footprint(key);
                let near_x = center[0].clamp(low[0], high[0]) - center[0];
                let near_z = center[1].clamp(low[1], high[1]) - center[1];
                if ((near_x * near_x) + (near_z * near_z)) > (radius * radius) {
                    continue;
                }
                for chain_level in (level..=self.layout.top_level()).rev() {
                    let ancestor = key.ancestor(chain_level);
                    if self.resident.contains_key(&ancestor) {
                        self.touch(ancestor);
                    } else {
                        wanted.push(ancestor);
                        break;
                    }
                }
            }
        }
    }

    /// Tiles a view draws (kept) and would refine with (wanted).
    fn collect_view(
        &mut self,
        surface: &TerrainSurface,
        view: &TerrainView,
        wanted: &mut Vec<TileKey>,
    ) -> Result<(), TerrainError> {
        let selection = select_terrain_nodes(surface, view)?;
        for node in &selection.nodes {
            for level in node.key.level..=self.layout.top_level() {
                self.touch(node.key.ancestor(level));
            }
        }
        wanted.extend(selection.wanted);
        Ok(())
    }

    /// Request or synthesize wanted tiles within budget; returns how many are still missing.
    fn fetch(&mut self, wanted: &[TileKey], report: &mut TerrainStreamingReport) -> usize {
        let mut waiting = 0;
        let mut seen = HashSet::new();
        for &key in wanted {
            if !seen.insert(key) || self.resident.contains_key(&key) || self.pending.contains(&key) {
                continue;
            }

            // At the residency limit, new work waits for eviction to free room; otherwise tiles would be fetched only
            // to be released again before any view uses them.
            if self.resident.len() + self.pending.len() >= self.options.budget.maximum_resident_tiles {
                waiting += 1;
                continue;
            }
            let backing_off = self.backoff.get(&key).is_some_and(|since| self.update - since < RETRY_AFTER_UPDATES);
            if backing_off {
                waiting += 1;
                continue;
            }
            if key.level >= 0 {
                if self.pending.len() >= self.options.budget.maximum_pending_loads {
                    waiting += 1;
                    continue;
                }
                self.loader.request(key);
                self.pending.insert(key);
                continue;
            }
            if report.synthesized >= self.options.budget.maximum_syntheses_per_update {
                waiting += 1;
                continue;
            }
            match self.synthesize(key) {
                Some(tile) => {
                    self.resident.insert(key, tile);
                    self.touch(key);
                    self.arrived.insert(key, self.update);
                    self.changed = true;
                    report.synthesized += 1;
                }
                None => waiting += 1,
            }
        }
        waiting
    }

    /// Release least recently used leaf tiles over the residency budget, and, while tiles wait for room, enough stale
    /// tiles to start the next update's work. Tiles used this update never leave.
    fn evict(&mut self, waiting: usize, report: &mut TerrainStreamingReport) {
        let budget = self.options.budget;
        let headroom = waiting.min(budget.maximum_pending_loads + budget.maximum_syntheses_per_update);
        let limit = budget.maximum_resident_tiles.saturating_sub(headroom);
        while self.resident.len() + self.pending.len() > limit {
            let roots: HashSet<TileKey> = self.layout.roots().collect();
            let candidate = self
                .resident
                .keys()
                .copied()
                .filter(|key| !roots.contains(key))
                .filter(|key| self.last_used.get(key).copied().unwrap_or(0) < self.update)
                .filter(|key| key.children().iter().all(|child| !self.resident.contains_key(child)))
                .min_by_key(|key| (self.last_used.get(key).copied().unwrap_or(0), key.level, key.z, key.x));
            let Some(key) = candidate else {
                return;
            };
            let used_after_arrival = self.last_used.get(&key) > self.arrived.get(&key);
            if !used_after_arrival {
                self.backoff.insert(key, self.update);
            }
            self.resident.remove(&key);
            self.sources.remove(&key);
            self.last_used.remove(&key);
            self.arrived.remove(&key);
            self.changed = true;
            report.evicted += 1;
        }
    }

    /// Publish a new snapshot when something changed and every root is resident.
    fn publish(&mut self) -> bool {
        if !self.changed || !self.layout.roots().all(|root| self.resident.contains_key(&root)) {
            return false;
        }
        let (finest, collision, margin) = match &self.options.detail {
            Some(detail) => (detail.finest_level(), detail.collision_level, detail.amplitude_bound()),
            None => (0, 0, 0.0),
        };
        let surface = TerrainSurface::assemble(
            self.lineage,
            self.layout.clone(),
            finest,
            collision,
            self.options.materials,
            self.resident.clone(),
            margin,
        );
        match surface {
            Ok(surface) => {
                self.surface = Some(surface);
                self.changed = false;
                true
            }
            Err(_) => false,
        }
    }

    /// A stored tile with its craters, and its errors moved onto the detail surface when detail is on.
    fn resolve_stored(&mut self, source: &TerrainTile) -> Result<Arc<TerrainTile>, TerrainError> {
        let craters = self.craters_touching(source.key());
        let edited = apply_craters(&self.layout, source, &craters)?;
        let revision = self.next_revision();
        let Some(detail) = self.options.detail else {
            return Ok(Arc::new(edited.with_revision(revision)));
        };

        // The finest surface now carries detail noise, so bounds widen and authored levels gain its error.
        let margin = detail.amplitude_bound();
        let (first, errors) = if source.key().level == 0 {
            (detail.finest_level(), effective_errors(&detail, &self.options.materials, &self.layout, &edited))
        } else {
            (0, offset_errors(&detail, &self.options.materials, &self.layout, &edited))
        };
        let heights = edited.heights().to_vec();
        let rebuilt = edited.rebuilt(
            &self.layout,
            heights,
            edited.minimum() - margin,
            edited.maximum() + margin,
            first,
            errors,
        )?;
        Ok(Arc::new(rebuilt.with_revision(revision)))
    }

    /// Synthesize a detail tile when its level-0 ancestor is resident.
    fn synthesize(&mut self, key: TileKey) -> Option<Arc<TerrainTile>> {
        let detail = self.options.detail?;
        if key.level < detail.finest_level() {
            return None;
        }
        let ancestor = key.ancestor(0);
        let source = self.sources.get(&ancestor)?.clone();
        let level_zero = self.resident.get(&ancestor)?.clone();
        let craters = self.craters_touching(key);
        let tile = synthesize_tile(
            &self.layout,
            &detail,
            &self.options.materials,
            key,
            &source,
            &craters,
            level_zero.errors(),
        )
        .ok()?;
        let revision = self.next_revision();
        Some(Arc::new(tile.with_revision(revision)))
    }

    /// Every crater reaching a tile's footprint.
    fn craters_touching(&self, key: TileKey) -> Vec<TerrainCrater> {
        let root = key.ancestor(self.layout.top_level());
        let Some(indices) = self.craters_by_root.get(&root) else {
            return Vec::new();
        };
        let (low, high) = self.layout.tile_footprint(key);
        indices.iter().map(|index| self.craters[*index]).filter(|crater| crater.touches(low, high)).collect()
    }

    /// Whether a crater reaches a tile's footprint.
    fn reaches(&self, crater: &TerrainCrater, key: TileKey) -> bool {
        let (low, high) = self.layout.tile_footprint(key);
        crater.touches(low, high)
    }

    /// The collision level of this streamer's surface.
    fn collision_level(&self) -> i32 {
        self.options.detail.map(|detail| detail.collision_level).unwrap_or(0)
    }

    /// Record a tile as used this update.
    fn touch(&mut self, key: TileKey) {
        self.last_used.insert(key, self.update);
    }

    /// A fresh tile revision.
    fn next_revision(&mut self) -> u64 {
        let revision = self.next_revision;
        self.next_revision += 1;
        revision
    }
}
