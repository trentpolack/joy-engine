// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Retained render proxies: what a frame reads about each mesh instance (its world matrix including every ancestor
//! node, its world bounds, and its draw policy), kept between frames on the scene's dense instance slots, plus the
//! bounds tree culling queries.
//!
//! Edits mark proxies before they happen, because `RenderScene::get_mut` hands out a plain mutable borrow:
//! - `get_mut` marks its instance edited (the caller may change anything);
//! - inserting, removing, and reparenting an instance marks its slot;
//! - editing, reparenting, or removing a scene node marks the node, and the next refresh marks every instance beneath
//!   it with a hierarchy change.
//!
//! A refresh (the renderer runs one as each frame begins) compares each marked instance with the snapshot its proxy was
//! built from, so a mark becomes an exact [`ProxyChanges`]: transform, hierarchy, mesh, material, or other presentation
//! state. Only transform, hierarchy, and mesh changes recompute the world matrix and bounds and touch the tree, and a
//! marked instance that did not change costs one comparison.
//!
//! Every change to what the scene writes into shadow maps is recorded in a short caster journal: the bounds a caster
//! left and the bounds it now covers. Each renderer reads the journal from its own cursor to find which cached shadow
//! maps went stale (see `shadows/cache.rs`).

use std::ops::{
    BitOr,
    BitOrAssign,
};

use joy_core::math::Mat4;
use joy_core::{
    LinearColor,
    Transform,
};

use crate::culling::{
    EMPTY_BOUNDS,
    Overlap,
    world_bounds,
};
use crate::mesh::Bounds;
use crate::scene::bounds_tree::{
    BoundsTree,
    TreeLeaf,
};
use crate::scene::{
    MeshInstance,
    RenderPassKind,
    RenderScene,
    SceneNodeId,
};

/// Caster changes the journal keeps before it drops the oldest, so a scene no renderer reads cannot grow it. A frame
/// that changes more casters than this re-renders every shadow map, which it would nearly do anyway.
const JOURNAL_CAPACITY: usize = 8192;

/// Refreshes the journal keeps changes for. A renderer reads the journal every frame it draws the scene, so it is at
/// most one refresh behind; the rest covers other renderers drawing the same scene.
const JOURNAL_REVISIONS: u64 = 8;

/// What changed on a proxy, as a set of flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProxyChanges(u8);

impl ProxyChanges {
    /// Nothing changed.
    pub(crate) const NONE: Self = Self(0);
    /// The instance was borrowed mutably; the refresh compares it with its snapshot to find what changed.
    pub(crate) const EDITED: Self = Self(1);
    /// The instance's own transform changed.
    pub(crate) const TRANSFORM: Self = Self(1 << 1);
    /// The instance's parent, or an ancestor node's transform or visibility, changed.
    pub(crate) const HIERARCHY: Self = Self(1 << 2);
    /// The instance's mesh asset changed, or the instance is new.
    pub(crate) const MESH: Self = Self(1 << 3);
    /// A material slot override changed.
    pub(crate) const MATERIAL: Self = Self(1 << 4);
    /// Tint, opacity, visibility, queue, or shadow policy changed.
    pub(crate) const STATE: Self = Self(1 << 5);

    /// Whether any flag of `other` is set.
    pub(crate) fn intersects(self, other: Self) -> bool {
        (self.0 & other.0) != 0
    }

    /// Whether no flag is set.
    pub(crate) fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }
}

impl BitOr for ProxyChanges {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl BitOrAssign for ProxyChanges {
    fn bitor_assign(&mut self, other: Self) {
        self.0 |= other.0;
    }
}

/// One change to what the scene writes into shadow maps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CasterChange {
    /// The refresh revision that recorded the change.
    pub revision: u64,
    /// World bounds the caster covered before the change; `None` when it did not cast.
    pub before: Option<Bounds>,
    /// World bounds it covers now; `None` when it no longer casts.
    pub after: Option<Bounds>,
}

