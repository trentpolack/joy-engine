// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene batching: turns the scene's render proxies into the frame's mesh draw items.
//!
//! The camera queries the scene's bounds tree for the instances it sees; their submeshes become opaque batches (one
//! draw per model, material, asset, and submesh, nearest instance first, batches ordered front to back) and blended
//! items for the transparent queue. Each shadow view that renders this frame gets its casters' batches the same way,
//! from the caster slots the shadow planner found for it. Items borrow nothing from the scene: they carry identities
//! and index ranges, and their instance records go into the frame's instance list, so the batcher reuses its scratch
//! lists every frame.
//!
//! Instances keep the order the scene iterates them in wherever ties would otherwise decide what draws first, so the
//! frame draws exactly what a linear scan would.

use std::collections::HashMap;
use std::ops::Range;

use joy_core::math::{
    Mat4,
    Vec3,
};

use crate::camera::CameraFrame;
use crate::culling::{
    ClipPlanes,
    Overlap,
    bounds_outside_view,
    outside_frustum,
};
use crate::draw_items::{
    DrawGeometry,
    DrawItem,
};
use crate::material::{
    MaterialCache,
    ModelId,
};
use crate::mesh::Bounds;
use crate::pipelines::InstanceRaw;
use crate::scene::{
    MeshInstance,
    MeshInstanceId,
    RenderProxy,
    RenderScene,
    SceneProxies,
};

/// Stencil values are eight bits and zero marks pixels no deferred surface covers, so models past this registry index
/// shade forward.
pub(crate) const MAX_DEFERRED_MODEL: ModelId = 254;

/// Distances along the camera's forward axis, for ordering opaque batches front to back and transparency back to front.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ViewDepth {
    pub eye: Vec3,
    pub forward: Vec3,
}

impl ViewDepth {
    pub(crate) fn new(camera: &CameraFrame) -> Self {
        // The view matrix's third row is the camera's backward axis in world space.
        let backward = camera.view.row(2).truncate();
        Self {
            eye: camera.eye,
            forward: (-backward).try_normalize().unwrap_or(Vec3::NEG_Z),
        }
    }

    /// Distance of `point` in front of the camera.
    pub(crate) fn of(self, point: Vec3) -> f32 {
        (point - self.eye).dot(self.forward)
    }

    /// View depth of the nearest point of the sphere around `bounds`.
    fn nearest(self, bounds: Bounds) -> f32 {
        self.of(bounds.center()) - ((bounds.max - bounds.min).length() * 0.5)
    }
}

/// One camera-visible blended submesh, waiting to be sorted with the frame's other transparency.
#[derive(Clone, Debug)]
pub(crate) struct BlendDraw {
    /// View depth of the instance's bounds center.
    pub depth: f32,
    /// The draw, without its instance range (the transparent queue's order decides it).
    pub item: DrawItem,
    pub raw: InstanceRaw,
}

/// What the camera draws from the scene this frame.
#[derive(Debug, Default)]
pub(crate) struct CameraBatches {
    /// Opaque mesh batches, front to back by their nearest instance.
    pub opaque: Vec<DrawItem>,
    /// Blended submeshes, in scene order.
    pub blends: Vec<BlendDraw>,
    /// This frame's model matrix of each camera-visible instance, kept as motion history when effects run.
    pub models: Vec<(MeshInstanceId, Mat4)>,
    /// Instances inside the camera frustum.
    pub visible: usize,
    /// Camera-visible submeshes left out because their material names a shading model nothing registered.
    pub unavailable: usize,
}

/// One opaque submesh waiting to be grouped into a batch: by the camera, or by a shadow view. Kept small, since
/// batching sorts thousands of them; the instance record it draws waits in the batcher's record list.
#[derive(Clone, Debug)]
struct PendingDraw {
    /// `(model, material, asset, submesh)` for camera batches; casters zero the model and material when unmasked.
    key: (ModelId, u64, u64, usize),
    masked: bool,
    deferred: bool,
    indices: Range<u32>,
    /// The instance's record in [`SceneBatcher::records`].
    record: u32,
    /// View depth of the nearest point of the instance's bounding sphere (camera batches only).
    depth: f32,
}

/// The renderer's batcher; owns only scratch lists reused from frame to frame.
#[derive(Debug, Default)]
pub(crate) struct SceneBatcher {
    /// Camera-visible proxy slots, ascending (scene order).
    visible: Vec<u32>,
    pending: Vec<PendingDraw>,
    /// One instance record per instance the pending draws come from, shared by its submeshes.
    records: Vec<InstanceRaw>,
}

