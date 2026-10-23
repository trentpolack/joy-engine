// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Decoded particle textures keyed by their embedded PNG source.
//!
//! The set owns CPU [`TextureAsset`]s only; the renderer
//! uploads each on first draw and releases the GPU copy when the asset is dropped, so the set never borrows a device.
//! Decoding is synchronous, so a prepare cannot be superseded mid-flight; the prepare/activate split remains so a new
//! definition's images are ready before it replaces the displayed one, and a failed decode leaves the set unchanged.
//! The textured shader is `shaders/textured.wgsl`, drawn through
//! [`FrameInputs::transparent_draws`](crate::FrameInputs).

use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::Arc;

use crate::{
    TextureAsset,
    TransparentDraw,
};

use joy_fx::{
    CompiledParticleEffect,
    ParticleTextureError,
    decode_particle_texture_source,
};

use crate::particles::ParticleBatch;

/// Decode one embedded PNG particle texture source into an image, for previews of textures that are not part of a
/// prepared definition yet (JOYFX EDITOR's sprite thumbnail).
pub fn decode_particle_texture(source: &str) -> Result<TextureAsset, ParticleTextureError> {
    ParticleTextureSet::decode(source)
}

/// Decoded images for the textured emitters of one or more effect definitions.
#[derive(Clone, Debug, Default)]
pub struct ParticleTextureSet {
    textures: HashMap<Arc<str>, TextureAsset>,
    active: HashSet<Arc<str>>,
}

impl ParticleTextureSet {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Decode every image `definition`'s textured emitters need, keeping unchanged images and the active ones.
    ///
    /// Atomic: on failure nothing changes and the displayed definition stays drawable.
    pub fn prepare(&mut self, definition: &CompiledParticleEffect) -> Result<(), ParticleTextureError> {
        self.prepare_sources(textured_sources(definition))
    }

    /// Commit `definition` (or none) as the displayed one and release images it no longer draws.
    pub fn activate(&mut self, definition: Option<&CompiledParticleEffect>) {
        self.activate_sources(definition.map(textured_sources).unwrap_or_default());
    }

    /// Borrow the image for `source` after a successful prepare.
    pub fn get(&self, source: &str) -> Result<&TextureAsset, ParticleTextureError> {
        self.textures.get(source).ok_or(ParticleTextureError::NotReady)
    }

    /// Number of decoded images held.
    pub fn len(&self) -> usize {
        self.textures.len()
    }

    /// Whether no images are held.
    pub fn is_empty(&self) -> bool {
        self.textures.is_empty()
    }

    /// Release every image; GPU copies are released after the renderer's next frame.
    pub fn clear(&mut self) {
        self.textures.clear();
        self.active.clear();
    }

    /// Borrow renderer draws for `batches`, in order; fails if a textured batch's image was not prepared.
    pub fn transparent_draws<'a>(
        &'a self,
        batches: &'a [ParticleBatch],
    ) -> Result<Vec<TransparentDraw<'a>>, ParticleTextureError> {
        batches
            .iter()
            .map(|batch| match batch {
                ParticleBatch::Colored(vertices) => Ok(TransparentDraw::Colored(vertices)),
                ParticleBatch::Textured {
                    source,
                    vertices,
                } => Ok(TransparentDraw::Textured {
                    texture: self.get(source)?,
                    vertices,
                }),
            })
            .collect()
    }

    /// A set holding exactly `textures`, all marked active.
    #[doc(hidden)]
    pub fn from_textures(textures: HashMap<Arc<str>, TextureAsset>) -> Self {
        let active = textures.keys().cloned().collect();
        Self {
            textures,
            active,
        }
    }

    /// Decode one embedded PNG source.
    #[doc(hidden)]
    pub fn decode(source: &str) -> Result<TextureAsset, ParticleTextureError> {
        let bytes = decode_particle_texture_source(source)?;
        TextureAsset::decode_png(&bytes).map_err(|error| ParticleTextureError::Decode(error.to_string()))
    }

    /// Decode any of `sources` not yet held; keep held images that are requested or active and drop the rest.
    pub(crate) fn prepare_sources(&mut self, sources: HashSet<Arc<str>>) -> Result<(), ParticleTextureError> {
        let mut next = HashMap::with_capacity(sources.len());
        for source in sources {
            if let Some(existing) = self.textures.get(&source) {
                next.insert(source, existing.clone());
                continue;
            }
            let texture = Self::decode(&source)?;
            next.insert(source, texture);
        }
        // Keep the displayed definition drawable until its owner activates the prepared replacement.
        for (source, texture) in &self.textures {
            if self.active.contains(source) {
                next.entry(source.clone()).or_insert_with(|| texture.clone());
            }
        }
        self.textures = next;
        Ok(())
    }

    /// Mark exactly `sources` as displayed and release every other image.
    pub(crate) fn activate_sources(&mut self, sources: HashSet<Arc<str>>) {
        self.textures.retain(|source, _| sources.contains(source));
        self.active = sources;
    }
}

/// Sources of a definition's textured and flipbook emitters (textures on other renderers are never drawn).
#[doc(hidden)]
pub fn textured_sources(definition: &CompiledParticleEffect) -> HashSet<Arc<str>> {
    definition
        .emitters()
        .iter()
        .filter(|emitter| emitter.asset.renderer.is_textured())
        .filter_map(|emitter| emitter.asset.texture.as_ref().map(|texture| texture.source.clone()))
        .collect()
}