/// Where a drawn proxy sits in the culling index.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ProxyPlacement {
    /// Not drawn, so no query reaches it.
    #[default]
    Absent,
    /// A leaf of the bounds tree.
    Tree(TreeLeaf),
    /// Drawn with non-finite bounds, which the tree cannot order; every query visits it and its exact test decides.
    Unbounded,
}

/// The instance state a proxy was last built from, compared on refresh to classify a mark.
#[derive(Clone, Debug, Default)]
struct ProxySnapshot {
    transform: Transform,
    parent: Option<SceneNodeId>,
    asset: u64,
    /// Per-slot material override identities.
    materials: Vec<Option<u64>>,
    tint: LinearColor,
    opacity: f32,
    visible: bool,
    pass: RenderPassKind,
    casts_shadow: bool,
    receives_shadow: bool,
}

impl ProxySnapshot {
    fn of(instance: &MeshInstance) -> Self {
        let mut snapshot = Self::default();
        snapshot.capture(instance);
        snapshot
    }

    /// Record `instance`'s current state, reusing the override list's allocation.
    fn capture(&mut self, instance: &MeshInstance) {
        self.transform = instance.transform;
        self.parent = instance.parent;
        self.asset = instance.asset.id();
        self.materials.clear();
        self.materials.extend(instance.materials.iter().map(|material| material.as_ref().map(|asset| asset.id())));
        self.tint = instance.tint;
        self.opacity = instance.opacity;
        self.visible = instance.visible;
        self.pass = instance.pass;
        self.casts_shadow = instance.casts_shadow;
        self.receives_shadow = instance.receives_shadow;
    }

    /// What differs between this snapshot and `instance`, and whether the tint or opacity (which alpha-tested casters
    /// read) did.
    fn changes(&self, instance: &MeshInstance) -> (ProxyChanges, bool) {
        let mut changes = ProxyChanges::NONE;
        if self.transform != instance.transform {
            changes |= ProxyChanges::TRANSFORM;
        }
        if self.parent != instance.parent {
            changes |= ProxyChanges::HIERARCHY;
        }
        if self.asset != instance.asset.id() {
            changes |= ProxyChanges::MESH;
        }
        let same_materials = self.materials.len() == instance.materials.len()
            && self
                .materials
                .iter()
                .zip(&instance.materials)
                .all(|(id, material)| *id == material.as_ref().map(|asset| asset.id()));
        if !same_materials {
            changes |= ProxyChanges::MATERIAL;
        }
        let coverage_changed = self.tint != instance.tint || self.opacity != instance.opacity;
        let state_changed = coverage_changed
            || self.visible != instance.visible
            || self.pass != instance.pass
            || self.casts_shadow != instance.casts_shadow
            || self.receives_shadow != instance.receives_shadow;
        if state_changed {
            changes |= ProxyChanges::STATE;
        }
        (changes, coverage_changed)
    }
}

/// What a frame reads about one mesh instance, cached between frames.
#[derive(Clone, Debug)]
pub(crate) struct RenderProxy {
    /// The slot generation this proxy describes; a different generation means the slot was recycled.
    generation: u32,
    /// Whether the proxy describes a live instance.
    live: bool,
    /// Marks waiting for the next refresh.
    pending: ProxyChanges,
    /// World matrix, including every ancestor node.
    pub(crate) world: Mat4,
    /// World-space bounds of the asset under `world`.
    pub(crate) bounds: Bounds,
    /// Whether the instance and every ancestor are visible.
    shown: bool,
    /// Whether it is shown and its opacity is above zero: whether it draws at all.
    pub(crate) drawn: bool,
    /// Whether every submesh draws in the sorted transparent queue ([`MeshInstance::is_transparent`]).
    pub(crate) transparent: bool,
    /// Whether some opaque submesh writes shadow maps.
    pub(crate) casts: bool,
    /// Whether some casting submesh is alpha-tested, so tint and opacity reach the shadow maps.
    masked: bool,
    placement: ProxyPlacement,
    snapshot: ProxySnapshot,
}

