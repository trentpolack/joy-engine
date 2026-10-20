// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shading models: the registry of WGSL programs materials draw with.
//!
//! A shading model is one WGSL file implementing the contract in `shaders/surface-contract.wgsl`, plus a description
//! of its material group: the `vec4f` parameters that follow the coverage header in `MaterialParameters`, and the
//! textures bound after them. The renderer composes every model into the same scene, depth pre-pass, and shadow caster
//! entry points, so a new model needs no other renderer change. A model that also implements `materialEncode` and
//! `materialDecode` supports deferred lighting, and its opaque surfaces shade in the G-buffer and lighting passes where
//! the device supports them. The engine registers its built-in models (lit, layered, and unlit) on creation; games
//! register their own with [`Renderer::register_shading_model`](crate::Renderer::register_shading_model).

use std::borrow::Cow;
use std::collections::HashMap;

use joy_render_types::material::{
    BASE_COLOR_MAP,
    EMISSIVE_MAP,
    LAYERED_MODEL,
    LIT_MODEL,
    METALLIC_MAP,
    NORMAL_MAP,
    OCCLUSION_MAP,
    ROUGHNESS_MAP,
    UNLIT_MODEL,
};
use joy_render_types::{
    AlphaMode,
    Shading,
};

use crate::RenderError;
use crate::material::MaterialAsset;
use crate::pipelines::{
    lighting_module_source,
    scene_module_source,
};

/// The built-in models' WGSL.
const LIT_SOURCE: &str = include_str!("../shaders/models/lit.wgsl");
const LAYERED_SOURCE: &str = include_str!("../shaders/models/layered.wgsl");
const UNLIT_SOURCE: &str = include_str!("../shaders/models/unlit.wgsl");

/// Index of a registered model in the renderer's registry.
pub(crate) type ModelId = usize;

/// Registry index of the built-in lit model, which draws transient opaque triangles.
pub(crate) const LIT_MODEL_ID: ModelId = 0;

/// Registry index of the built-in unlit model, which draws transient transparent triangles.
pub(crate) const UNLIT_MODEL_ID: ModelId = 2;

/// The functions a model implements to support deferred lighting (surface-contract.wgsl).
const DEFERRED_FUNCTIONS: [&str; 2] = ["materialEncode", "materialDecode"];

/// The deferred lighting pass's fragment entry point (deferred-lighting.wgsl).
const LIGHTING_ENTRY: &str = "lightingFragmentMain";

/// Value of a texture binding a material leaves unset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextureFallback {
    /// Opaque white: multiplies nothing away.
    White,
    /// Opaque black: adds nothing.
    Black,
    /// A tangent-space normal pointing straight out of the surface.
    FlatNormal,
}

/// One texture a shading model samples.
#[derive(Clone, Debug, PartialEq)]
pub struct TextureDefinition {
    /// Binding name materials set it through.
    pub name: String,
    /// Whether the texture holds sRGB color, which sampling decodes to linear light; data maps (normals, masks) do not.
    pub srgb: bool,
    /// What a material without this texture binds.
    pub fallback: TextureFallback,
}

/// One `vec4f` parameter of a shading model's `MaterialParameters`, after the coverage header.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterDefinition {
    /// Name materials set it through ([`CustomShading::parameters`](joy_render_types::CustomShading)).
    pub name: String,
    /// The value a material that omits the parameter uses.
    pub default: [f32; 4],
}

/// A shading model: its WGSL and the layout of its material group.
///
/// The WGSL declares `@group(3) @binding(0) var<uniform> material: MaterialParameters` whose struct begins with
/// `coverage: vec4f` followed by one `vec4f` per entry of `parameters`, in order; each texture of `textures` at the
/// following bindings, in order; and one filtering sampler after the last texture. It implements the functions listed in
/// `surface-contract.wgsl` and reads only its inputs, its material group, and the environment helpers.
#[derive(Clone, Debug, PartialEq)]
pub struct ShadingModelDefinition {
    /// Registry name materials reference ([`CustomShading::model`](joy_render_types::CustomShading)).
    pub name: String,
    /// The model's WGSL (load it with `include_str!`).
    pub source: Cow<'static, str>,
    /// The `vec4f` parameters after the coverage header, in declaration order.
    pub parameters: Vec<ParameterDefinition>,
    /// The textures, in binding order starting at binding 1.
    pub textures: Vec<TextureDefinition>,
}

/// How a registered model's parameters are filled from a material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Packing {
    Lit,
    Layered,
    Unlit,
    Custom,
}

/// A model in the registry.
pub(crate) struct RegisteredModel {
    pub(crate) definition: ShadingModelDefinition,
    packing: Packing,
    /// Whether the model implements the deferred functions; its opaque materials may then shade deferred.
    pub(crate) deferred: bool,
    /// The material group's layout: a uniform, the model's textures, and a sampler.
    pub(crate) layout: wgpu::BindGroupLayout,
}

