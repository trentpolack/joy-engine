// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene effects: temporal antialiasing, motion blur, adaptive exposure, and volumetric fog and lighting.
//!
//! Each effect owns its targets and passes; [`SceneEffectsRuntime`] runs the enabled ones in the renderer's pass order
//! (see `runtime.rs`). Every effect works on WebGPU and native backends. On WebGL 2, temporal antialiasing, motion
//! blur, and adaptive exposure need float render targets, and volumetric fog, which needs compute shaders, reports
//! itself unavailable.

mod attachments;
mod config;
mod exposure;
mod frame;
mod motion_blur;
mod runtime;
mod temporal;
mod volumetrics;

pub(crate) use attachments::SceneAttachments;
pub use config::{
    ADAPTIVE_EXPOSURE_NAME,
    AdaptiveExposureOptions,
    MOTION_BLUR_NAME,
    MotionBlurOptions,
    SCENE_EFFECT_NAMES,
    SceneEffectsError,
    SceneEffectsOptions,
    TAA_NAME,
    TaaOptions,
    VOLUMETRICS_NAME,
    VolumetricOptions,
    VolumetricQuality,
};
pub(crate) use frame::{
    EffectsFrame,
    copy_into_scene,
    effect_target,
    linear_sampler,
};
pub(crate) use runtime::{
    EffectSupport,
    EffectsSetup,
    SceneEffectsRuntime,
    capabilities,
};
pub use runtime::{
    SceneEffectCapability,
    SceneEffectsCapabilities,
};