impl Default for RenderProxy {
    /// A proxy for an empty slot.
    fn default() -> Self {
        Self {
            generation: 0,
            live: false,
            pending: ProxyChanges::NONE,
            world: Mat4::IDENTITY,
            bounds: EMPTY_BOUNDS,
            shown: false,
            drawn: false,
            transparent: false,
            casts: false,
            masked: false,
            placement: ProxyPlacement::Absent,
            snapshot: ProxySnapshot::default(),
        }
    }
}

impl RenderProxy {
    /// The bounds this proxy writes into shadow maps, when it casts.
    fn caster_bounds(&self) -> Option<Bounds> {
        (self.live && self.casts).then_some(self.bounds)
    }
}

/// The scene's render proxies, the culling index over them, and the caster journal. Owned by [`RenderScene`].
#[derive(Debug, Default)]
pub(crate) struct SceneProxies {
    /// One proxy per instance slot, indexed like the scene's slots.
    proxies: Vec<RenderProxy>,
    /// Slots with marks, each listed once.
    pending: Vec<u32>,
    /// Node slots whose transform, visibility, or parent may have changed.
    pending_nodes: Vec<u32>,
    tree: BoundsTree,
    /// Drawn proxies with non-finite bounds.
    unbounded: Vec<u32>,
    /// Live proxies that cast.
    casters: usize,
    /// Increases with every refresh that applied marks.
    revision: u64,
    journal: Vec<CasterChange>,
    /// Changes recorded at or before this revision may be missing from the journal.
    journal_floor: u64,
}

impl SceneProxies {
    // --- Marks (edits, under the scene's mutable borrow) ----------------------------------------------------------

    /// Mark instance slot `index` with `changes` for the next refresh.
    pub(crate) fn mark(&mut self, index: u32, changes: ProxyChanges) {
        let slot = index as usize;
        if self.proxies.len() <= slot {
            self.proxies.resize_with(slot + 1, RenderProxy::default);
        }
        let proxy = &mut self.proxies[slot];
        if proxy.pending.is_empty() {
            self.pending.push(index);
        }
        // An empty set still queues the slot (an insertion or removal), so keep a flag that a refresh ignores.
        proxy.pending |= if changes.is_empty() { ProxyChanges::EDITED } else { changes };
    }

    /// Mark scene node slot `index`; the next refresh marks every instance beneath it.
    pub(crate) fn mark_node(&mut self, index: u32) {
        if !self.pending_nodes.contains(&index) {
            self.pending_nodes.push(index);
        }
    }

    /// Forget every proxy (the scene was cleared). Renderers reading the journal from before this re-render all of
    /// their shadow maps.
    pub(crate) fn reset(&mut self) {
        self.proxies.clear();
        self.pending.clear();
        self.pending_nodes.clear();
        self.tree.clear();
        self.unbounded.clear();
        self.casters = 0;
        self.journal.clear();
        self.revision += 1;
        self.journal_floor = self.revision;
    }

    /// Rebuild every proxy from scratch on the next refresh, after a panic left the cache in an unknown state.
    pub(crate) fn rebuild(&mut self, slots: usize) {
        self.reset();
        for index in 0..slots {
            self.mark(index as u32, ProxyChanges::MESH);
        }
    }

    // --- Refresh ---------------------------------------------------------------------------------------------------

    /// Apply every mark since the last refresh to the proxies, the bounds tree, and the caster journal.
    pub(crate) fn refresh(&mut self, scene: &RenderScene) {
        if !self.pending_nodes.is_empty() {
            self.mark_beneath_pending_nodes(scene);
        }
        if self.pending.is_empty() {
            return;
        }
        self.revision += 1;
        let pending = std::mem::take(&mut self.pending);
        for index in &pending {
            self.refresh_slot(scene, *index);
        }
        // Keep the list's allocation for the next frame's marks.
        self.pending = pending;
        self.pending.clear();
        self.trim_journal();
    }

