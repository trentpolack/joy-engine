// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`ParticleFrame`]: the one engine-owned way to submit particles for drawing.
//!
//! An owner keeps one frame, clears it at the start of each frame, and adds every effect it draws; the renderer
//! ([`FrameInputs::particles`](crate::FrameInputs::particles)) or the arcade triangle renderer
//! ([`TriangleFrame::particles`](crate::TriangleFrame::particles)) then culls, sorts, and draws it. Each effect becomes
//! a few batches, one per material (blend family and texture), each holding one compact instance per visible particle
//! and the world bounds the renderer culls against. Within a batch instances keep generation order; the renderer
//! orders straight and premultiplied batches back to front per particle and places each batch in its shared
//! transparency queue by one depth, so the frame itself never depends on the camera.
//!
//! The frame keeps its allocations between frames, and textures are referenced by compact ids, so a steady stream of
//! effects builds without per-frame or per-particle allocation.

use std::ops::Range;

use joy_core::math::{
    Mat4,
    Vec3,
};
use joy_fx::{
    ParticleBlend,
    ParticleEffect,
};

use crate::mesh::Bounds;
use crate::particles::instance::{
    EmitterDrawing,
    ParticleInstance,
};
use crate::particles::texture_set::ParticleTextureSet;
use crate::texture::TextureAsset;

/// Particles could not be added to a [`ParticleFrame`]. The frame is unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParticleRenderError {
    /// An effect with textured or flipbook emitters was added without a texture set.
    #[error("Textured particles require a ParticleTextureSet holding the effect's images.")]
    TexturedEffect,
    /// The texture set has not decoded an image a textured emitter samples; prepare the definition first.
    #[error("Particle image is not ready; prepare the ParticleTextureSet before drawing.")]
    TextureNotReady,
}

/// One drawable run: consecutive instances of one effect sharing a blend family and texture.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleBatch {
    instances: Range<u32>,
    additive: bool,
    texture: Option<u32>,
    bounds: Bounds,
}

impl ParticleBatch {
    /// The batch's instances in [`ParticleFrame::instances`].
    pub fn instances(&self) -> Range<u32> {
        self.instances.clone()
    }

    /// Whether the batch adds light (additive emitters) rather than compositing over the scene (straight and
    /// premultiplied alpha).
    pub fn is_additive(&self) -> bool {
        self.additive
    }

    /// The sampled image's index in [`ParticleFrame::textures`], for textured batches.
    pub fn texture(&self) -> Option<u32> {
        self.texture
    }

    /// World bounds containing everything the batch draws.
    pub fn bounds(&self) -> Bounds {
        self.bounds
    }
}

/// How one emitter's particles are grouped while an effect is added.
#[derive(Clone, Debug)]
struct EmitterSlot {
    drawing: EmitterDrawing,
    /// The emitter's group in the effect being added.
    group: usize,
}

/// One material group of the effect being added: its key, then the count and write cursor of its instances.
#[derive(Clone, Copy, Debug)]
struct Group {
    additive: bool,
    texture: Option<u32>,
    count: u32,
    cursor: u32,
}

/// A caller-owned, reusable list of particle instances and batches for one frame.
#[derive(Clone, Debug, Default)]
pub struct ParticleFrame {
    // This frame's submission.
    instances: Vec<ParticleInstance>,
    batches: Vec<ParticleBatch>,
    textures: Vec<TextureAsset>,

    // Per-effect scratch, kept so adding effects does not allocate once warm.
    emitters: Vec<EmitterSlot>,
    groups: Vec<Group>,
}

impl ParticleFrame {
    /// An empty frame.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove every effect, keeping allocations for the next frame. Texture handles are released here, so the renderer
    /// can free GPU copies the next frame no longer draws.
    pub fn clear(&mut self) {
        self.instances.clear();
        self.batches.clear();
        self.textures.clear();
    }

    /// Add `effect`'s visible particles in effect space (world space for untransformed effects).
    ///
    /// `textures` must hold the images of textured and flipbook emitters (see [`ParticleTextureSet::prepare`]); pass
    /// `None` for effects without them. Fails without changing the frame when a textured emitter's image is missing.
    pub fn add_effect(
        &mut self,
        effect: &ParticleEffect,
        textures: Option<&ParticleTextureSet>,
    ) -> Result<(), ParticleRenderError> {
        self.add(effect, textures, None)
    }

    /// Add `effect`'s visible particles placed by `transform` (effect space to world), such as an entity's transform.
    ///
    /// Positions and streak trails transform as points and vectors; sizes scale by the mean of the transform's axis
    /// scales, since billboards stay camera-facing (non-uniform scale moves particles but never stretches them).
    pub fn add_transformed_effect(
        &mut self,
        effect: &ParticleEffect,
        textures: Option<&ParticleTextureSet>,
        transform: &Mat4,
    ) -> Result<(), ParticleRenderError> {
        self.add(effect, textures, Some(transform))
    }

    /// Every instance, grouped by batch.
    pub fn instances(&self) -> &[ParticleInstance] {
        &self.instances
    }

    /// Batches in the order effects (and their materials) were added.
    pub fn batches(&self) -> &[ParticleBatch] {
        &self.batches
    }