/// Every shading model one renderer can draw, by registry index. Built-in models come first.
pub(crate) struct ShadingModels {
    models: Vec<RegisteredModel>,
    by_name: HashMap<String, ModelId>,
}

impl ShadingModels {
    /// The registry with the built-in models.
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let mut models = Self {
            models: Vec::new(),
            by_name: HashMap::new(),
        };
        for (definition, packing) in builtin_definitions() {
            let deferred = packing != Packing::Unlit;
            models.insert(device, definition, packing, deferred);
        }
        models
    }

    /// Add a game's model after checking that its WGSL compiles with the contract (composed after `common`, the shared
    /// scene sources) and declares the material group its definition describes. A model implementing the deferred
    /// functions must also compile into the lighting pass without reading its material group or image lighting. Fails
    /// for a name already registered, a malformed definition, or WGSL that does not validate.
    pub(crate) fn register(
        &mut self,
        device: &wgpu::Device,
        definition: ShadingModelDefinition,
        common: &str,
    ) -> Result<ModelId, RenderError> {
        if definition.name.trim().is_empty() || self.by_name.contains_key(&definition.name) {
            return Err(RenderError::InvalidMaterial(format!(
                "shading model \"{}\" needs a unique, nonempty name",
                definition.name
            )));
        }
        let deferred = validate_model_source(&definition, &scene_module_source(common, &definition.source))?;
        if deferred {
            validate_lighting_source(&definition, &lighting_module_source(common, &definition.source))?;
        }
        Ok(self.insert(device, definition, Packing::Custom, deferred))
    }

    /// The model registered as `name`.
    pub(crate) fn find(&self, name: &str) -> Option<ModelId> {
        self.by_name.get(name).copied()
    }

    /// A registered model.
    pub(crate) fn get(&self, id: ModelId) -> &RegisteredModel {
        &self.models[id]
    }

    /// Number of registered models.
    pub(crate) fn len(&self) -> usize {
        self.models.len()
    }

    /// Whether `material`, drawn with model `id`, can shade deferred: its model implements the deferred functions, and
    /// for layered materials, the G-buffer holds every feature the layer uses ([`layer_fits_gbuffer`]).
    pub(crate) fn deferred_material(&self, id: ModelId, material: &MaterialAsset) -> bool {
        let model = &self.models[id];
        if !model.deferred || material.material().alpha_mode == AlphaMode::Blend {
            return false;
        }
        match model.packing {
            Packing::Layered => material.surface_layer().is_none_or(layer_fits_gbuffer),
            Packing::Lit | Packing::Custom => true,
            Packing::Unlit => false,
        }
    }

    /// The material's `MaterialParameters` for model `id`: the coverage header, then the model's parameters.
    pub(crate) fn pack(&self, id: ModelId, material: &MaterialAsset) -> Vec<[f32; 4]> {
        let model = &self.models[id];
        let record = material.material();
        let alpha_mode = match record.alpha_mode {
            AlphaMode::Opaque => 0.0,
            AlphaMode::Mask => 1.0,
            AlphaMode::Blend => 2.0,
        };
        let mut values = vec![[record.alpha_cutoff, alpha_mode, id as f32, 0.0]];
        match (model.packing, &record.shading) {
            (Packing::Lit, Shading::Lit(lit)) => {
                values.push(lit.base_color.to_array());
                values.push(lit.emissive.with_alpha(0.0).to_array());
            }
            (Packing::Unlit, Shading::Unlit(unlit)) => values.push(unlit.base_color.to_array()),
            (Packing::Layered, Shading::Layered(_)) => {
                let layer = material.surface_layer().cloned().unwrap_or_default();
                values.extend(pack_layer(&layer, material));
            }
            (Packing::Custom, Shading::Custom(custom)) => {
                for parameter in &model.definition.parameters {
                    let value = custom.parameters.get(&parameter.name);
                    values.push(value.map_or(parameter.default, |value| value.to_vec4()));
                }
            }
            // A material always resolves to its own model's packing; defaults keep a mismatch drawable.
            _ => values.extend(model.definition.parameters.iter().map(|parameter| parameter.default)),
        }
        values
    }

    fn insert(
        &mut self,
        device: &wgpu::Device,
        definition: ShadingModelDefinition,
        packing: Packing,
        deferred: bool,
    ) -> ModelId {
        let layout = material_layout(device, &definition);
        let id = self.models.len();
        self.by_name.insert(definition.name.clone(), id);
        self.models.push(RegisteredModel {
            definition,
            packing,
            deferred,
            layout,
        });
        id
    }
}