    /// Mark every instance whose parent chain reaches a pending node with a hierarchy change.
    fn mark_beneath_pending_nodes(&mut self, scene: &RenderScene) {
        // Whether each node slot is pending or beneath a pending node, resolved once per node and remembered.
        let mut affected: Vec<Option<bool>> = vec![None; scene.node_slots.len()];
        for index in self.pending_nodes.drain(..) {
            if let Some(entry) = affected.get_mut(index as usize) {
                *entry = Some(true);
            }
        }
        let mut marked = Vec::new();
        for (index, slot) in scene.slots.iter().enumerate() {
            let Some(parent) = slot.instance.as_ref().and_then(|instance| instance.parent) else {
                continue;
            };
            if node_affected(scene, parent, &mut affected) {
                marked.push(index as u32);
            }
        }
        for index in marked {
            self.mark(index, ProxyChanges::HIERARCHY);
        }
    }

    /// Bring one slot's proxy up to date with the instance it holds now.
    fn refresh_slot(&mut self, scene: &RenderScene, index: u32) {
        let slot = &scene.slots[index as usize];
        let proxy = &mut self.proxies[index as usize];
        let marks = std::mem::take(&mut proxy.pending);
        let recycled = proxy.live && proxy.generation != slot.generation;
        let fresh = !proxy.live || recycled;
        match &slot.instance {
            None => {
                if proxy.live {
                    self.retire(index);
                }
            }
            Some(instance) if fresh => {
                if recycled {
                    self.retire(index);
                }
                self.create(scene, index, slot.generation, instance);
            }
            Some(instance) => self.update(scene, index, instance, marks),
        }
    }

    /// Build a proxy for a new instance in slot `index`.
    fn create(&mut self, scene: &RenderScene, index: u32, generation: u32, instance: &MeshInstance) {
        let (world, shown) = scene.compose(instance.parent, instance.transform.matrix(), instance.visible);
        let mut proxy = RenderProxy {
            generation,
            live: true,
            pending: ProxyChanges::NONE,
            world,
            bounds: world_bounds(instance.asset.bounds(), world),
            snapshot: ProxySnapshot::of(instance),
            ..RenderProxy::default()
        };
        apply_draw_policy(&mut proxy, instance, shown);
        let after = proxy.caster_bounds();
        self.proxies[index as usize] = proxy;
        self.place(index);
        if after.is_some() {
            self.casters += 1;
            self.record(None, after);
        }
    }

    /// Classify a marked instance's changes and apply them.
    fn update(&mut self, scene: &RenderScene, index: u32, instance: &MeshInstance, marks: ProxyChanges) {
        let proxy = &mut self.proxies[index as usize];
        let (differences, coverage_changed) = proxy.snapshot.changes(instance);
        let mut changes = marks.without(ProxyChanges::EDITED) | differences;
        if changes.is_empty() {
            return;
        }

        let before = proxy.caster_bounds();
        let was_drawn = proxy.drawn;
        let was_masked = proxy.masked;

        // The world matrix and inherited visibility follow the transform, the hierarchy, and the instance's own
        // visibility; a hierarchy mark whose ancestors did not actually change leaves the matrix as it was.
        let mut shown = proxy.shown;
        let placement_changes = ProxyChanges::TRANSFORM | ProxyChanges::HIERARCHY | ProxyChanges::STATE;
        if changes.intersects(placement_changes) {
            let (world, visible) = scene.compose(instance.parent, instance.transform.matrix(), instance.visible);
            if world == proxy.world {
                changes = changes.without(ProxyChanges::TRANSFORM | ProxyChanges::HIERARCHY);
            }
            proxy.world = world;
            shown = visible;
        }
        let moved = changes.intersects(ProxyChanges::TRANSFORM | ProxyChanges::HIERARCHY | ProxyChanges::MESH);
        if moved {
            proxy.bounds = world_bounds(instance.asset.bounds(), proxy.world);
        }
        apply_draw_policy(proxy, instance, shown);
        proxy.snapshot.capture(instance);

        // Keep the culling index in step with what draws and where.
        let now_drawn = proxy.drawn;
        if moved || was_drawn != now_drawn {
            self.place(index);
        }

        // Shadow maps read the geometry (position, mesh, and materials) of casting submeshes, and alpha-tested casters
        // also read tint and opacity.
        let proxy = &self.proxies[index as usize];
        let after = proxy.caster_bounds();
        let coverage = coverage_changed && (was_masked || proxy.masked);
        let geometry = moved || changes.intersects(ProxyChanges::MATERIAL) || coverage;
        if before.is_some() != after.is_some() || (after.is_some() && geometry) {
            self.casters = (self.casters + usize::from(after.is_some())) - usize::from(before.is_some());
            self.record(before, after);
        }
    }

