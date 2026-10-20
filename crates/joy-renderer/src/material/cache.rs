// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! GPU copies of materials: each material's parameter uniform, uploaded maps, and bind group.
//!
//! A material gets one bind group (group 3 of its model's scene, depth pre-pass, and shadow caster pipelines) the first
//! time a frame draws it. Images upload once per color space and are shared between materials; bindings a material
//! leaves unset take the model's fallback (white, black, or a flat normal). Color maps upload as sRGB textures, so
//! sampling decodes them and their mip chains average linear light; data maps upload as UNORM and read linearly. Every
//! image gets a full mip chain and is sampled trilinearly with anisotropic filtering. Entries release once every handle
//! to their material drops.

use std::collections::HashMap;
use std::sync::Weak;

use wgpu::util::DeviceExt;

use crate::RenderError;
use crate::gpu::GpuContext;
use crate::material::MaterialAsset;
use crate::material::models::{
    ModelId,
    ShadingModels,
    TextureFallback,
};
use crate::texture::TextureAsset;
use crate::texture_mips::{
    MipmapGenerator,
    mipmapped_sampler,
};

/// Format of color maps, decoded from sRGB when sampled.
const COLOR_MAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Format of data maps, read as stored.
const DATA_MAP_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// An uploaded image, released once every handle to its asset drops.
struct UploadedTexture {
    view: wgpu::TextureView,
    _texture: wgpu::Texture,
    asset: Weak<dyn std::any::Any + Send + Sync>,
}

/// A material's GPU copy, released once every handle to the material drops.
struct MaterialEntry {
    model: ModelId,
    /// Whether opaque surfaces with this material shade in the deferred passes when they run.
    deferred: bool,
    // The group references the buffer; drop it first.
    group: wgpu::BindGroup,
    _parameters: wgpu::Buffer,
    material: Weak<dyn std::any::Any + Send + Sync>,
}

/// Uploaded materials and the images they sample.
pub(crate) struct MaterialCache {
    sampler: wgpu::Sampler,
    white: (wgpu::Texture, wgpu::TextureView),
    black: (wgpu::Texture, wgpu::TextureView),
    flat_normal: (wgpu::Texture, wgpu::TextureView),
    // Uploaded images keyed by asset identity and color space (true for sRGB color maps).
    textures: HashMap<(u64, bool), UploadedTexture>,
    entries: HashMap<u64, MaterialEntry>,
    // Custom models materials referenced before anything registered them, reported once each.
    missing_models: Vec<String>,
}