/// Whether the layered model's G-buffer encoding holds everything `layer` uses: a gray F90, no thin film, a coat (if
/// any) that is untinted with the default F0, and at most one of haze, fuzz, and coat (see `materialEncode` in
/// `layered.wgsl`). Other layers shade forward.
pub(crate) fn layer_fits_gbuffer(layer: &joy_render_types::SurfaceLayer) -> bool {
    const TOLERANCE: f32 = 0.001;
    let [red, green, blue] = layer.f90.rgb_array();
    let gray_f90 = (red - green).abs() < TOLERANCE && (green - blue).abs() < TOLERANCE;
    let film = layer.thin_film.is_some_and(|film| film.weight > 0.0);
    let haze = layer.haze.is_some_and(|haze| haze.weight > 0.0);
    let fuzz =
        layer.fuzz.is_some_and(|fuzz| fuzz.amount > 0.0 && fuzz.color.rgb_array().iter().any(|channel| *channel > 0.0));
    let coat = layer.coat.filter(|coat| coat.weight > 0.0);
    let plain_coat = coat.is_none_or(|coat| {
        let [tint_red, tint_green, tint_blue] = coat.tint.rgb_array();
        let untinted = [tint_red, tint_green, tint_blue].iter().all(|channel| (channel - 1.0).abs() < TOLERANCE);
        untinted && (coat.f0 - joy_render_types::material::DIELECTRIC_F0).abs() < TOLERANCE
    });
    let lobes = usize::from(haze) + usize::from(fuzz) + usize::from(coat.is_some());
    gray_f90 && !film && plain_coat && lobes <= 1
}

/// The `vec4f` parameters of a layered material, after the coverage header (`MaterialParameters` in `layered.wgsl`).
fn pack_layer(layer: &joy_render_types::SurfaceLayer, material: &MaterialAsset) -> [[f32; 4]; 10] {
    let haze = layer.haze.unwrap_or(joy_render_types::Haze {
        roughness: layer.roughness,
        weight: 0.0,
    });
    let fuzz = layer.fuzz.unwrap_or(joy_render_types::Fuzz {
        color: joy_core::LinearColor::BLACK,
        roughness: 0.5,
        amount: 0.0,
    });
    let coat = layer.coat.map_or(
        joy_render_types::Coat {
            weight: 0.0,
            ..joy_render_types::Coat::default()
        },
        |coat| coat,
    );
    let film = layer.thin_film.unwrap_or(joy_render_types::ThinFilm {
        thickness: 0.0,
        ior: 1.3,
        weight: 0.0,
    });
    let flag = |binding: &str| if material.texture(binding).is_some() { 1.0 } else { 0.0 };
    [
        layer.base_color.to_array(),
        layer.f0.with_alpha(layer.metallic).to_array(),
        layer.f90.with_alpha(layer.roughness).to_array(),
        layer.emissive.with_alpha(0.0).to_array(),
        [haze.roughness, haze.weight, fuzz.roughness, fuzz.amount],
        fuzz.color.with_alpha(0.0).to_array(),
        [coat.weight, coat.roughness, coat.f0, 0.0],
        coat.tint.with_alpha(0.0).to_array(),
        [film.thickness, film.ior, film.weight, 0.0],
        [flag(NORMAL_MAP), flag(OCCLUSION_MAP), 0.0, 0.0],
    ]
}

/// The engine's models in registry order: lit, layered, unlit (matching [`LIT_MODEL_ID`] and [`UNLIT_MODEL_ID`]).
fn builtin_definitions() -> [(ShadingModelDefinition, Packing); 3] {
    let parameter = |name: &str, default: [f32; 4]| ParameterDefinition {
        name: name.to_owned(),
        default,
    };
    let texture = |name: &str, srgb: bool, fallback: TextureFallback| TextureDefinition {
        name: name.to_owned(),
        srgb,
        fallback,
    };
    let lit = ShadingModelDefinition {
        name: LIT_MODEL.to_owned(),
        source: Cow::Borrowed(LIT_SOURCE),
        parameters: vec![parameter("baseColor", [1.0; 4]), parameter("emissive", [0.0; 4])],
        textures: vec![texture(BASE_COLOR_MAP, true, TextureFallback::White)],
    };
    let layered = ShadingModelDefinition {
        name: LAYERED_MODEL.to_owned(),
        source: Cow::Borrowed(LAYERED_SOURCE),
        parameters: [
            "baseColor",
            "f0Metallic",
            "f90Roughness",
            "emissive",
            "hazeFuzz",
            "fuzzColor",
            "coat",
            "coatTint",
            "thinFilm",
            "maps",
        ]
        .into_iter()
        .map(|name| parameter(name, [0.0; 4]))
        .collect(),
        textures: vec![
            texture(BASE_COLOR_MAP, true, TextureFallback::White),
            texture(NORMAL_MAP, false, TextureFallback::FlatNormal),
            texture(EMISSIVE_MAP, true, TextureFallback::White),
            texture(METALLIC_MAP, false, TextureFallback::White),
            texture(ROUGHNESS_MAP, false, TextureFallback::White),
            texture(OCCLUSION_MAP, false, TextureFallback::White),
        ],
    };
    let unlit = ShadingModelDefinition {
        name: UNLIT_MODEL.to_owned(),
        source: Cow::Borrowed(UNLIT_SOURCE),
        parameters: vec![parameter("baseColor", [1.0; 4])],
        textures: vec![texture(BASE_COLOR_MAP, true, TextureFallback::White)],
    };
    [(lit, Packing::Lit), (layered, Packing::Layered), (unlit, Packing::Unlit)]
}

