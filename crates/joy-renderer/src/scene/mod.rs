// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Retained presentation scene: mesh instances, resident terrain instances, and transform nodes.
//!
//! Scenes hold no gameplay authority. Instances reference shared assets and own their presentation state (transform,
//! tint, opacity, visibility, pass, shading, and shadow policy). The renderer borrows a scene for each frame.
//!
//! Instances and nodes may be parented to transform-only [`SceneNode`]s:
//! their transform is local to the parent's world transform, and hiding a node hides everything beneath it. Removing
//! a node removes its whole subtree. Instances are leaves, so
//! parent several instances to one node to move them together.

mod terrain_instance;

use std::collections::HashSet;

use indexmap::IndexMap;
use joy_core::math::Mat4;
use joy_core::{
    LinearColor,
    Transform,
};

use crate::mesh::MeshAsset;
use crate::terrain::TerrainAsset;

pub use terrain_instance::{
    DEFAULT_TERRAIN_TOLERANCE,
    TerrainInstance,
};

/// Which queue an instance draws in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderPassKind {
    /// Transparent when the asset has vertex alpha or the instance opacity is below one; opaque otherwise.
    #[default]
    Auto,
    /// Always opaque.
    Opaque,
    /// Always transparent (unlit, blended, sorted back to front), even at full opacity.
    Transparent,
}

/// Surface shading model for an instance's opaque geometry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shading {
    /// Flat, double-sided vertex-color lighting; the engine's standard look.
    #[default]
    Lit,
    /// Metallic-roughness PBR using the asset's baked material factors and normals.
    Pbr,
    /// Vertex color without lighting.
    Unlit,
}

/// Presentation state for one drawn copy of an asset.
#[derive(Clone, Debug)]
pub struct MeshInstance {
    /// Shared geometry.
    pub asset: MeshAsset,
    /// World-space transform (scale, rotate X/Y/Z, translate).
    pub transform: Transform,
    /// Linear multiplier applied to vertex color; RGB may exceed one, and alpha is unused (see `opacity`).
    pub tint: LinearColor,
    /// Opacity multiplier in `[0, 1]`.
    pub opacity: f32,
    /// Hidden instances neither draw nor cast shadows.
    pub visible: bool,
    /// Queue selection.
    pub pass: RenderPassKind,
    /// Opaque shading model.
    pub shading: Shading,
    /// Whether opaque geometry writes into the directional shadow map.
    pub casts_shadow: bool,
    /// Whether lighting samples the directional shadow map.
    pub receives_shadow: bool,
    /// Surface roughness in `[0, 1]` that scene effects (reflections) read; 1 is fully rough.
    pub scene_roughness: f32,
    /// Scene node this instance inherits transform and visibility from; set through `RenderScene::set_parent`.
    pub(crate) parent: Option<SceneNodeId>,
}

impl MeshInstance {
    /// An identity-transformed, untinted, visible, lit instance that casts and receives shadows.
    pub fn new(asset: MeshAsset) -> Self {
        Self {
            asset,
            transform: Transform::IDENTITY,
            tint: LinearColor::WHITE,
            opacity: 1.0,
            visible: true,
            pass: RenderPassKind::Auto,
            shading: Shading::Lit,
            casts_shadow: true,
            receives_shadow: true,
            scene_roughness: 1.0,
            parent: None,
        }
    }

    /// Builder-style transform assignment.
    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    /// Builder-style tint assignment.
    pub fn with_tint(mut self, tint: LinearColor) -> Self {
        self.tint = tint;
        self
    }

    /// Builder-style shading assignment.
    pub fn with_shading(mut self, shading: Shading) -> Self {
        self.shading = shading;
        self
    }

    /// Whether this instance draws in the sorted transparent queue.
    pub fn is_transparent(&self) -> bool {
        match self.pass {
            RenderPassKind::Opaque => false,
            RenderPassKind::Transparent => true,
            RenderPassKind::Auto => self.opacity < 1.0 || self.asset.has_vertex_alpha(),
        }
    }
}