impl MaterialCache {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        Self {
            sampler: mipmapped_sampler(device, "material maps", wgpu::AddressMode::Repeat),
            white: solid(device, queue, "material white", [255, 255, 255, 255]),
            black: solid(device, queue, "material black", [0, 0, 0, 255]),
            flat_normal: solid(device, queue, "material flat normal", [128, 128, 255, 255]),
            textures: HashMap::new(),
            entries: HashMap::new(),
            missing_models: Vec::new(),
        }
    }

    /// Upload a material's parameters and maps and build its bind group the first time it is drawn. Returns the model
    /// that draws it, or `None` when the material names a custom model nothing registered (it is then skipped, with
    /// one warning per model name).
    pub(crate) fn prepare(
        &mut self,
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        models: &ShadingModels,
        material: &MaterialAsset,
    ) -> Result<Option<ModelId>, RenderError> {
        if let Some(entry) = self.entries.get(&material.id()) {
            return Ok(Some(entry.model));
        }
        let Some(model) = models.find(material.model().name()) else {
            let name = material.model().name().to_owned();
            if !self.missing_models.contains(&name) {
                log::warn!("Materials using the unregistered shading model \"{name}\" are not drawn.");
                self.missing_models.push(name);
            }
            return Ok(None);
        };

        // Images first, in the color space each binding expects.
        let definition = &models.get(model).definition;
        for texture in &definition.textures {
            if let Some(asset) = material.texture(&texture.name) {
                self.upload(gpu, mipmaps, asset, texture.srgb)?;
            }
        }
        for (binding, _) in material.textures() {
            if !definition.textures.iter().any(|texture| texture.name == binding) {
                log::warn!(
                    "Shading model \"{}\" has no \"{binding}\" texture; the material's map is ignored.",
                    definition.name
                );
            }
        }

        // Parameters, then the group: uniform, textures in definition order, sampler.
        let parameters = models.pack(model, material);
        let device = gpu.device();
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("material parameters"),
            contents: bytemuck::cast_slice(&parameters),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let views: Vec<wgpu::TextureView> = definition
            .textures
            .iter()
            .map(|texture| self.view(material.texture(&texture.name), texture.srgb, texture.fallback))
            .collect();
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }];
        for (index, view) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry {
                binding: index as u32 + 1,
                resource: wgpu::BindingResource::TextureView(view),
            });
        }
        entries.push(wgpu::BindGroupEntry {
            binding: views.len() as u32 + 1,
            resource: wgpu::BindingResource::Sampler(&self.sampler),
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("material"),
            layout: &models.get(model).layout,
            entries: &entries,
        });
        self.entries.insert(
            material.id(),
            MaterialEntry {
                model,
                deferred: models.deferred_material(model, material),
                group,
                _parameters: buffer,
                material: material.downgrade(),
            },
        );
        Ok(Some(model))
    }

    /// The model and bind group of a prepared material.
    pub(crate) fn get(&self, material: u64) -> Option<(ModelId, &wgpu::BindGroup)> {
        self.entries.get(&material).map(|entry| (entry.model, &entry.group))
    }

    /// Whether a prepared material shades deferred (see `ShadingModels::deferred_material`).
    pub(crate) fn deferred(&self, material: u64) -> bool {
        self.entries.get(&material).is_some_and(|entry| entry.deferred)
    }

    /// Release entries and images whose assets were dropped.
    pub(crate) fn release_dropped(&mut self) {
        self.entries.retain(|_, entry| entry.material.strong_count() > 0);
        self.textures.retain(|_, entry| entry.asset.strong_count() > 0);
    }

    /// The view for one binding: the uploaded image, or the binding's fallback.
    fn view(&self, asset: Option<&TextureAsset>, srgb: bool, fallback: TextureFallback) -> wgpu::TextureView {
        let uploaded = asset.and_then(|asset| self.textures.get(&(asset.id(), srgb)));
        match (uploaded, fallback) {
            (Some(uploaded), _) => uploaded.view.clone(),
            (None, TextureFallback::White) => self.white.1.clone(),
            (None, TextureFallback::Black) => self.black.1.clone(),
            (None, TextureFallback::FlatNormal) => self.flat_normal.1.clone(),
        }
    }

    /// Upload one image in its color space with a mip chain, once.
    fn upload(
        &mut self,
        gpu: &GpuContext,
        mipmaps: &mut MipmapGenerator,
        asset: &TextureAsset,
        srgb: bool,
    ) -> Result<(), RenderError> {
        let key = (asset.id(), srgb);
        if self.textures.contains_key(&key) {
            return Ok(());
        }
        let max_texture_dimension = gpu.capabilities().max_texture_dimension;
        if asset.width() > max_texture_dimension || asset.height() > max_texture_dimension {
            return Err(RenderError::InvalidTexture(format!(
                "Material texture dimensions must be 1–{max_texture_dimension} pixels per axis."
            )));
        }
        let format = if srgb { COLOR_MAP_FORMAT } else { DATA_MAP_FORMAT };
        let texture = mipmaps.upload(gpu, "material map", asset, format);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.textures.insert(
            key,
            UploadedTexture {
                view,
                _texture: texture,
                asset: asset.downgrade(),
            },
        );
        Ok(())
    }
}

/// A 1x1 texture of one color.
fn solid(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    color: [u8; 4],
) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &color,
    );
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}
