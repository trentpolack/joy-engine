// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Effect pipelines: luma.gl's `create*ShaderPassPipeline` factories (MIT License, Copyright (c) vis.gl contributors)
//! with the Joy engine's overrides from `scene-effect-pipelines.ts`: unspecified target formats use the chain's color
//! format, targets sample linearly with clamped edges, and raw depth histories use 32-bit floats read with nearest
//! sampling.

use crate::scene_effects::config::{
    ClusteredLightingOptions,
    GtaoOptions,
    NormalSource,
    Reprojection,
    SceneEffectsOptions,
    SsaoOptions,
    SsgiOptions,
    SsrOptions,
};
use crate::scene_effects::pass::UniformValues;

/// How a render target is sampled by later steps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetSampling {
    /// Linear filtering, clamped edges.
    Linear,
    /// Nearest texel, clamped edges; required for 32-bit float depth history.
    Nearest,
}

/// When a target's contents are valid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TargetLifetime {
    /// Written and read within one frame.
    Transient,
    /// Keeps last frame's value; starts as the original scene or a clear color.
    History(HistoryStart),
}

/// A history target's first value after creation, resize, or reset.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum HistoryStart {
    /// A copy of the unprocessed scene.
    Original,
    /// A solid color.
    Clear([f32; 4]),
}

/// A named intermediate texture.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TargetSpec {
    pub(crate) name: &'static str,
    pub(crate) scale: f32,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) sampling: TargetSampling,
    pub(crate) lifetime: TargetLifetime,
}

/// Where a step's input texture comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InputSource {
    /// The running result of earlier steps.
    Previous,
    /// A named target (its history value when not yet written this frame).
    Target(&'static str),
}

/// One full-screen draw.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StepSpec {
    pub(crate) pass: &'static str,
    /// Texture bindings and their sources; `sourceTexture` defaults to the running result.
    pub(crate) inputs: Vec<(&'static str, InputSource)>,
    /// Output target; `None` advances the running result.
    pub(crate) output: Option<&'static str>,
    /// Step uniform overrides.
    pub(crate) uniforms: UniformValues,
}

/// A sequence of steps with the targets they share.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PipelineSpec {
    pub(crate) name: &'static str,
    pub(crate) targets: Vec<TargetSpec>,
    pub(crate) steps: Vec<StepSpec>,
}

/// Effect pipelines grouped like the TypeScript renderer: lighting, temporal antialiasing, then finishing.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct PipelineGroups {
    pub(crate) lighting: Vec<PipelineSpec>,
    pub(crate) temporal: Vec<PipelineSpec>,
    pub(crate) finishing: Vec<PipelineSpec>,
}

/// Targets whose depth history needs full precision.
const RAW_DEPTH_HISTORY: [&str; 5] =
    ["gtaoHistoryDepth", "ssgiHistoryDepth", "ssrHistoryDepth", "taaHistoryDepth", "cameraReprojectionTaaHistoryDepth"];

use wgpu::TextureFormat::{
    Rg16Float,
    Rgba8Unorm,
    Rgba16Float,
};

/// A target before the Joy overrides; `None` format means the chain's color format.
fn target(name: &'static str, scale: f32, format: Option<wgpu::TextureFormat>, lifetime: TargetLifetime) -> RawTarget {
    RawTarget { name, scale, format, lifetime }
}

struct RawTarget {
    name: &'static str,
    scale: f32,
    format: Option<wgpu::TextureFormat>,
    lifetime: TargetLifetime,
}

fn history(color: [f32; 4]) -> TargetLifetime {
    TargetLifetime::History(HistoryStart::Clear(color))
}

const TRANSIENT: TargetLifetime = TargetLifetime::Transient;
const ORIGINAL: TargetLifetime = TargetLifetime::History(HistoryStart::Original);