/// Stable handle to a scene instance; stale handles never alias a newer instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MeshInstanceId {
    index: u32,
    generation: u32,
}

/// Stable handle to a scene terrain instance; stale handles never alias a newer instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TerrainInstanceId {
    index: u32,
    generation: u32,
}

/// Stable handle to a scene node; stale handles never alias a newer node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SceneNodeId {
    index: u32,
    generation: u32,
}

/// A transform-only node. Children's transforms are local to its world transform, and hiding it hides them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneNode {
    /// Transform relative to the parent node, or to the world for a root node.
    pub transform: Transform,
    /// Hidden nodes hide their whole subtree.
    pub visible: bool,
}

impl Default for SceneNode {
    fn default() -> Self {
        Self { transform: Transform::IDENTITY, visible: true }
    }
}

/// A scene-graph edit was refused; the scene is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SceneGraphError {
    /// The parent is not a live node of this scene.
    #[error("Parent must be a live node of this scene.")]
    UnknownParent,
    /// The node or instance being edited is not live.
    #[error("The scene node or instance is not live.")]
    UnknownTarget,
    /// The parent is the node itself or one of its descendants.
    #[error("Scene parenting cannot create a cycle.")]
    Cycle,
}

struct Slot<T> {
    generation: u32,
    instance: Option<T>,
}

struct NodeSlot {
    generation: u32,
    node: Option<SceneNode>,
    parent: Option<SceneNodeId>,
}

/// Flat retained collection of mesh and terrain instances, each iterated in creation-slot order.
#[derive(Default)]
pub struct RenderScene {
    slots: Vec<Slot<MeshInstance>>,
    free: Vec<u32>,
    live: usize,
    terrain_slots: Vec<Slot<TerrainInstance>>,
    terrain_free: Vec<u32>,
    node_slots: Vec<NodeSlot>,
    node_free: Vec<u32>,
}

