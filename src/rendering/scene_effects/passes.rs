// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Screen-space shader passes ported from luma.gl 9.4.2 (`@luma.gl/effects`, MIT License, Copyright (c) vis.gl
//! contributors; see `LICENSE-luma.gl`). Each pass's WGSL lives in `shaders/`; this table records its uniform block
//! in declaration order with luma.gl's default values. The table was generated from the upstream npm package.

use crate::rendering::scene_effects::pass::{
    PassDefinition,
    UniformField,
    UniformKind,
};

/// Every pass the scene-effect pipelines run, sorted by name.
// One field per line keeps the generated table diffable; rustfmt would reflow the longer rows.
#[rustfmt::skip]
pub(crate) const PASSES: &[PassDefinition] = &[
    PassDefinition {
        name: "advancedCopy",
        source: include_str!("shaders/advanced_copy.wgsl"),
        uniforms: &[],
    },
    PassDefinition {
        name: "cameraReprojectionTaaDepthHistoryCopy",
        source: include_str!("shaders/camera_reprojection_taa_depth_history_copy.wgsl"),
        uniforms: &[],
    },
    PassDefinition {
        name: "cameraReprojectionTaaResolve",
        source: include_str!("shaders/camera_reprojection_taa_resolve.wgsl"),
        uniforms: &[
            UniformField { name: "inverseViewProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "previousViewProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "historyWeight", kind: UniformKind::F32, default: &[0.9] },
            UniformField { name: "depthThreshold", kind: UniformKind::F32, default: &[0.0025] },
            UniformField { name: "currentJitter", kind: UniformKind::Vec2, default: &[0.0, 0.0] },
            UniformField { name: "previousJitter", kind: UniformKind::Vec2, default: &[0.0, 0.0] },
        ],
    },
    PassDefinition {
        name: "clusteredVolumetricComposite",
        source: include_str!("shaders/clustered_volumetric_composite.wgsl"),
        uniforms: &[
            UniformField { name: "strength", kind: UniformKind::F32, default: &[0.9] },
            UniformField { name: "debugMode", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "clusteredVolumetricDepthHistoryCopy",
        source: include_str!("shaders/clustered_volumetric_depth_history_copy.wgsl"),
        uniforms: &[
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
        ],
    },
    PassDefinition {
        name: "clusteredVolumetricTemporal",
        source: include_str!("shaders/clustered_volumetric_temporal.wgsl"),
        uniforms: &[
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "inverseViewProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "previousViewProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "historyWeight", kind: UniformKind::F32, default: &[0.88] },
            UniformField { name: "depthThreshold", kind: UniformKind::F32, default: &[0.035] },
        ],
    },
    PassDefinition {
        name: "clusteredVolumetricTrace",
        source: include_str!("shaders/clustered_volumetric_trace.wgsl"),
        uniforms: &[
            UniformField { name: "projectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "inverseViewMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "directionalLightDirectionView", kind: UniformKind::Vec3, default: &[0.35, 0.82, 0.38] },
            UniformField { name: "directionalLightColor", kind: UniformKind::Vec3, default: &[1.0, 0.85, 0.66] },
            UniformField { name: "fogColor", kind: UniformKind::Vec3, default: &[0.18, 0.3, 0.48] },
            UniformField { name: "godRayPosition", kind: UniformKind::Vec2, default: &[0.72, 0.16] },
            UniformField { name: "density", kind: UniformKind::F32, default: &[0.055] },
            UniformField { name: "heightFalloff", kind: UniformKind::F32, default: &[0.23] },
            UniformField { name: "fogHeight", kind: UniformKind::F32, default: &[0.2] },
            UniformField { name: "anisotropy", kind: UniformKind::F32, default: &[0.45] },
            UniformField { name: "directionalIntensity", kind: UniformKind::F32, default: &[2.2] },
            UniformField { name: "pointLightIntensity", kind: UniformKind::F32, default: &[1.8] },
            UniformField { name: "godRayIntensity", kind: UniformKind::F32, default: &[0.0] },
            UniformField { name: "godRayDensity", kind: UniformKind::F32, default: &[0.94] },
            UniformField { name: "godRayDecay", kind: UniformKind::F32, default: &[0.96] },
            UniformField { name: "godRaySampleCount", kind: UniformKind::F32, default: &[18.0] },
            UniformField { name: "godRaysOnly", kind: UniformKind::F32, default: &[0.0] },
            UniformField { name: "maxDistance", kind: UniformKind::F32, default: &[28.0] },
            UniformField { name: "sampleCount", kind: UniformKind::F32, default: &[10.0] },
            UniformField { name: "shadowStrength", kind: UniformKind::F32, default: &[0.74] },
            UniformField { name: "clusterCountX", kind: UniformKind::U32, default: &[1.0] },
            UniformField { name: "clusterCountY", kind: UniformKind::U32, default: &[1.0] },
            UniformField { name: "clusterCountZ", kind: UniformKind::U32, default: &[1.0] },
            UniformField { name: "maxLightsPerCluster", kind: UniformKind::U32, default: &[1.0] },
            UniformField { name: "pointLightCount", kind: UniformKind::U32, default: &[0.0] },
            UniformField { name: "clusterNearPlane", kind: UniformKind::F32, default: &[0.1] },
            UniformField { name: "clusterFarPlane", kind: UniformKind::F32, default: &[100.0] },
        ],
    },
    PassDefinition {
        name: "depthAwareBlur",
        source: include_str!("shaders/depth_aware_blur.wgsl"),
        uniforms: &[
            UniformField { name: "direction", kind: UniformKind::Vec2, default: &[1.0, 0.0] },
            UniformField { name: "radius", kind: UniformKind::F32, default: &[4.0] },
            UniformField { name: "depthSigma", kind: UniformKind::F32, default: &[0.01] },
            UniformField { name: "spatialSigma", kind: UniformKind::F32, default: &[3.0] },
        ],
    },
    PassDefinition {
        name: "depthHistoryCopy",
        source: include_str!("shaders/depth_history_copy.wgsl"),
        uniforms: &[],
    },
    PassDefinition {
        name: "gtaoComposite",
        source: include_str!("shaders/gtao_composite.wgsl"),
        uniforms: &[
            UniformField { name: "strength", kind: UniformKind::F32, default: &[0.68] },
            UniformField { name: "debugMode", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "gtaoDepthHistoryCopy",
        source: include_str!("shaders/gtao_depth_history_copy.wgsl"),
        uniforms: &[],
    },
    PassDefinition {
        name: "gtaoEvaluate",
        source: include_str!("shaders/gtao_evaluate.wgsl"),
        uniforms: &[
            UniformField { name: "projectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "radius", kind: UniformKind::F32, default: &[2.2] },
            UniformField { name: "bias", kind: UniformKind::F32, default: &[0.04] },
            UniformField { name: "intensity", kind: UniformKind::F32, default: &[3.2] },
            UniformField { name: "frameIndex", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "gtaoTemporal",
        source: include_str!("shaders/gtao_temporal.wgsl"),
        uniforms: &[
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "historyWeight", kind: UniformKind::F32, default: &[0.88] },
            UniformField { name: "depthThreshold", kind: UniformKind::F32, default: &[0.015] },
        ],
    },
    PassDefinition {
        name: "hdrAutoExposureAdapt",
        source: include_str!("shaders/hdr_auto_exposure_adapt.wgsl"),
        uniforms: &[
            UniformField { name: "keyValue", kind: UniformKind::F32, default: &[0.48] },
            UniformField { name: "minimumExposure", kind: UniformKind::F32, default: &[0.45] },
            UniformField { name: "maximumExposure", kind: UniformKind::F32, default: &[2.4] },
            UniformField { name: "brightenSpeed", kind: UniformKind::F32, default: &[1.6] },
            UniformField { name: "darkenSpeed", kind: UniformKind::F32, default: &[2.8] },
            UniformField { name: "deltaTime", kind: UniformKind::F32, default: &[0.016] },
            UniformField { name: "enabled", kind: UniformKind::F32, default: &[1.0] },
        ],
    },
    PassDefinition {
        name: "hdrAutoExposureApply",
        source: include_str!("shaders/hdr_auto_exposure_apply.wgsl"),
        uniforms: &[
            UniformField { name: "debugMode", kind: UniformKind::F32, default: &[0.0] },
            UniformField { name: "enabled", kind: UniformKind::F32, default: &[1.0] },
        ],
    },
    PassDefinition {
        name: "hdrLuminanceExtract",
        source: include_str!("shaders/hdr_luminance_extract.wgsl"),
        uniforms: &[],
    },
    PassDefinition {
        name: "hdrLuminanceReduce",
        source: include_str!("shaders/hdr_luminance_reduce.wgsl"),
        uniforms: &[],
    },
    PassDefinition {
        name: "motionBlur",
        source: include_str!("shaders/motion_blur.wgsl"),
        uniforms: &[
            UniformField { name: "strength", kind: UniformKind::F32, default: &[1.0] },
            UniformField { name: "sampleCount", kind: UniformKind::F32, default: &[10.0] },
        ],
    },
    PassDefinition {
        name: "screenSpaceOutline",
        source: include_str!("shaders/screen_space_outline.wgsl"),
        uniforms: &[
            UniformField { name: "color", kind: UniformKind::Vec4, default: &[0.02, 0.08, 0.12, 0.48] },
            UniformField { name: "thickness", kind: UniformKind::F32, default: &[1.5] },
            UniformField { name: "depthThreshold", kind: UniformKind::F32, default: &[0.003] },
            UniformField { name: "normalThreshold", kind: UniformKind::F32, default: &[0.18] },
            UniformField { name: "useNormalTexture", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "ssaoComposite",
        source: include_str!("shaders/ssao_composite.wgsl"),
        uniforms: &[
            UniformField { name: "debugMode", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "ssaoEvaluate",
        source: include_str!("shaders/ssao_evaluate.wgsl"),
        uniforms: &[
            UniformField { name: "nearPlane", kind: UniformKind::F32, default: &[0.1] },
            UniformField { name: "farPlane", kind: UniformKind::F32, default: &[200.0] },
            UniformField { name: "radius", kind: UniformKind::F32, default: &[7.0] },
            UniformField { name: "bias", kind: UniformKind::F32, default: &[0.03] },
            UniformField { name: "intensity", kind: UniformKind::F32, default: &[1.35] },
            UniformField { name: "useNormalTexture", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "ssgiComposite",
        source: include_str!("shaders/ssgi_composite.wgsl"),
        uniforms: &[
            UniformField { name: "strength", kind: UniformKind::F32, default: &[1.0] },
            UniformField { name: "debugMode", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "ssgiDepthHistoryCopy",
        source: include_str!("shaders/ssgi_depth_history_copy.wgsl"),
        uniforms: &[],
    },
    PassDefinition {
        name: "ssgiSpatial",
        source: include_str!("shaders/ssgi_spatial.wgsl"),
        uniforms: &[
            UniformField { name: "direction", kind: UniformKind::Vec2, default: &[1.0, 0.0] },
            UniformField { name: "radius", kind: UniformKind::F32, default: &[3.0] },
            UniformField { name: "depthSigma", kind: UniformKind::F32, default: &[0.012] },
        ],
    },
    PassDefinition {
        name: "ssgiTemporal",
        source: include_str!("shaders/ssgi_temporal.wgsl"),
        uniforms: &[
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "historyWeight", kind: UniformKind::F32, default: &[0.89] },
            UniformField { name: "depthThreshold", kind: UniformKind::F32, default: &[0.022] },
        ],
    },
    PassDefinition {
        name: "ssgiTrace",
        source: include_str!("shaders/ssgi_trace.wgsl"),
        uniforms: &[
            UniformField { name: "projectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "radius", kind: UniformKind::F32, default: &[4.5] },
            UniformField { name: "thickness", kind: UniformKind::F32, default: &[0.32] },
            UniformField { name: "intensity", kind: UniformKind::F32, default: &[2.2] },
            UniformField { name: "rayCount", kind: UniformKind::F32, default: &[7.0] },
            UniformField { name: "stepCount", kind: UniformKind::F32, default: &[8.0] },
            UniformField { name: "frameIndex", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "ssrComposite",
        source: include_str!("shaders/ssr_composite.wgsl"),
        uniforms: &[
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "strength", kind: UniformKind::F32, default: &[1.0] },
            UniformField { name: "debugMode", kind: UniformKind::F32, default: &[0.0] },
            UniformField { name: "depthSigma", kind: UniformKind::F32, default: &[0.04] },
        ],
    },
    PassDefinition {
        name: "ssrDepthHistoryCopy",
        source: include_str!("shaders/ssr_depth_history_copy.wgsl"),
        uniforms: &[],
    },
    PassDefinition {
        name: "ssrSpatial",
        source: include_str!("shaders/ssr_spatial.wgsl"),
        uniforms: &[
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "direction", kind: UniformKind::Vec2, default: &[1.0, 0.0] },
            UniformField { name: "maxRadius", kind: UniformKind::F32, default: &[5.0] },
            UniformField { name: "depthSigma", kind: UniformKind::F32, default: &[0.035] },
        ],
    },
    PassDefinition {
        name: "ssrTemporal",
        source: include_str!("shaders/ssr_temporal.wgsl"),
        uniforms: &[
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "historyWeight", kind: UniformKind::F32, default: &[0.86] },
            UniformField { name: "depthThreshold", kind: UniformKind::F32, default: &[0.018] },
        ],
    },
    PassDefinition {
        name: "ssrTrace",
        source: include_str!("shaders/ssr_trace.wgsl"),
        uniforms: &[
            UniformField { name: "projectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "inverseProjectionMatrix", kind: UniformKind::Mat4, default: &[1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0] },
            UniformField { name: "intensity", kind: UniformKind::F32, default: &[1.35] },
            UniformField { name: "maxDistance", kind: UniformKind::F32, default: &[60.0] },
            UniformField { name: "thickness", kind: UniformKind::F32, default: &[0.45] },
            UniformField { name: "sampleCount", kind: UniformKind::F32, default: &[48.0] },
            UniformField { name: "maxRoughness", kind: UniformKind::F32, default: &[0.88] },
            UniformField { name: "frameIndex", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
    PassDefinition {
        name: "taaResolve",
        source: include_str!("shaders/taa_resolve.wgsl"),
        uniforms: &[
            UniformField { name: "historyWeight", kind: UniformKind::F32, default: &[0.9] },
            UniformField { name: "depthThreshold", kind: UniformKind::F32, default: &[0.01] },
            UniformField { name: "currentJitter", kind: UniformKind::Vec2, default: &[0.0, 0.0] },
            UniformField { name: "previousJitter", kind: UniformKind::Vec2, default: &[0.0, 0.0] },
        ],
    },
    PassDefinition {
        name: "volumetricFog",
        source: include_str!("shaders/volumetric_fog.wgsl"),
        uniforms: &[
            UniformField { name: "fogColor", kind: UniformKind::Vec4, default: &[0.18, 0.34, 0.48, 0.6] },
            UniformField { name: "density", kind: UniformKind::F32, default: &[0.22] },
            UniformField { name: "heightFalloff", kind: UniformKind::F32, default: &[3.0] },
            UniformField { name: "scattering", kind: UniformKind::F32, default: &[0.35] },
            UniformField { name: "historyWeight", kind: UniformKind::F32, default: &[0.82] },
            UniformField { name: "time", kind: UniformKind::F32, default: &[0.0] },
        ],
    },
];
