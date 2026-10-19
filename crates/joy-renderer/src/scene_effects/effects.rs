// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The effect graph for one renderer: three chains (lighting, temporal
//! antialiasing, finishing), each fed the previous chain's result, with per-frame uniforms derived from the camera,
//! lights, and settings.

use std::collections::HashMap;

use joy_core::math::Mat4;

use crate::scene_effects::chain::{
    ChainResources,
    ExternalBindings,
    ShaderPassChain,
};
use crate::scene_effects::config::{
    Reprojection,
    SCENE_EFFECT_NAMES,
    SceneEffectsError,
    SceneEffectsOptions,
};
use crate::scene_effects::pass::UniformValues;
use crate::scene_effects::pipelines::scene_effect_pipelines;
use crate::scene_effects::volume_lights::{
    ClusterBindings,
    MAX_LIGHTS_PER_CLUSTER,
};

/// Per-frame inputs to the effect graph.
pub(crate) struct EffectsFrame<'a> {
    pub(crate) depth: &'a wgpu::TextureView,
    pub(crate) normal: Option<&'a wgpu::TextureView>,
    /// Jittered motion (lighting effects).
    pub(crate) velocity: Option<&'a wgpu::TextureView>,
    /// Unjittered motion (antialiasing and motion blur).
    pub(crate) taa_velocity: Option<&'a wgpu::TextureView>,
    /// Raster (jittered) projection.
    pub(crate) projection: Mat4,
    pub(crate) inverse_projection: Mat4,
    pub(crate) inverse_view: Mat4,
    pub(crate) inverse_view_projection: Mat4,
    pub(crate) previous_view_projection: Mat4,
    pub(crate) inverse_raster_view_projection: Mat4,
    pub(crate) previous_raster_view_projection: Mat4,
    pub(crate) near: f32,
    pub(crate) far: f32,
    pub(crate) orthographic: bool,
    pub(crate) current_jitter: [f32; 2],
    pub(crate) previous_jitter: [f32; 2],
    pub(crate) directional_direction_view: [f32; 3],
    pub(crate) directional_color: [f32; 3],
    pub(crate) clusters: Option<ClusterBindings<'a>>,
    pub(crate) time_seconds: f32,
    pub(crate) delta_time_seconds: f32,
    pub(crate) reset_history: bool,
}

/// Whether each requested effect can run, and why not.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneEffectCapability {
    /// Effect name in TypeScript spelling.
    pub name: &'static str,
    /// The options enable it.
    pub requested: bool,
    /// It runs on this device.
    pub available: bool,
    /// Why a requested effect does not run.
    pub reason: Option<&'static str>,
}

/// What the current device and options support.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneEffectsCapabilities {
    /// Whether the backend supports scene effects at all.
    pub available: bool,
    /// The chain's color format, when effects run.
    pub color_format: Option<wgpu::TextureFormat>,
    /// One entry per effect.
    pub effects: Vec<SceneEffectCapability>,
}

/// The reason reported for every effect on backends without scene effects, matching TypeScript.
const BACKEND_REQUIRED: &str = "Requires the WebGPU backend.";

/// Which effects one [`SceneEffectsRuntime::encode`] call runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EffectStage {
    /// Effects that read opaque surfaces and change their lighting (ambient occlusion, global illumination,
    /// reflections, volumetric lighting and fog, depth-aware blur, and outlines). They run before transparency, whose
    /// surfaces have no depth or normals of their own.
    Lighting,
    /// Temporal antialiasing, then motion blur and adaptive exposure, over the finished scene including transparency.
    Composite,
}

/// Effect chains and their frame counter.
pub(crate) struct SceneEffectsRuntime {
    options: SceneEffectsOptions,
    lighting: Option<ShaderPassChain>,
    temporal: Option<ShaderPassChain>,
    finishing: Option<ShaderPassChain>,
    resources: ChainResources,
    frame_index: u32,
    depth_convention: Option<(bool, u32, u32)>,
}

impl SceneEffectsRuntime {
    /// Compile the enabled effects for `color_format`.
    pub(crate) fn new(device: &wgpu::Device, options: SceneEffectsOptions, color_format: wgpu::TextureFormat) -> Self {
        let groups = scene_effect_pipelines(&options, color_format);
        let chain = |specs: &[_]| (!specs.is_empty()).then(|| ShaderPassChain::new(device, specs, color_format));
        Self {
            lighting: chain(&groups.lighting),
            temporal: chain(&groups.temporal),
            finishing: chain(&groups.finishing),
            resources: ChainResources::new(device),
            options,
            frame_index: 0,
            depth_convention: None,
        }
    }