impl RenderScene {
    /// An empty scene.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an instance and return its handle.
    pub fn insert(&mut self, instance: MeshInstance) -> MeshInstanceId {
        self.live += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.instance = Some(instance);
            return MeshInstanceId { index, generation: slot.generation };
        }
        self.slots.push(Slot { generation: 0, instance: Some(instance) });
        MeshInstanceId { index: (self.slots.len() - 1) as u32, generation: 0 }
    }

    /// Convenience: add an untransformed instance of `asset`.
    pub fn create_mesh(&mut self, asset: MeshAsset) -> MeshInstanceId {
        self.insert(MeshInstance::new(asset))
    }

    /// Borrow a live instance.
    pub fn get(&self, id: MeshInstanceId) -> Option<&MeshInstance> {
        self.slots.get(id.index as usize).filter(|slot| slot.generation == id.generation)?.instance.as_ref()
    }

    /// Mutably borrow a live instance.
    pub fn get_mut(&mut self, id: MeshInstanceId) -> Option<&mut MeshInstance> {
        self.slots.get_mut(id.index as usize).filter(|slot| slot.generation == id.generation)?.instance.as_mut()
    }

    /// Remove an instance; returns whether the handle was live. The handle and its slot generation are retired.
    pub fn remove(&mut self, id: MeshInstanceId) -> bool {
        let Some(slot) = self.slots.get_mut(id.index as usize) else {
            return false;
        };
        if slot.generation != id.generation || slot.instance.is_none() {
            return false;
        }
        slot.instance = None;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(id.index);
        self.live -= 1;
        true
    }

    /// Remove every mesh and terrain instance; the scene stays usable and outstanding handles become stale.
    pub fn clear(&mut self) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.instance.take().is_some() {
                slot.generation = slot.generation.wrapping_add(1);
                self.free.push(index as u32);
            }
        }
        self.live = 0;
        for (index, slot) in self.terrain_slots.iter_mut().enumerate() {
            if slot.instance.take().is_some() {
                slot.generation = slot.generation.wrapping_add(1);
                self.terrain_free.push(index as u32);
            }
        }
        for (index, slot) in self.node_slots.iter_mut().enumerate() {
            if slot.node.take().is_some() {
                slot.generation = slot.generation.wrapping_add(1);
                slot.parent = None;
                self.node_free.push(index as u32);
            }
        }
    }

    /// Number of live mesh instances.
    pub fn len(&self) -> usize {
        self.live
    }

    /// Whether the scene has no instances.
    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Live instances with their handles, in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (MeshInstanceId, &MeshInstance)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.instance
                .as_ref()
                .map(|instance| (MeshInstanceId { index: index as u32, generation: slot.generation }, instance))
        })
    }

    /// Add a resident terrain instance and return its handle. The renderer creates its GPU resources on first draw.
    pub fn insert_terrain(&mut self, instance: TerrainInstance) -> TerrainInstanceId {
        if let Some(index) = self.terrain_free.pop() {
            let slot = &mut self.terrain_slots[index as usize];
            slot.instance = Some(instance);
            return TerrainInstanceId { index, generation: slot.generation };
        }
        self.terrain_slots.push(Slot { generation: 0, instance: Some(instance) });
        TerrainInstanceId { index: (self.terrain_slots.len() - 1) as u32, generation: 0 }
    }

    /// Convenience: add an untransformed terrain instance of `asset` with default LOD policy.
    pub fn create_terrain(&mut self, asset: TerrainAsset) -> TerrainInstanceId {
        self.insert_terrain(TerrainInstance::new(asset))
    }

    /// Borrow a live terrain instance.
    pub fn terrain(&self, id: TerrainInstanceId) -> Option<&TerrainInstance> {
        self.terrain_slots.get(id.index as usize).filter(|slot| slot.generation == id.generation)?.instance.as_ref()
    }

    /// Mutably borrow a live terrain instance.
    pub fn terrain_mut(&mut self, id: TerrainInstanceId) -> Option<&mut TerrainInstance> {
        self.terrain_slots.get_mut(id.index as usize).filter(|slot| slot.generation == id.generation)?.instance.as_mut()
    }

    /// Remove a terrain instance; returns whether the handle was live. The renderer releases its GPU resources on the
    /// next frame.
    pub fn remove_terrain(&mut self, id: TerrainInstanceId) -> bool {
        let Some(slot) = self.terrain_slots.get_mut(id.index as usize) else {
            return false;
        };
        if slot.generation != id.generation || slot.instance.is_none() {
            return false;
        }
        slot.instance = None;
        slot.generation = slot.generation.wrapping_add(1);
        self.terrain_free.push(id.index);
        true
    }

    /// Live terrain instances with their handles, in slot order.
    pub fn terrains(&self) -> impl Iterator<Item = (TerrainInstanceId, &TerrainInstance)> {
        self.terrain_slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.instance
                .as_ref()
                .map(|instance| (TerrainInstanceId { index: index as u32, generation: slot.generation }, instance))
        })
    }

    /// Number of live terrain instances.
    pub fn terrain_count(&self) -> usize {
        self.terrain_slots.iter().filter(|slot| slot.instance.is_some()).count()
    }

    /// Add a transform node under `parent` (or at the root) and return its handle.
    pub fn insert_node(
        &mut self,
        node: SceneNode,
        parent: Option<SceneNodeId>,
    ) -> Result<SceneNodeId, SceneGraphError> {
        if parent.is_some_and(|parent| self.node_slot(parent).is_none()) {
            return Err(SceneGraphError::UnknownParent);
        }
        if let Some(index) = self.node_free.pop() {
            let slot = &mut self.node_slots[index as usize];
            slot.node = Some(node);
            slot.parent = parent;
            return Ok(SceneNodeId { index, generation: slot.generation });
        }
        self.node_slots.push(NodeSlot { generation: 0, node: Some(node), parent });
        Ok(SceneNodeId { index: (self.node_slots.len() - 1) as u32, generation: 0 })
    }

    /// Borrow a live node.
    pub fn node(&self, id: SceneNodeId) -> Option<&SceneNode> {
        self.node_slot(id)?.node.as_ref()
    }

    /// Mutably borrow a live node.
    pub fn node_mut(&mut self, id: SceneNodeId) -> Option<&mut SceneNode> {
        self.node_slots.get_mut(id.index as usize).filter(|slot| slot.generation == id.generation)?.node.as_mut()
    }

    /// A live node's parent.
    pub fn node_parent(&self, id: SceneNodeId) -> Option<SceneNodeId> {
        self.node_slot(id)?.parent
    }

    /// Reparent a node, keeping its local transform. Cycles and dead handles are refused without changes.
    pub fn set_node_parent(&mut self, id: SceneNodeId, parent: Option<SceneNodeId>) -> Result<(), SceneGraphError> {
        if self.node_slot(id).is_none() {
            return Err(SceneGraphError::UnknownTarget);
        }
        let mut ancestor = parent;
        while let Some(current) = ancestor {
            if current == id {
                return Err(SceneGraphError::Cycle);
            }
            ancestor = self.node_slot(current).ok_or(SceneGraphError::UnknownParent)?.parent;
        }
        self.node_slots[id.index as usize].parent = parent;
        Ok(())
    }

    /// Remove a node and its whole subtree (descendant nodes and every instance beneath them); returns whether the
    /// handle was live.
    pub fn remove_node(&mut self, id: SceneNodeId) -> bool {
        if self.node_slot(id).is_none() {
            return false;
        }
        let removed: HashSet<SceneNodeId> = self.subtree(id);
        let doomed: Vec<MeshInstanceId> = self
            .iter()
            .filter(|(_, instance)| instance.parent.is_some_and(|parent| removed.contains(&parent)))
            .map(|(id, _)| id)
            .collect();
        for instance in doomed {
            self.remove(instance);
        }
        let doomed: Vec<TerrainInstanceId> = self
            .terrains()
            .filter(|(_, instance)| instance.parent.is_some_and(|parent| removed.contains(&parent)))
            .map(|(id, _)| id)
            .collect();
        for terrain in doomed {
            self.remove_terrain(terrain);
        }
        for node in removed {
            let slot = &mut self.node_slots[node.index as usize];
            slot.node = None;
            slot.parent = None;
            slot.generation = slot.generation.wrapping_add(1);
            self.node_free.push(node.index);
        }
        true
    }

    /// Parent a mesh instance to a node (or detach it with `None`), keeping its local transform.
    pub fn set_parent(&mut self, id: MeshInstanceId, parent: Option<SceneNodeId>) -> Result<(), SceneGraphError> {
        if parent.is_some_and(|parent| self.node_slot(parent).is_none()) {
            return Err(SceneGraphError::UnknownParent);
        }
        self.get_mut(id).ok_or(SceneGraphError::UnknownTarget)?.parent = parent;
        Ok(())
    }

    /// A mesh instance's parent node.
    pub fn parent(&self, id: MeshInstanceId) -> Option<SceneNodeId> {
        self.get(id)?.parent
    }

    /// Parent a terrain instance to a node (or detach it with `None`), keeping its local transform.
    pub fn set_terrain_parent(
        &mut self,
        id: TerrainInstanceId,
        parent: Option<SceneNodeId>,
    ) -> Result<(), SceneGraphError> {
        if parent.is_some_and(|parent| self.node_slot(parent).is_none()) {
            return Err(SceneGraphError::UnknownParent);
        }
        self.terrain_mut(id).ok_or(SceneGraphError::UnknownTarget)?.parent = parent;
        Ok(())
    }

    /// A terrain instance's parent node.
    pub fn terrain_parent(&self, id: TerrainInstanceId) -> Option<SceneNodeId> {
        self.terrain(id)?.parent
    }

    /// A mesh instance's world matrix, including every ancestor node.
    pub fn world_matrix(&self, id: MeshInstanceId) -> Option<Mat4> {
        let instance = self.get(id)?;
        Some(self.compose(instance.parent, instance.transform.matrix(), instance.visible).0)
    }

    /// Whether a mesh instance and every ancestor node are visible.
    pub fn world_visible(&self, id: MeshInstanceId) -> Option<bool> {
        let instance = self.get(id)?;
        Some(self.compose(instance.parent, Mat4::IDENTITY, instance.visible).1)
    }

    /// A node's world matrix, including every ancestor node.
    pub fn node_world_matrix(&self, id: SceneNodeId) -> Option<Mat4> {
        let node = self.node(id)?;
        Some(self.compose(self.node_parent(id), node.transform.matrix(), node.visible).0)
    }

    /// World matrix and inherited visibility for a local transform beneath `parent`.
    pub(crate) fn compose(&self, parent: Option<SceneNodeId>, local: Mat4, visible: bool) -> (Mat4, bool) {
        let mut matrix = local;
        let mut visible = visible;
        let mut current = parent;
        while let Some(id) = current {
            let Some(slot) = self.node_slot(id) else {
                break;
            };
            if let Some(node) = &slot.node {
                matrix = node.transform.matrix() * matrix;
                visible &= node.visible;
            }
            current = slot.parent;
        }
        (matrix, visible)
    }

    fn node_slot(&self, id: SceneNodeId) -> Option<&NodeSlot> {
        self.node_slots.get(id.index as usize).filter(|slot| slot.generation == id.generation && slot.node.is_some())
    }

    /// A node and all of its live descendant nodes.
    fn subtree(&self, root: SceneNodeId) -> HashSet<SceneNodeId> {
        let mut members = HashSet::from([root]);
        let mut changed = true;
        while changed {
            changed = false;
            for (index, slot) in self.node_slots.iter().enumerate() {
                let id = SceneNodeId { index: index as u32, generation: slot.generation };
                if slot.node.is_some()
                    && !members.contains(&id)
                    && slot.parent.is_some_and(|parent| members.contains(&parent))
                {
                    members.insert(id);
                    changed = true;
                }
            }
        }
        members
    }
}