    /// Retire the proxy of a removed (or recycled) instance.
    fn retire(&mut self, index: u32) {
        let proxy = &mut self.proxies[index as usize];
        let before = proxy.caster_bounds();
        proxy.drawn = false;
        self.place(index);
        self.proxies[index as usize].live = false;
        if before.is_some() {
            self.casters -= 1;
            self.record(before, None);
        }
    }

    /// Put a proxy where its bounds and draw state say it belongs in the culling index.
    fn place(&mut self, index: u32) {
        let proxy = &self.proxies[index as usize];
        let finite = proxy.bounds.min.is_finite() && proxy.bounds.max.is_finite();
        let (bounds, drawn, placement) = (proxy.bounds, proxy.drawn, proxy.placement);
        let placement = match (placement, drawn, finite) {
            (ProxyPlacement::Tree(leaf), true, true) => {
                self.tree.update(leaf, bounds);
                ProxyPlacement::Tree(leaf)
            }
            (current, true, true) => {
                self.displace(index, current);
                ProxyPlacement::Tree(self.tree.insert(bounds, index))
            }
            (ProxyPlacement::Unbounded, true, false) => ProxyPlacement::Unbounded,
            (current, true, false) => {
                self.displace(index, current);
                self.unbounded.push(index);
                ProxyPlacement::Unbounded
            }
            (current, false, _) => {
                self.displace(index, current);
                ProxyPlacement::Absent
            }
        };
        self.proxies[index as usize].placement = placement;
    }

    /// Take a proxy out of wherever it sits in the culling index.
    fn displace(&mut self, index: u32, placement: ProxyPlacement) {
        match placement {
            ProxyPlacement::Absent => {}
            ProxyPlacement::Tree(leaf) => self.tree.remove(leaf),
            ProxyPlacement::Unbounded => self.unbounded.retain(|item| *item != index),
        }
    }

    /// Append a caster change at this refresh's revision.
    fn record(&mut self, before: Option<Bounds>, after: Option<Bounds>) {
        self.journal.push(CasterChange {
            revision: self.revision,
            before,
            after,
        });
    }

    /// Drop journal entries older than [`JOURNAL_REVISIONS`] refreshes, and the oldest beyond [`JOURNAL_CAPACITY`].
    fn trim_journal(&mut self) {
        let oldest_kept = self.revision.saturating_sub(JOURNAL_REVISIONS);
        let overflow = self.journal.len().saturating_sub(JOURNAL_CAPACITY);
        let expired = self.journal.iter().take_while(|change| change.revision <= oldest_kept).count();
        let dropped = expired.max(overflow);
        if dropped == 0 {
            return;
        }
        self.journal_floor = self.journal_floor.max(self.journal[dropped - 1].revision);
        self.journal.drain(..dropped);
    }