impl SceneBatcher {
    /// Find the instances the camera's `view_projection` sees, by bounds tree, in scene order; returns how many drawn
    /// instances lie outside it. Each candidate is tested as the camera always tested instances: its local bounds
    /// through its world matrix, which culls tighter than its world box.
    pub(crate) fn cull_camera(&mut self, proxies: &SceneProxies, scene: &RenderScene, view_projection: Mat4) -> usize {
        self.visible.clear();
        let visible = &mut self.visible;
        let planes = ClipPlanes::new(view_projection);
        proxies.query(
            |bounds| planes.classify(bounds),
            |index, proxy, overlap| {
                // A leaf wholly inside the view holds an instance inside it by the leaf's margin, which the exact test
                // would keep; the rest take it.
                let Some((_, instance)) = scene.instance_at(index) else {
                    return;
                };
                if overlap == Overlap::Inside || !outside_frustum(instance.asset.bounds(), proxy.world, view_projection)
                {
                    visible.push(index);
                }
            },
        );
        self.visible.sort_unstable();
        proxies.drawn_count() - self.visible.len()
    }

    /// Slots of the camera-visible instances, ascending.
    pub(crate) fn visible(&self) -> &[u32] {
        &self.visible
    }

    /// Batch the camera-visible instances' submeshes into opaque batches and blended items, appending opaque instance
    /// records to `raws`. Every visible material must already be prepared in `materials`; submeshes whose material
    /// names an unregistered model are counted and left out. With effect history, each instance carries its previous
    /// model matrix for motion vectors.
    pub(crate) fn batch_camera(
        &mut self,
        frame: CameraFrameInputs<'_>,
        raws: &mut Vec<InstanceRaw>,
        batches: &mut CameraBatches,
    ) {
        let CameraFrameInputs {
            scene,
            proxies,
            materials,
            view_depth,
            previous_models,
        } = frame;
        self.pending.clear();
        self.records.clear();
        batches.opaque.clear();
        batches.blends.clear();
        batches.models.clear();
        batches.unavailable = 0;
        batches.visible = self.visible.len();

        for index in &self.visible {
            let (Some((id, instance)), Some(proxy)) = (scene.instance_at(*index), proxies.get(*index)) else {
                continue;
            };
            let previous = previous_models.and_then(|models| models.get(&id)).copied().unwrap_or(proxy.world);
            if previous_models.is_some() {
                batches.models.push((id, proxy.world));
            }
            let raw = instance_raw(instance, proxy.world, previous);
            let record = self.records.len() as u32;
            self.records.push(raw);
            let center_depth = view_depth.of(proxy.bounds.center());
            let nearest_depth = view_depth.nearest(proxy.bounds);

            // Every submesh of a transparent instance blends; otherwise each submesh follows its material.
            for (submesh_index, submesh) in instance.asset.submeshes().iter().enumerate() {
                let material = instance.material(submesh.slot);
                let Some((model, _)) = materials.get(material.id()) else {
                    batches.unavailable += 1;
                    continue;
                };
                let geometry = DrawGeometry::Mesh {
                    asset: instance.asset.id(),
                    indices: submesh.indices.clone(),
                };
                if proxy.transparent || material.blends() {
                    batches.blends.push(BlendDraw {
                        depth: center_depth,
                        item: DrawItem {
                            geometry,
                            model,
                            material: material.id(),
                            instances: 0..0,
                            masked: false,
                            deferred: false,
                        },
                        raw,
                    });
                } else {
                    self.pending.push(PendingDraw {
                        key: (model, material.id(), instance.asset.id(), submesh_index),
                        masked: material.masks(),
                        deferred: materials.deferred(material.id()) && model <= MAX_DEFERRED_MODEL,
                        indices: submesh.indices.clone(),
                        record,
                        depth: nearest_depth,
                    });
                }
            }
        }

        // Items of one key share a batch, nearest instance first, and batches are ordered by their nearest instance,
        // so depth fills front to back and the surfaces behind fail the depth test before shading.
        self.pending.sort_by(|left, right| left.key.cmp(&right.key).then(left.depth.total_cmp(&right.depth)));
        let mut groups: Vec<&[PendingDraw]> = self.pending.chunk_by(|left, right| left.key == right.key).collect();
        groups.sort_by(|left, right| left[0].depth.total_cmp(&right[0].depth));
        for group in groups {
            batches.opaque.push(append_batch(group, &self.records, raws));
        }
    }