fn step(
    pass: &'static str,
    inputs: &[(&'static str, InputSource)],
    output: Option<&'static str>,
    uniforms: UniformValues,
) -> StepSpec {
    StepSpec { pass, inputs: inputs.to_vec(), output, uniforms }
}

use InputSource::{
    Previous,
    Target,
};

fn ssao(options: &SsaoOptions) -> (Vec<RawTarget>, Vec<StepSpec>) {
    let scale = options.resolution_scale;
    let normals = options.normal_source == NormalSource::NormalTexture;
    let mut evaluate_inputs = vec![("sourceTexture", Previous)];
    if !normals {
        evaluate_inputs.push(("normalTexture", Previous));
    }
    let targets = vec![
        target("ssaoRaw", scale, Some(Rgba8Unorm), TRANSIENT),
        target("ssaoScratch", scale, Some(Rgba8Unorm), TRANSIENT),
        target("ssaoBlurred", scale, Some(Rgba8Unorm), TRANSIENT),
    ];
    let steps = vec![
        step(
            "ssaoEvaluate",
            &evaluate_inputs,
            Some("ssaoRaw"),
            vec![("useNormalTexture", vec![f32::from(u8::from(normals))])],
        ),
        step(
            "depthAwareBlur",
            &[("sourceTexture", Target("ssaoRaw"))],
            Some("ssaoScratch"),
            vec![("direction", vec![1.0, 0.0])],
        ),
        step(
            "depthAwareBlur",
            &[("sourceTexture", Target("ssaoScratch"))],
            Some("ssaoBlurred"),
            vec![("direction", vec![0.0, 1.0])],
        ),
        step(
            "ssaoComposite",
            &[("sourceTexture", Previous), ("ambientOcclusionTexture", Target("ssaoBlurred"))],
            None,
            vec![],
        ),
    ];
    (targets, steps)
}

/// The temporal-accumulation shape GTAO, SSGI, SSR, and clustered lighting share: evaluate, accumulate against history,
/// copy depth history, blur twice, composite.
#[allow(clippy::too_many_arguments)]
fn accumulated(
    prefix: [&'static str; 5],
    names: [&'static str; 5],
    formats: [wgpu::TextureFormat; 3],
    history_clear: [f32; 4],
    depth_clear: [f32; 4],
    scale: f32,
    blur_uniforms: [UniformValues; 2],
    composite_input: &'static str,
) -> (Vec<RawTarget>, Vec<StepSpec>) {
    let [evaluate, temporal, depth_copy, blur, composite] = prefix;
    let [raw, history_name, depth_history, scratch, result] = names;
    let [color, depth_format, blur_format] = formats;
    let targets = vec![
        target(raw, scale, Some(color), TRANSIENT),
        target(history_name, scale, Some(color), history(history_clear)),
        target(depth_history, scale, Some(depth_format), history(depth_clear)),
        target(scratch, scale, Some(blur_format), TRANSIENT),
        target(result, scale, Some(blur_format), TRANSIENT),
    ];
    let [horizontal, vertical] = blur_uniforms;
    let steps = vec![
        step(evaluate, &[("sourceTexture", Previous)], Some(raw), vec![]),
        step(
            temporal,
            &[
                ("sourceTexture", Target(raw)),
                ("historyTexture", Target(history_name)),
                ("previousDepthTexture", Target(depth_history)),
            ],
            Some(history_name),
            vec![],
        ),
        step(depth_copy, &[("sourceTexture", Previous)], Some(depth_history), vec![]),
        step(blur, &[("sourceTexture", Target(history_name))], Some(scratch), horizontal),
        step(blur, &[("sourceTexture", Target(scratch))], Some(result), vertical),
        step(composite, &[("sourceTexture", Previous), (composite_input, Target(result))], None, vec![]),
    ];
    (targets, steps)
}

fn blur_uniforms(direction: [f32; 2], tuning: Option<[f32; 3]>) -> UniformValues {
    let mut values: UniformValues = vec![("direction", direction.to_vec())];
    if let Some([radius, spatial_sigma, depth_sigma]) = tuning {
        values.push(("radius", vec![radius]));
        values.push(("spatialSigma", vec![spatial_sigma]));
        values.push(("depthSigma", vec![depth_sigma]));
    }
    values
}

fn gtao(options: &GtaoOptions) -> (Vec<RawTarget>, Vec<StepSpec>) {
    let tuning = Some([3.0, 2.5, 0.008]);
    accumulated(
        ["gtaoEvaluate", "gtaoTemporal", "gtaoDepthHistoryCopy", "depthAwareBlur", "gtaoComposite"],
        ["gtaoRaw", "gtaoHistory", "gtaoHistoryDepth", "gtaoScratch", "gtaoBlurred"],
        [Rgba8Unorm, Rgba16Float, Rgba8Unorm],
        [1.0, 1.0, 1.0, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        options.resolution_scale,
        [blur_uniforms([1.0, 0.0], tuning), blur_uniforms([0.0, 1.0], tuning)],
        "ambientOcclusionTexture",
    )
}

fn ssgi(options: &SsgiOptions) -> (Vec<RawTarget>, Vec<StepSpec>) {
    accumulated(
        ["ssgiTrace", "ssgiTemporal", "ssgiDepthHistoryCopy", "ssgiSpatial", "ssgiComposite"],
        ["ssgiRaw", "ssgiHistory", "ssgiHistoryDepth", "ssgiScratch", "ssgiIndirect"],
        [Rgba16Float, Rgba16Float, Rgba16Float],
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 1.0],
        options.resolution_scale,
        [blur_uniforms([1.0, 0.0], None), blur_uniforms([0.0, 1.0], None)],
        "indirectLightingTexture",
    )
}

fn ssr(options: &SsrOptions) -> (Vec<RawTarget>, Vec<StepSpec>) {
    accumulated(
        ["ssrTrace", "ssrTemporal", "ssrDepthHistoryCopy", "ssrSpatial", "ssrComposite"],
        ["ssrRaw", "ssrHistory", "ssrHistoryDepth", "ssrScratch", "ssrReflection"],
        [Rgba16Float, Rgba16Float, Rgba16Float],
        [0.0, 0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0, 1.0],
        options.resolution_scale,
        [blur_uniforms([1.0, 0.0], None), blur_uniforms([0.0, 1.0], None)],
        "reflectionTexture",
    )
}

fn clustered(options: &ClusteredLightingOptions) -> (Vec<RawTarget>, Vec<StepSpec>) {
    let tuning = Some([3.0, 2.2, 0.018]);
    accumulated(
        [
            "clusteredVolumetricTrace",
            "clusteredVolumetricTemporal",
            "clusteredVolumetricDepthHistoryCopy",
            "depthAwareBlur",
            "clusteredVolumetricComposite",
        ],
        [
            "clusteredVolumeRaw",
            "clusteredVolumeHistory",
            "clusteredVolumeDepthHistory",
            "clusteredVolumeScratch",
            "clusteredVolumeScattering",
        ],
        [Rgba16Float, Rg16Float, Rgba16Float],
        [0.0, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, 1.0],
        options.resolution_scale,
        [blur_uniforms([1.0, 0.0], tuning), blur_uniforms([0.0, 1.0], tuning)],
        "scatteringTexture",
    )
}

fn volumetric_fog() -> (Vec<RawTarget>, Vec<StepSpec>) {
    (
        vec![target("fogHistory", 1.0, None, ORIGINAL)],
        vec![
            step(
                "volumetricFog",
                &[("sourceTexture", Previous), ("historyTexture", Target("fogHistory"))],
                Some("fogHistory"),
                vec![],
            ),
            step("advancedCopy", &[("sourceTexture", Target("fogHistory"))], None, vec![]),
        ],
    )
}

fn depth_aware_blur(options: &crate::scene_effects::config::DepthAwareBlurOptions) -> (Vec<RawTarget>, Vec<StepSpec>) {
    // Per-step tuning keeps the blur module's defaults for the AO and volume denoisers that share it.
    let tuned = |direction: [f32; 2]| -> UniformValues {
        vec![
            ("radius", vec![options.radius]),
            ("depthSigma", vec![options.depth_sigma]),
            ("spatialSigma", vec![options.spatial_sigma]),
            ("direction", direction.to_vec()),
        ]
    };
    (
        vec![target("depthAwareBlurScratch", 1.0, None, TRANSIENT)],
        vec![
            step("depthAwareBlur", &[("sourceTexture", Previous)], Some("depthAwareBlurScratch"), tuned([1.0, 0.0])),
            step("depthAwareBlur", &[("sourceTexture", Target("depthAwareBlurScratch"))], None, tuned([0.0, 1.0])),
        ],
    )
}

fn outlines(normal_source: NormalSource) -> (Vec<RawTarget>, Vec<StepSpec>) {
    let normals = normal_source == NormalSource::NormalTexture;
    let mut inputs = vec![("sourceTexture", Previous)];
    if !normals {
        inputs.push(("normalTexture", Previous));
    }
    (
        vec![],
        vec![step("screenSpaceOutline", &inputs, None, vec![("useNormalTexture", vec![f32::from(u8::from(normals))])])],
    )
}

fn taa(reprojection: Reprojection) -> (Vec<RawTarget>, Vec<StepSpec>) {
    let (resolve, color, depth, depth_copy, color_format) = match reprojection {
        Reprojection::Velocity => ("taaResolve", "taaHistoryColor", "taaHistoryDepth", "depthHistoryCopy", None),
        Reprojection::Camera => (
            "cameraReprojectionTaaResolve",
            "cameraReprojectionTaaHistoryColor",
            "cameraReprojectionTaaHistoryDepth",
            "cameraReprojectionTaaDepthHistoryCopy",
            Some(Rgba16Float),
        ),
    };
    (
        vec![
            target(color, 1.0, color_format, ORIGINAL),
            target(depth, 1.0, Some(Rgba16Float), history([1.0, 0.0, 0.0, 1.0])),
        ],
        vec![
            step(
                resolve,
                &[
                    ("sourceTexture", Previous),
                    ("historyTexture", Target(color)),
                    ("previousDepthTexture", Target(depth)),
                ],
                Some(color),
                vec![],
            ),
            step("advancedCopy", &[("sourceTexture", Target(color))], None, vec![]),
            step(depth_copy, &[("sourceTexture", Previous)], Some(depth), vec![]),
        ],
    )
}

fn adaptive_exposure(metering_scale: f32, initial_exposure: f32) -> (Vec<RawTarget>, Vec<StepSpec>) {
    let quarter = metering_scale.max(0.25);
    let initial = initial_exposure.max(0.0001);
    let sixteenth = quarter / 4.0;
    let sixty_fourth = sixteenth / 4.0;
    let two_fifty_sixth = sixty_fourth / 4.0;
    let thousandth = two_fifty_sixth / 4.0;
    let levels = [
        ("hdrLuminanceQuarter", quarter),
        ("hdrLuminanceSixteenth", sixteenth),
        ("hdrLuminanceSixtyFourth", sixty_fourth),
        ("hdrLuminanceTwoFiftySixth", two_fifty_sixth),
        ("hdrLuminanceThousandth", thousandth),
    ];
    let mut targets: Vec<RawTarget> =
        levels.iter().map(|(name, scale)| target(name, *scale, Some(Rgba16Float), TRANSIENT)).collect();
    targets.push(target("hdrExposureHistory", thousandth, Some(Rgba16Float), history([initial, 1.0, 1.0, 1.0])));
    let mut steps = vec![step("hdrLuminanceExtract", &[("sourceTexture", Previous)], Some(levels[0].0), vec![])];
    for pair in levels.windows(2) {
        steps.push(step("hdrLuminanceReduce", &[("sourceTexture", Target(pair[0].0))], Some(pair[1].0), vec![]));
    }
    steps.push(step(
        "hdrAutoExposureAdapt",
        &[("sourceTexture", Target("hdrLuminanceThousandth")), ("historyTexture", Target("hdrExposureHistory"))],
        Some("hdrExposureHistory"),
        vec![],
    ));
    steps.push(step(
        "hdrAutoExposureApply",
        &[("sourceTexture", Previous), ("exposureTexture", Target("hdrExposureHistory"))],
        None,
        vec![],
    ));
    (targets, steps)
}

/// Apply the Joy overrides to a pipeline's targets.
fn finish(
    name: &'static str,
    (targets, steps): (Vec<RawTarget>, Vec<StepSpec>),
    color: wgpu::TextureFormat,
) -> PipelineSpec {
    let targets = targets
        .into_iter()
        .map(|raw| {
            let precise = RAW_DEPTH_HISTORY.contains(&raw.name);
            TargetSpec {
                name: raw.name,
                scale: raw.scale,
                format: if precise { wgpu::TextureFormat::Rgba32Float } else { raw.format.unwrap_or(color) },
                sampling: if precise { TargetSampling::Nearest } else { TargetSampling::Linear },
                lifetime: raw.lifetime,
            }
        })
        .collect();
    PipelineSpec { name, targets, steps }
}

/// Build the enabled effects' pipelines in the TypeScript order.
pub(crate) fn scene_effect_pipelines(options: &SceneEffectsOptions, color: wgpu::TextureFormat) -> PipelineGroups {
    let mut groups = PipelineGroups::default();
    if let Some(ssao_options) = &options.ssao {
        groups.lighting.push(finish("ssaoShaderPassPipeline", ssao(ssao_options), color));
    }
    if let Some(gtao_options) = &options.gtao {
        groups.lighting.push(finish("gtaoShaderPassPipeline", gtao(gtao_options), color));
    }
    if let Some(ssgi_options) = &options.ssgi {
        groups.lighting.push(finish("ssgiShaderPassPipeline", ssgi(ssgi_options), color));
    }
    if let Some(ssr_options) = &options.ssr {
        groups.lighting.push(finish("ssrShaderPassPipeline", ssr(ssr_options), color));
    }
    if let Some(clustered_options) = &options.clustered_lighting {
        groups.lighting.push(finish(
            "clusteredVolumetricLightingShaderPassPipeline",
            clustered(clustered_options),
            color,
        ));
    }
    if options.height_fog.is_some() {
        groups.lighting.push(finish("volumetricFogShaderPassPipeline", volumetric_fog(), color));
    }
    if let Some(blur) = &options.depth_aware_blur {
        groups.lighting.push(finish("depthAwareBlurShaderPassPipeline", depth_aware_blur(blur), color));
    }
    if let Some(outline_options) = &options.outlines {
        groups.lighting.push(finish("outlineShaderPassPipeline", outlines(outline_options.normal_source), color));
    }
    if let Some(taa_options) = &options.taa {
        let name = match taa_options.reprojection {
            Reprojection::Velocity => "taaShaderPassPipeline",
            Reprojection::Camera => "cameraReprojectionTaaShaderPassPipeline",
        };
        groups.temporal.push(finish(name, taa(taa_options.reprojection), color));
    }
    if options.motion_blur.is_some() {
        groups.finishing.push(finish(
            "motionBlurShaderPassPipeline",
            (vec![], vec![step("motionBlur", &[], None, vec![])]),
            color,
        ));
    }
    if let Some(exposure) = &options.adaptive_exposure {
        groups.finishing.push(finish(
            "hdrAutoExposureShaderPassPipeline",
            adaptive_exposure(exposure.metering_scale, exposure.initial_exposure),
            color,
        ));
    }
    groups
}

#[cfg(test)]
#[path = "pipelines.test.rs"]
mod tests;
