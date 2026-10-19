// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Screen-space scene effects: ambient occlusion (SSAO or GTAO), global illumination, reflections, outlines, height fog,
//! clustered volumetric lighting, depth-aware blur, temporal antialiasing, motion blur, and adaptive exposure.
//!
//! The shader passes are luma.gl's (`@luma.gl/effects`, MIT License, Copyright (c) vis.gl contributors; see
//! `LICENSE-luma.gl`), assembled by the engine's Rust shader-pass renderer.

mod attachments;
mod chain;
mod config;
mod effects;
mod pass;
mod passes;
mod pipelines;
mod volume_lights;

pub(crate) use attachments::SceneAttachments;
pub(crate) use effects::{
    EffectStage,
    EffectsFrame,
    SceneEffectsRuntime,
    capabilities,
};
pub(crate) use volume_lights::{
    VolumeCamera,
    VolumeLights,
};

pub use config::{
    AdaptiveExposureOptions,
    ClusteredLightingOptions,
    DepthAwareBlurOptions,
    GtaoOptions,
    HeightFogOptions,
    MotionBlurOptions,
    NormalSource,
    OutlineOptions,
    Reprojection,
    SCENE_EFFECT_NAMES,
    SceneEffectsColorFormat,
    SceneEffectsError,
    SceneEffectsOptions,
    SceneEffectsRequirements,
    SsaoOptions,
    SsgiOptions,
    SsrOptions,
    TaaOptions,
};
pub use effects::{
    SceneEffectCapability,
    SceneEffectsCapabilities,
};
pub use volume_lights::SceneVolumeStats;