    /// Batch the opaque submeshes of the casters in `slots` (ascending) for one shadow view, appending their instance
    /// records to `raws`: unmasked casters share one draw per asset and submesh whatever their material, and masked
    /// ones also group by model and material. Returns the instances appended.
    pub(crate) fn batch_casters(
        &mut self,
        frame: &CasterFrameInputs<'_>,
        slots: &[u32],
        raws: &mut Vec<InstanceRaw>,
        items: &mut Vec<DrawItem>,
    ) -> usize {
        self.pending.clear();
        self.records.clear();
        for index in slots {
            let (Some((_, instance)), Some(proxy)) = (frame.scene.instance_at(*index), frame.proxies.get(*index))
            else {
                continue;
            };
            let record = self.records.len() as u32;
            self.records.push(instance_raw(instance, proxy.world, proxy.world));
            for (submesh_index, submesh) in instance.asset.submeshes().iter().enumerate() {
                let material = instance.material(submesh.slot);
                if material.blends() {
                    continue;
                }
                let Some((model, _)) = frame.materials.get(material.id()) else {
                    continue;
                };
                let masked = material.masks();
                let key = if masked {
                    (model, material.id(), instance.asset.id(), submesh_index)
                } else {
                    (0, 0, instance.asset.id(), submesh_index)
                };
                self.pending.push(PendingDraw {
                    key,
                    masked,
                    deferred: false,
                    indices: submesh.indices.clone(),
                    record,
                    depth: 0.0,
                });
            }
        }

        // Unmasked casters sort before masked ones; the sort is stable, so each draw keeps its casters in scene order.
        self.pending.sort_by_key(|draw| (draw.masked, draw.key));
        let start = raws.len();
        for group in self.pending.chunk_by(|left, right| (left.masked, left.key) == (right.masked, right.key)) {
            items.push(append_batch(group, &self.records, raws));
        }
        raws.len() - start
    }
}

/// What camera batching reads.
pub(crate) struct CameraFrameInputs<'a> {
    pub scene: &'a RenderScene,
    pub proxies: &'a SceneProxies,
    pub materials: &'a MaterialCache,
    pub view_depth: ViewDepth,
    /// Each instance's model matrix from the previous frame, while effects keep motion history.
    pub previous_models: Option<&'a HashMap<MeshInstanceId, Mat4>>,
}

/// What caster batching reads.
pub(crate) struct CasterFrameInputs<'a> {
    pub scene: &'a RenderScene,
    pub proxies: &'a SceneProxies,
    pub materials: &'a MaterialCache,
}

/// Every mesh asset and material the instances in `slots` draw with, for uploading before batching.
pub(crate) fn instance_assets<'a>(
    scene: &'a RenderScene,
    slots: impl Iterator<Item = u32> + 'a,
) -> impl Iterator<Item = (&'a crate::mesh::MeshAsset, &'a crate::material::MaterialAsset)> + 'a {
    slots.filter_map(|index| scene.instance_at(index)).flat_map(|(_, instance)| {
        instance.asset.submeshes().iter().map(move |submesh| (&instance.asset, instance.material(submesh.slot)))
    })
}

/// Whether a proxy casts into `view_projection`, by its world bounds.
pub(crate) fn casts_into_view(proxy: &RenderProxy, view_projection: Mat4) -> bool {
    proxy.casts && !bounds_outside_view(proxy.bounds, view_projection)
}

/// One batch over a group of pending draws that share a key, appending their instance records (from `records`).
fn append_batch(group: &[PendingDraw], records: &[InstanceRaw], raws: &mut Vec<InstanceRaw>) -> DrawItem {
    let start = raws.len() as u32;
    raws.extend(group.iter().map(|draw| records[draw.record as usize]));
    let first = &group[0];
    let (model, material, asset, _) = first.key;
    DrawItem {
        geometry: DrawGeometry::Mesh {
            asset,
            indices: first.indices.clone(),
        },
        model,
        material,
        instances: start..raws.len() as u32,
        masked: first.masked,
        deferred: first.deferred,
    }
}

/// Pack an instance's transform and its previous frame's, tint, opacity, and shadow flag.
pub(crate) fn instance_raw(instance: &MeshInstance, model: Mat4, previous_model: Mat4) -> InstanceRaw {
    InstanceRaw {
        model: model.to_cols_array_2d(),
        tint: instance.tint.with_alpha(instance.opacity.clamp(0.0, 1.0)).to_array(),
        flags: [if instance.receives_shadow { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
        previous_model: previous_model.to_cols_array_2d(),
    }
}