/// The material group for `definition`: binding 0 the parameters, then each texture, then the sampler.
fn material_layout(device: &wgpu::Device, definition: &ShadingModelDefinition) -> wgpu::BindGroupLayout {
    let texture_count = definition.textures.len() as u32;
    let mut entries = vec![crate::pipelines::uniform_entry(0)];
    entries.extend((1..=texture_count).map(crate::pipelines::texture_entry));
    entries.push(crate::pipelines::sampler_entry(texture_count + 1));
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{} material", definition.name)),
        entries: &entries,
    })
}

/// Check a game's model before any pipeline compiles it: the composed scene module must parse and validate, and the
/// model must declare its material group as the definition describes, so a mistake reports an error instead of
/// failing inside the GPU device. Returns whether the model implements the deferred functions.
fn validate_model_source(definition: &ShadingModelDefinition, composed: &str) -> Result<bool, RenderError> {
    use wgpu::naga;

    let invalid =
        |message: String| RenderError::InvalidMaterial(format!("shading model \"{}\": {message}", definition.name));
    let module = naga::front::wgsl::parse_str(composed).map_err(|error| invalid(error.emit_to_string(composed)))?;
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|error| invalid(format!("{error:?}")))?;

    // The material group: binding 0 a uniform, then one texture per definition entry, then a sampler.
    let texture_count = definition.textures.len() as u32;
    for (_, variable) in module.global_variables.iter() {
        let Some(binding) = &variable.binding else {
            continue;
        };
        if binding.group != 3 {
            continue;
        }
        let expected = match binding.binding {
            0 => matches!(variable.space, naga::AddressSpace::Uniform),
            index if index <= texture_count => matches!(module.types[variable.ty].inner, naga::TypeInner::Image { .. }),
            index if index == texture_count + 1 => {
                matches!(module.types[variable.ty].inner, naga::TypeInner::Sampler { .. })
            }
            _ => false,
        };
        if !expected {
            return Err(invalid(format!("group 3 binding {} does not match the definition", binding.binding)));
        }
    }

    // Deferred support needs both functions; one alone is a mistake worth reporting.
    let defined = |name: &str| module.functions.iter().any(|(_, function)| function.name.as_deref() == Some(name));
    match DEFERRED_FUNCTIONS.map(defined) {
        [true, true] => Ok(true),
        [false, false] => Ok(false),
        _ => Err(invalid(format!(
            "deferred lighting needs both {} and {}",
            DEFERRED_FUNCTIONS[0], DEFERRED_FUNCTIONS[1]
        ))),
    }
}

/// Check a deferred model's lighting module: it must validate, and its lighting entry point must not reach the material
/// group (3) or image lighting (2), which the lighting pass does not bind.
fn validate_lighting_source(definition: &ShadingModelDefinition, composed: &str) -> Result<(), RenderError> {
    use wgpu::naga;

    let invalid =
        |message: String| RenderError::InvalidMaterial(format!("shading model \"{}\": {message}", definition.name));
    let module = naga::front::wgsl::parse_str(composed).map_err(|error| invalid(error.emit_to_string(composed)))?;
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty())
        .validate(&module)
        .map_err(|error| invalid(format!("{error:?}")))?;
    let Some(entry) = module.entry_points.iter().position(|entry| entry.name == LIGHTING_ENTRY) else {
        return Err(invalid(format!("the lighting module has no {LIGHTING_ENTRY}")));
    };
    let usage = info.get_entry_point(entry);
    for (handle, variable) in module.global_variables.iter() {
        let unbound = variable.binding.as_ref().is_some_and(|binding| binding.group >= 2);
        if unbound && !usage[handle].is_empty() {
            return Err(invalid(format!(
                "{} and {} must not read the material group or image lighting (\"{}\")",
                DEFERRED_FUNCTIONS[1],
                "materialDirect",
                variable.name.as_deref().unwrap_or("unnamed")
            )));
        }
    }
    Ok(())
}