    pub(crate) fn options(&self) -> &SceneEffectsOptions {
        &self.options
    }

    /// Whether any effect runs in the [`EffectStage::Lighting`] stage, between opaque geometry and transparency.
    pub(crate) fn has_lighting_effects(&self) -> bool {
        self.lighting.is_some()
    }

    /// Frames rendered since creation.
    pub(crate) fn frame_index(&self) -> u32 {
        self.frame_index
    }

    /// Restart every history from its initial value.
    pub(crate) fn reset_history(&mut self) {
        for chain in [&mut self.lighting, &mut self.temporal, &mut self.finishing].into_iter().flatten() {
            chain.reset_history();
        }
        self.depth_convention = None;
    }

    /// Run one stage's chains over `source` and copy the result back into it. Fails without recording for invalid
    /// frames.
    ///
    /// A frame runs [`EffectStage::Lighting`] over the opaque scene, then the renderer draws transparency, then
    /// [`EffectStage::Composite`] runs over everything. The lighting stage restarts history when the frame asks or the
    /// depth convention changed; the composite stage finishes the frame.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::TextureView,
        source_format: wgpu::TextureFormat,
        size: (u32, u32),
        frame: &EffectsFrame<'_>,
        stage: EffectStage,
    ) -> Result<(), SceneEffectsError> {
        let uniforms = self.frame_uniforms(frame)?;
        let convention = (frame.orthographic, frame.near.to_bits(), frame.far.to_bits());
        if stage == EffectStage::Lighting
            && (frame.reset_history || self.depth_convention.is_some_and(|previous| previous != convention))
        {
            self.reset_history();
        }

        let base = ExternalBindings {
            depth: Some(frame.depth),
            normal: frame.normal,
            velocity: frame.velocity,
            point_lights: frame.clusters.as_ref().map(|clusters| clusters.point_lights),
            cluster_light_counts: frame.clusters.as_ref().map(|clusters| clusters.cluster_light_counts),
            cluster_light_indices: frame.clusters.as_ref().map(|clusters| clusters.cluster_light_indices),
        };
        // Lighting effects use jittered motion; antialiasing and finishing use unjittered motion.
        let later = ExternalBindings { velocity: frame.taa_velocity.or(frame.velocity), ..base };

        // Each chain reads the scene texture and its result is copied back, so the next chain (and transparency) sees
        // it as the scene: the TypeScript chains pass each result on as the next one's source the same way.
        let Self { lighting, temporal, finishing, resources, .. } = self;
        let chains = match stage {
            EffectStage::Lighting => [(lighting.as_mut(), base), (None, base)],
            EffectStage::Composite => [(temporal.as_mut(), later), (finishing.as_mut(), later)],
        };
        for (chain, externals) in chains {
            let Some(chain) = chain else {
                continue;
            };
            let output = chain.encode(device, queue, encoder, resources, source, size, &externals, &uniforms);
            resources.blit(device, encoder, output, source, source_format);
        }
        if stage == EffectStage::Composite {
            self.depth_convention = Some(convention);
            self.frame_index = self.frame_index.wrapping_add(1);
        }
        Ok(())
    }

    /// Uniforms for every pass this frame, keyed by pass name; ported from `createRenderOptions`.
    fn frame_uniforms(
        &self,
        frame: &EffectsFrame<'_>,
    ) -> Result<HashMap<&'static str, UniformValues>, SceneEffectsError> {
        let options = &self.options;
        let mut uniforms: HashMap<&'static str, UniformValues> = HashMap::new();
        let matrix = |value: Mat4| value.to_cols_array().to_vec();
        let scalar = |value: f32| vec![value];
        let frame_index = self.frame_index as f32;

        if (options.ssao.is_some() || options.clustered_lighting.is_some())
            && !(frame.near > 0.0 && frame.far.is_finite() && frame.far > frame.near)
        {
            return Err(SceneEffectsError::InvalidFrame("frame.farPlane must be greater than frame.nearPlane.".into()));
        }
        if let Some(ssao) = &options.ssao {
            if frame.orthographic {
                return Err(SceneEffectsError::InvalidFrame(
                    "SSAO requires perspective projection depth reconstruction.".into(),
                ));
            }
            uniforms.insert(
                "ssaoEvaluate",
                vec![
                    ("radius", scalar(ssao.radius)),
                    ("bias", scalar(ssao.bias)),
                    ("intensity", scalar(ssao.intensity)),
                    ("nearPlane", scalar(frame.near)),
                    ("farPlane", scalar(frame.far)),
                ],
            );
        }
        let matrices = || -> UniformValues {
            vec![
                ("projectionMatrix", matrix(frame.projection)),
                ("inverseProjectionMatrix", matrix(frame.inverse_projection)),
            ]
        };
        let temporal = |history_weight: f32, depth_threshold: f32| -> UniformValues {
            vec![
                ("historyWeight", scalar(history_weight)),
                ("depthThreshold", scalar(depth_threshold)),
                ("inverseProjectionMatrix", matrix(frame.inverse_projection)),
            ]
        };
        if let Some(gtao) = &options.gtao {
            let mut evaluate = vec![
                ("radius", scalar(gtao.radius)),
                ("bias", scalar(gtao.bias)),
                ("intensity", scalar(gtao.intensity)),
            ];
            evaluate.extend(matrices());
            evaluate.push(("frameIndex", scalar(frame_index)));
            uniforms.insert("gtaoEvaluate", evaluate);
            uniforms.insert("gtaoTemporal", temporal(gtao.history_weight, gtao.depth_threshold));
            uniforms.insert("gtaoComposite", vec![("strength", scalar(gtao.strength))]);
        }
        if let Some(ssgi) = &options.ssgi {
            let mut trace = vec![
                ("radius", scalar(ssgi.radius)),
                ("thickness", scalar(ssgi.thickness)),
                ("intensity", scalar(ssgi.intensity)),
                ("rayCount", scalar(ssgi.ray_count)),
                ("stepCount", scalar(ssgi.step_count)),
            ];
            trace.extend(matrices());
            trace.push(("frameIndex", scalar(frame_index)));
            uniforms.insert("ssgiTrace", trace);
            uniforms.insert("ssgiTemporal", temporal(ssgi.history_weight, ssgi.depth_threshold));
            uniforms.insert("ssgiComposite", vec![("strength", scalar(ssgi.strength))]);
        }
        if let Some(ssr) = &options.ssr {
            let mut trace = vec![
                ("intensity", scalar(ssr.intensity)),
                ("maxDistance", scalar(ssr.max_distance)),
                ("thickness", scalar(ssr.thickness)),
                ("sampleCount", scalar(ssr.sample_count)),
                ("maxRoughness", scalar(ssr.max_roughness)),
            ];
            trace.extend(matrices());
            trace.push(("frameIndex", scalar(frame_index)));
            uniforms.insert("ssrTrace", trace);
            uniforms.insert("ssrTemporal", temporal(ssr.history_weight, ssr.depth_threshold));
            uniforms.insert("ssrSpatial", vec![("inverseProjectionMatrix", matrix(frame.inverse_projection))]);
            uniforms.insert(
                "ssrComposite",
                vec![("strength", scalar(ssr.strength)), ("inverseProjectionMatrix", matrix(frame.inverse_projection))],
            );
        }
        if let Some(outlines) = &options.outlines {
            uniforms.insert(
                "screenSpaceOutline",
                vec![
                    ("color", outlines.color.to_array().to_vec()),
                    ("thickness", scalar(outlines.thickness)),
                    ("depthThreshold", scalar(outlines.depth_threshold)),
                    ("normalThreshold", scalar(outlines.normal_threshold)),
                ],
            );
        }
        if let Some(fog) = &options.height_fog {
            uniforms.insert(
                "volumetricFog",
                vec![
                    ("fogColor", fog.fog_color.to_array().to_vec()),
                    ("density", scalar(fog.density)),
                    ("heightFalloff", scalar(fog.height_falloff)),
                    ("scattering", scalar(fog.scattering)),
                    ("historyWeight", scalar(0.0)),
                    ("time", scalar(frame.time_seconds)),
                ],
            );
        }
        if let Some(clustered) = &options.clustered_lighting {
            let Some(clusters) = &frame.clusters else {
                return Err(SceneEffectsError::InvalidFrame(
                    "Clustered lighting requires point-light clusters.".into(),
                ));
            };
            let mut trace = vec![
                ("fogColor", clustered.fog_color.rgb_array().to_vec()),
                ("density", scalar(clustered.density)),
                ("heightFalloff", scalar(clustered.height_falloff)),
                ("fogHeight", scalar(clustered.fog_height)),
                ("anisotropy", scalar(clustered.anisotropy)),
                ("directionalIntensity", scalar(clustered.directional_intensity)),
                ("pointLightIntensity", scalar(clustered.point_light_intensity)),
                ("maxDistance", scalar(clustered.max_distance)),
                ("sampleCount", scalar(clustered.sample_count)),
                ("shadowStrength", scalar(clustered.shadow_strength)),
            ];
            trace.extend(matrices());
            trace.extend([
                ("inverseViewMatrix", matrix(frame.inverse_view)),
                ("clusterCountX", scalar(clusters.cluster_count[0] as f32)),
                ("clusterCountY", scalar(clusters.cluster_count[1] as f32)),
                ("clusterCountZ", scalar(clusters.cluster_count[2] as f32)),
                ("maxLightsPerCluster", scalar(MAX_LIGHTS_PER_CLUSTER as f32)),
                ("pointLightCount", scalar(clusters.point_light_count as f32)),
                ("clusterNearPlane", scalar(frame.near)),
                ("clusterFarPlane", scalar(frame.far)),
                ("directionalLightDirectionView", frame.directional_direction_view.to_vec()),
                ("directionalLightColor", frame.directional_color.to_vec()),
            ]);
            uniforms.insert("clusteredVolumetricTrace", trace);
            uniforms.insert(
                "clusteredVolumetricTemporal",
                vec![
                    ("historyWeight", scalar(clustered.history_weight)),
                    ("depthThreshold", scalar(clustered.depth_threshold)),
                    ("inverseProjectionMatrix", matrix(frame.inverse_projection)),
                    ("inverseViewProjectionMatrix", matrix(frame.inverse_raster_view_projection)),
                    ("previousViewProjectionMatrix", matrix(frame.previous_raster_view_projection)),
                ],
            );
            uniforms.insert(
                "clusteredVolumetricDepthHistoryCopy",
                vec![("inverseProjectionMatrix", matrix(frame.inverse_projection))],
            );
            uniforms.insert("clusteredVolumetricComposite", vec![("strength", scalar(clustered.strength))]);
        }
        if let Some(taa) = &options.taa {
            let mut values = vec![
                ("historyWeight", scalar(taa.history_weight)),
                ("depthThreshold", scalar(taa.depth_threshold)),
                ("currentJitter", frame.current_jitter.to_vec()),
                ("previousJitter", frame.previous_jitter.to_vec()),
            ];
            match taa.reprojection {
                Reprojection::Camera => {
                    values.push(("inverseViewProjectionMatrix", matrix(frame.inverse_view_projection)));
                    values.push(("previousViewProjectionMatrix", matrix(frame.previous_view_projection)));
                    uniforms.insert("cameraReprojectionTaaResolve", values);
                }
                Reprojection::Velocity => {
                    uniforms.insert("taaResolve", values);
                }
            }
        }
        if let Some(blur) = &options.motion_blur {
            uniforms.insert(
                "motionBlur",
                vec![("strength", scalar(blur.strength)), ("sampleCount", scalar(blur.sample_count))],
            );
        }
        if let Some(exposure) = &options.adaptive_exposure {
            let delta = if frame.delta_time_seconds > 0.0 { frame.delta_time_seconds } else { 1.0 / 60.0 };
            uniforms.insert(
                "hdrAutoExposureAdapt",
                vec![
                    ("keyValue", scalar(exposure.key_value)),
                    ("minimumExposure", scalar(exposure.minimum_exposure)),
                    ("maximumExposure", scalar(exposure.maximum_exposure)),
                    ("brightenSpeed", scalar(exposure.brighten_speed)),
                    ("darkenSpeed", scalar(exposure.darken_speed)),
                    ("deltaTime", scalar(delta.min(0.25))),
                    ("enabled", scalar(1.0)),
                ],
            );
            uniforms.insert("hdrAutoExposureApply", vec![("enabled", scalar(1.0))]);
        }
        Ok(uniforms)
    }
}

/// Capability report for `options` on a backend with (`supported`) or without scene effects.
pub(crate) fn capabilities(
    options: &SceneEffectsOptions,
    supported: bool,
    color_format: Option<wgpu::TextureFormat>,
) -> SceneEffectsCapabilities {
    let enabled: Vec<&str> = options.enabled_names().collect();
    let effects = SCENE_EFFECT_NAMES
        .iter()
        .map(|name| {
            let requested = enabled.contains(name);
            SceneEffectCapability {
                name,
                requested,
                available: requested && supported,
                reason: (requested && !supported).then_some(BACKEND_REQUIRED),
            }
        })
        .collect();
    SceneEffectsCapabilities { available: supported, color_format: color_format.filter(|_| supported), effects }
}
