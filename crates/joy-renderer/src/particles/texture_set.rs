// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Decoded particle textures keyed by their compact [`ParticleTextureId`].
//!
//! The set owns CPU [`TextureAsset`]s only; the renderer uploads each on first draw and releases the GPU copy when the
//! asset is dropped, so the set never borrows a device. Decoding is synchronous, so a prepare cannot be superseded
//! mid-flight; the prepare/activate split remains so a new definition's images are ready before it replaces the
//! displayed one, and a failed decode leaves the set unchanged. Per-frame lookups ([`ParticleFrame::add_effect`]) use
//! the 64-bit id each compiled emitter carries; the source text is compared only when an image is first decoded.
//!
//! [`ParticleFrame::add_effect`]: crate::particles::ParticleFrame::add_effect

use std::collections::{
    HashMap,
    HashSet,
};
use std::sync::Arc;

use crate::TextureAsset;

use joy_fx::{
    CompiledParticleEffect,
    ParticleTextureError,
    ParticleTextureId,
    decode_particle_texture_source,
};

/// Decode one embedded PNG particle texture source into an image, for previews of textures that are not part of a
/// prepared definition yet (JOYFX EDITOR's sprite thumbnail).
pub fn decode_particle_texture(source: &str) -> Result<TextureAsset, ParticleTextureError> {
    ParticleTextureSet::decode(source)
}

/// One decoded image and the source text it came from (kept to confirm ids when another definition is prepared).
#[derive(Clone, Debug)]
struct DecodedTexture {
    source: Arc<str>,
    texture: TextureAsset,
}

/// Decoded images for the textured emitters of one or more effect definitions.
#[derive(Clone, Debug, Default)]
pub struct ParticleTextureSet {
    textures: HashMap<ParticleTextureId, DecodedTexture>,
    active: HashSet<ParticleTextureId>,
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
        let sources = definition.map(textured_sources).unwrap_or_default();
        self.textures.retain(|id, _| sources.contains_key(id));
        self.active = sources.into_keys().collect();
    }

    /// Add decoded textures by source text, keeping any image already present; they count as active.
    pub fn insert_textures(&mut self, textures: impl IntoIterator<Item = (Arc<str>, TextureAsset)>) {
        for (source, texture) in textures {
            let id = ParticleTextureId::of(&source);
            self.active.insert(id);
            self.textures.entry(id).or_insert(DecodedTexture {
                source,
                texture,
            });
        }
    }

    /// Borrow the image for a compiled emitter's texture `id` after a successful prepare.
    pub fn get(&self, id: ParticleTextureId) -> Option<&TextureAsset> {
        self.textures.get(&id).map(|decoded| &decoded.texture)
    }

    /// Borrow the image decoded from `source` text, if held.
    pub fn get_source(&self, source: &str) -> Option<&TextureAsset> {
        self.textures
            .get(&ParticleTextureId::of(source))
            .filter(|decoded| decoded.source.as_ref() == source)
            .map(|decoded| &decoded.texture)
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

    /// Decode one embedded PNG source.
    #[doc(hidden)]
    pub fn decode(source: &str) -> Result<TextureAsset, ParticleTextureError> {
        let bytes = decode_particle_texture_source(source)?;
        TextureAsset::decode_png(&bytes).map_err(|error| ParticleTextureError::Decode(error.to_string()))
    }

    /// Decode any of `sources` not yet held; keep held images that are requested or active and drop the rest.
    fn prepare_sources(&mut self, sources: HashMap<ParticleTextureId, Arc<str>>) -> Result<(), ParticleTextureError> {
        let mut next = HashMap::with_capacity(sources.len());
        for (id, source) in sources {
            match self.textures.get(&id) {
                Some(existing) if existing.source == source => {
                    next.insert(id, existing.clone());
                }
                // Two different sources hashed to one id: refuse rather than draw the wrong image.
                Some(_) => return Err(ParticleTextureError::Decode("particle texture ids collided".to_owned())),
                None => {
                    let texture = Self::decode(&source)?;
                    next.insert(
                        id,
                        DecodedTexture {
                            source,
                            texture,
                        },
                    );
                }
            }
        }
        // Keep the displayed definition drawable until its owner activates the prepared replacement.
        for (id, decoded) in &self.textures {
            if self.active.contains(id) {
                next.entry(*id).or_insert_with(|| decoded.clone());
            }
        }
        self.textures = next;
        Ok(())
    }
}

/// Texture ids and sources of a definition's textured and flipbook emitters (textures on other renderers are never
/// drawn).
#[doc(hidden)]
pub fn textured_sources(definition: &CompiledParticleEffect) -> HashMap<ParticleTextureId, Arc<str>> {
    definition
        .emitters()
        .iter()
        .filter(|emitter| emitter.asset.renderer.is_textured())
        .filter_map(|emitter| Some((emitter.texture_id?, emitter.asset.texture.as_ref()?.source.clone())))
        .collect()
}