    /// The images textured batches sample, by [`ParticleBatch::texture`].
    pub fn textures(&self) -> &[TextureAsset] {
        &self.textures
    }

    /// Whether nothing was added since the last [`clear`](Self::clear).
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Building
    // -----------------------------------------------------------------------------------------------------------------

    /// Group `effect`'s emitters by material, count each group's visible particles, then write the instances of every
    /// group contiguously in generation order (a counting sort, so nothing is allocated per particle).
    fn add(
        &mut self,
        effect: &ParticleEffect,
        textures: Option<&ParticleTextureSet>,
        transform: Option<&Mat4>,
    ) -> Result<(), ParticleRenderError> {
        let texture_count = self.textures.len();
        if let Err(error) = self.group_emitters(effect, textures) {
            self.textures.truncate(texture_count);
            return Err(error);
        }

        // Count each group's visible particles.
        for particle in effect.particles() {
            let Some(slot) = self.emitters.get(particle.emitter_index) else {
                continue;
            };
            if slot.drawing.is_visible(particle) {
                self.groups[slot.group].count += 1;
            }
        }

        // Lay the groups out one after another and size the instance list once.
        let start = self.instances.len() as u32;
        let mut offset = start;
        for group in &mut self.groups {
            group.cursor = offset;
            offset += group.count;
        }
        self.instances.resize(offset as usize, ParticleInstance::default());

        // Write instances at their group's cursor.
        let size_scale = transform.map_or(1.0, size_scale);
        for particle in effect.particles() {
            let Some(slot) = self.emitters.get(particle.emitter_index) else {
                continue;
            };
            if !slot.drawing.is_visible(particle) {
                continue;
            }
            let group = &mut self.groups[slot.group];
            self.instances[group.cursor as usize] = slot.drawing.instance(particle, transform, size_scale);
            group.cursor += 1;
        }

        // One batch per nonempty group, with its bounds.
        let mut first = start;
        for group in &self.groups {
            let range = first..(first + group.count);
            first = range.end;
            if range.is_empty() {
                continue;
            }
            let bounds = instance_bounds(&self.instances[range.start as usize..range.end as usize]);
            self.batches.push(ParticleBatch {
                instances: range,
                additive: group.additive,
                texture: group.texture,
                bounds,
            });
        }
        Ok(())
    }

    /// Resolve each emitter's drawing settings and material group for `effect`, registering textures with the frame.
    fn group_emitters(
        &mut self,
        effect: &ParticleEffect,
        textures: Option<&ParticleTextureSet>,
    ) -> Result<(), ParticleRenderError> {
        self.emitters.clear();
        self.groups.clear();
        for emitter in effect.definition().emitters() {
            let asset = &emitter.asset;
            let texture = if asset.renderer.is_textured() {
                let textures = textures.ok_or(ParticleRenderError::TexturedEffect)?;
                let image = emitter.texture_id.and_then(|id| textures.get(id));
                let (Some(image), Some(texture), Some(dimensions)) =
                    (image, &asset.texture, emitter.texture_dimensions)
                else {
                    return Err(ParticleRenderError::TextureNotReady);
                };
                Some((self.texture_slot(image), texture, dimensions))
            } else {
                None
            };

            let additive = asset.blend == ParticleBlend::Additive;
            let texture_slot = texture.as_ref().map(|(slot, _, _)| *slot);
            let group = self.group_for(additive, texture_slot);
            let atlas = texture.map(|(_, texture, dimensions)| (texture, dimensions));
            self.emitters.push(EmitterSlot {
                drawing: EmitterDrawing::new(asset.renderer, asset.blend, atlas),
                group,
            });
        }
        Ok(())
    }

    /// The effect's material group for a blend family and texture, created on first use.
    fn group_for(&mut self, additive: bool, texture: Option<u32>) -> usize {
        let existing = self.groups.iter().position(|group| group.additive == additive && group.texture == texture);
        existing.unwrap_or_else(|| {
            self.groups.push(Group {
                additive,
                texture,
                count: 0,
                cursor: 0,
            });
            self.groups.len() - 1
        })
    }

    /// The frame's index for `image`, registering it on first use (images are compared by identity).
    fn texture_slot(&mut self, image: &TextureAsset) -> u32 {
        if let Some(index) = self.textures.iter().position(|held| held.id() == image.id()) {
            return index as u32;
        }
        self.textures.push(image.clone());
        (self.textures.len() - 1) as u32
    }
}

/// The mean of `transform`'s axis scales: how much it scales particle sizes.
fn size_scale(transform: &Mat4) -> f32 {
    let axes = [transform.x_axis, transform.y_axis, transform.z_axis];
    axes.iter().map(|axis| axis.truncate().length()).sum::<f32>() / 3.0
}

/// World bounds containing every instance's drawn extent.
fn instance_bounds(instances: &[ParticleInstance]) -> Bounds {
    let mut bounds = Bounds {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };
    for instance in instances {
        let position = instance.position();
        let extent = Vec3::splat(instance.extent());
        bounds.min = bounds.min.min(position - extent);
        bounds.max = bounds.max.max(position + extent);
    }
    bounds
}
