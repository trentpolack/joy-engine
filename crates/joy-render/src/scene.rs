// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Retained presentation scene: a flat collection of mesh instances.
//!
//! Scenes hold no gameplay authority. Instances reference shared assets and own their presentation state (transform,
//! tint, opacity, visibility, pass, shading, and shadow policy). The renderer borrows a scene for each frame.

use std::collections::HashSet;

use indexmap::IndexMap;
use joy_foundation::{Rgb, Transform};

use crate::mesh::MeshAsset;

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
    /// Linear RGB multiplier applied to vertex color.
    pub tint: Rgb,
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
}

impl MeshInstance {
    /// An identity-transformed, untinted, visible, lit instance that casts and receives shadows.
    pub fn new(asset: MeshAsset) -> Self {
        Self {
            asset,
            transform: Transform::IDENTITY,
            tint: [1.0, 1.0, 1.0],
            opacity: 1.0,
            visible: true,
            pass: RenderPassKind::Auto,
            shading: Shading::Lit,
            casts_shadow: true,
            receives_shadow: true,
        }
    }

    /// Builder-style transform assignment.
    pub fn with_transform(mut self, transform: Transform) -> Self {
        self.transform = transform;
        self
    }

    /// Builder-style tint assignment.
    pub fn with_tint(mut self, tint: Rgb) -> Self {
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

struct Slot {
    generation: u32,
    instance: Option<MeshInstance>,
}

/// Flat retained collection of mesh instances, iterated in creation-slot order.
#[derive(Default)]
pub struct RenderScene {
    slots: Vec<Slot>,
    free: Vec<u32>,
    live: usize,
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

    /// Remove every instance; the scene stays usable and outstanding handles become stale.
    pub fn clear(&mut self) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if slot.instance.take().is_some() {
                slot.generation = slot.generation.wrapping_add(1);
                self.free.push(index as u32);
            }
        }
        self.live = 0;
    }

    /// Number of live instances.
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
mod tests {
    use super::*;
    use crate::mesh::MeshChannels;

    fn asset() -> MeshAsset {
        MeshAsset::from_channels(MeshChannels {
            positions: &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            indices: &[0, 1, 2],
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn stale_handles_do_not_alias_new_instances() {
        let mut scene = RenderScene::new();
        let first = scene.create_mesh(asset());
        assert!(scene.remove(first));
        let second = scene.create_mesh(asset());
        assert!(scene.get(first).is_none());
        assert!(scene.get(second).is_some());
        assert!(!scene.remove(first));
        assert_eq!(scene.len(), 1);
    }

    #[test]
    fn collections_prune_missing_keys() {
        let mut scene = RenderScene::new();
        let mut collection = MeshCollection::new();
        let shared = asset();

        collection.begin_update();
        collection.set(&mut scene, "a", &shared).tint = [1.0, 0.0, 0.0];
        collection.set(&mut scene, "b", &shared);
        collection.end_update(&mut scene);
        assert_eq!(scene.len(), 2);

        collection.begin_update();
        let retained = collection.set(&mut scene, "a", &shared);
        assert_eq!(retained.tint, [1.0, 0.0, 0.0]);
        collection.end_update(&mut scene);
        assert_eq!(scene.len(), 1);
        assert_eq!(collection.len(), 1);
    }
}