/// Keyed synchronization of instances with a changing set of simulation records.
///
/// Call [`begin_update`](Self::begin_update), [`set`](Self::set) once per live record, then
/// [`end_update`](Self::end_update): keys that were not set are removed from the scene, so despawned records cannot
/// grow it. A key returns the same instance across updates. Prefer stable IDs over array indices as keys.
#[derive(Default)]
pub struct MeshCollection {
    instances: IndexMap<String, MeshInstanceId>,
    touched: HashSet<String>,
}

impl MeshCollection {
    /// An empty collection.
    pub fn new() -> Self {
        Self::default()
    }

    /// Start an update pass.
    pub fn begin_update(&mut self) {
        self.touched.clear();
    }

    /// Create or update the instance for `key` and return a mutable borrow for further edits.
    ///
    /// An instance whose asset changed keeps its handle; only the asset reference is replaced.
    pub fn set<'scene>(
        &mut self,
        scene: &'scene mut RenderScene,
        key: &str,
        asset: &MeshAsset,
    ) -> &'scene mut MeshInstance {
        self.touched.insert(key.to_owned());
        let existing = self.instances.get(key).copied().filter(|id| scene.get(*id).is_some());
        let id = match existing {
            Some(id) => id,
            None => {
                let id = scene.create_mesh(asset.clone());
                self.instances.insert(key.to_owned(), id);
                id
            }
        };
        let instance = scene.get_mut(id).expect("collection handles are created in this scene and checked above");
        if !instance.asset.ptr_eq(asset) {
            instance.asset = asset.clone();
        }
        instance
    }

    /// Finish an update pass, removing instances whose keys were not set.
    pub fn end_update(&mut self, scene: &mut RenderScene) {
        let touched = &self.touched;
        self.instances.retain(|key, id| {
            let keep = touched.contains(key);
            if !keep {
                scene.remove(*id);
            }
            keep
        });
    }

    /// Remove every instance owned by this collection.
    pub fn clear(&mut self, scene: &mut RenderScene) {
        for id in self.instances.values() {
            scene.remove(*id);
        }
        self.instances.clear();
        self.touched.clear();
    }

    /// Number of keyed instances.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Whether the collection is empty.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