    // --- Reads (frames) --------------------------------------------------------------------------------------------

    /// Increases with every refresh that applied marks; a renderer remembers it as its journal cursor.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Caster changes recorded after revision `cursor`, oldest first; `None` when some were already dropped, so the
    /// reader must treat every caster as changed.
    pub(crate) fn caster_changes_since(&self, cursor: u64) -> Option<&[CasterChange]> {
        if cursor < self.journal_floor {
            return None;
        }
        let start = self.journal.partition_point(|change| change.revision <= cursor);
        Some(&self.journal[start..])
    }

    /// Live proxies that cast shadows.
    pub(crate) fn caster_count(&self) -> usize {
        self.casters
    }

    /// The proxy of instance slot `index`, while that slot holds a live instance as of the last refresh.
    pub(crate) fn get(&self, index: u32) -> Option<&RenderProxy> {
        self.proxies.get(index as usize).filter(|proxy| proxy.live)
    }

    /// Marks waiting on slot `index` for the next refresh.
    #[cfg(test)]
    pub(crate) fn pending(&self, index: u32) -> ProxyChanges {
        self.proxies.get(index as usize).map_or(ProxyChanges::NONE, |proxy| proxy.pending)
    }

    /// Visit every drawn proxy whose bounds `classify` might not place outside a volume (a conservative, monotone test;
    /// see [`BoundsTree::query`]). The visit receives the slot index, the proxy, and whether its tree leaf lies wholly
    /// inside the volume (its exact bounds then lie inside by at least the leaf margin); otherwise it must apply its own
    /// exact test. Visits arrive in no particular order.
    pub(crate) fn query(
        &self,
        classify: impl Fn(&Bounds) -> Overlap,
        mut visit: impl FnMut(u32, &RenderProxy, Overlap),
    ) {
        self.tree.query(classify, |index, overlap| visit(index, &self.proxies[index as usize], overlap));
        for index in &self.unbounded {
            visit(*index, &self.proxies[*index as usize], Overlap::Partial);
        }
    }

    /// Drawn proxies: live, shown, and not fully faded.
    pub(crate) fn drawn_count(&self) -> usize {
        self.tree.len() + self.unbounded.len()
    }
}

/// Whether node `id` is pending or beneath a pending node, filling `affected` along its chain.
fn node_affected(scene: &RenderScene, id: SceneNodeId, affected: &mut [Option<bool>]) -> bool {
    // Walk up to the first node whose answer is known (or the root), then write the answer back down the chain.
    let mut chain = Vec::new();
    let mut current = Some(id);
    let mut answer = false;
    while let Some(node) = current {
        let Some(slot) = scene.node_slot(node) else {
            break;
        };
        if let Some(known) = affected[node.index as usize] {
            answer = known;
            break;
        }
        chain.push(node.index as usize);
        current = slot.parent;
    }
    for index in chain {
        affected[index] = Some(answer);
    }
    answer
}

/// Set what an instance draws as, from its state and whether it and every ancestor are `shown`.
fn apply_draw_policy(proxy: &mut RenderProxy, instance: &MeshInstance, shown: bool) {
    // The same conditions the renderer has always drawn and cast by: hidden or fully faded instances do neither, and
    // only opaque submeshes of opaque instances cast.
    proxy.shown = shown;
    proxy.drawn = shown && (instance.opacity > 0.0 || instance.opacity.is_nan());
    proxy.transparent = instance.is_transparent();
    let may_cast = proxy.drawn && instance.casts_shadow && !proxy.transparent;
    let mut casts = false;
    let mut masked = false;
    if may_cast {
        for submesh in instance.asset.submeshes() {
            let material = instance.material(submesh.slot);
            if !material.blends() {
                casts = true;
                masked |= material.masks();
            }
        }
    }
    proxy.casts = casts;
    proxy.masked = masked;
}

#[cfg(test)]
#[path = "proxy.test.rs"]
mod tests;
