// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared, immutable materials ready to draw.
//!
//! A [`MaterialAsset`] validates a [`Material`] once and resolves its texture sources to decoded images. Assets hold no
//! GPU resources: the renderer uploads a material's parameters and maps the first time a frame draws it and releases
//! them once every handle drops, like mesh assets. Any number of meshes and instances may share one asset.

use std::sync::Arc;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use joy_render_types::{
    Material,
    Shading,
    ShadingModel,
    SurfaceLayer,
};

use crate::RenderError;
use crate::texture::TextureAsset;

/// Source of process-unique material identities used to key GPU uploads and batches.
static NEXT_MATERIAL_ID: AtomicU64 = AtomicU64::new(1);

/// An immutable, cheaply cloned material shared by any number of meshes and instances.
#[derive(Clone, Debug)]
pub struct MaterialAsset {
    inner: Arc<MaterialInner>,
}

#[derive(Debug)]
struct MaterialInner {
    id: u64,
    material: Material,

    // Layered materials collapse their surface tree once, here, into the layer the model shades.
    layer: Option<SurfaceLayer>,

    // Decoded maps by the binding name the shading model samples them through.
    textures: Vec<(String, TextureAsset)>,
}

impl Default for MaterialAsset {
    /// The engine's standard look: opaque white lit shading.
    fn default() -> Self {
        Self::new(Material::default()).expect("The default material has no textures and valid values.")
    }
}

impl MaterialAsset {
    /// Validate a material that samples no textures.
    ///
    /// Fails for out-of-range values and for materials with texture sources (use [`MaterialAsset::with_textures`]).
    pub fn new(material: Material) -> Result<Self, RenderError> {
        Self::with_textures(material, |_| None)
    }

    /// Validate a material and resolve each texture source through `resolve`, which returns the decoded image (for
    /// example from a cache of embedded `data:` URLs) or `None` when the source cannot be loaded.
    ///
    /// Fails for out-of-range values and for any source `resolve` cannot load.
    pub fn with_textures(
        material: Material,
        mut resolve: impl FnMut(&str) -> Option<TextureAsset>,
    ) -> Result<Self, RenderError> {
        material.validate().map_err(|error| RenderError::InvalidMaterial(error.to_string()))?;

        let layer = match &material.shading {
            Shading::Layered(surface) => Some(surface.collapse()),
            _ => None,
        };

        // Resolve the sources the model actually samples: a layered material samples its collapsed layer's maps.
        let sources: Vec<(String, String)> = match &layer {
            Some(layer) => layer.maps.sources().map(|(name, source)| (name.to_owned(), source.clone())).collect(),
            None => {
                material.texture_sources().into_iter().map(|(name, source)| (name.to_owned(), source.clone())).collect()
            }
        };

        let mut textures = Vec::with_capacity(sources.len());
        for (binding, source) in sources {
            let Some(texture) = resolve(&source) else {
                return Err(RenderError::InvalidMaterial(format!(
                    "the {binding} texture \"{}\" could not be loaded",
                    abbreviated(&source)
                )));
            };

            textures.push((binding, texture));
        }

        let id = NEXT_MATERIAL_ID.fetch_add(1, Ordering::Relaxed);
        Ok(Self {
            inner: Arc::new(MaterialInner {
                id,
                material,
                layer,
                textures,
            }),
        })
    }

    /// Process-unique identity used to key GPU uploads and batches.
    pub fn id(&self) -> u64 {
        self.inner.id
    }

    /// The validated material record.
    pub fn material(&self) -> &Material {
        &self.inner.material
    }

    /// The shading model the material draws with.
    pub fn model(&self) -> ShadingModel<'_> {
        self.inner.material.model()
    }

    /// Whether the material draws in the sorted transparent queue.
    pub fn blends(&self) -> bool {
        self.inner.material.blends()
    }

    /// Whether depth-only passes alpha-test the material.
    pub fn masks(&self) -> bool {
        self.inner.material.masks()
    }

    /// The decoded image bound to a model's texture `binding`, if the material sets one.
    pub fn texture(&self, binding: &str) -> Option<&TextureAsset> {
        self.inner.textures.iter().find(|(name, _)| name == binding).map(|(_, texture)| texture)
    }

    /// Whether two handles refer to the same asset.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// The single layer a layered material's surface tree collapses into.
    pub(crate) fn surface_layer(&self) -> Option<&SurfaceLayer> {
        self.inner.layer.as_ref()
    }

    /// Every decoded image with its binding name.
    pub(crate) fn textures(&self) -> impl Iterator<Item = (&str, &TextureAsset)> {
        self.inner.textures.iter().map(|(name, texture)| (name.as_str(), texture))
    }

    /// A type-erased weak reference used by the GPU cache to release uploads for dropped materials.
    pub(crate) fn downgrade(&self) -> std::sync::Weak<dyn std::any::Any + Send + Sync> {
        let strong: Arc<dyn std::any::Any + Send + Sync> = self.inner.clone();
        Arc::downgrade(&strong)
    }
}

/// A texture source short enough for an error message (embedded `data:` URLs run to megabytes).
fn abbreviated(source: &str) -> String {
    const MAXIMUM_CHARACTERS: usize = 48;
    if source.chars().count() <= MAXIMUM_CHARACTERS {
        return source.to_owned();
    }
    let prefix: String = source.chars().take(MAXIMUM_CHARACTERS).collect();
    format!("{prefix}…")
}
