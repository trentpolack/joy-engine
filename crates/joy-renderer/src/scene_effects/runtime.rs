// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The scene effects one renderer runs, the order it runs them in, and which of them a device supports.
//!
//! Per frame, in the renderer's pass order:
//! 1. [`SceneEffectsRuntime::encode_volume`] after the shadow views: volumetric injection and integration.
//! 2. [`SceneEffectsRuntime::encode_lighting`] after the opaque scene: volumetric fog over opaque surfaces and the
//!    sky, before transparency draws (and fogs itself).
//! 3. [`SceneEffectsRuntime::encode_composite`] over the finished scene: temporal antialiasing, then motion blur over
//!    the antialiased image, then adaptive exposure metering the result.

use crate::lighting::LightTextures;
use crate::scene_effects::config::{
    ADAPTIVE_EXPOSURE_NAME,
    MOTION_BLUR_NAME,
    SCENE_EFFECT_NAMES,
    TAA_NAME,
    VOLUMETRICS_NAME,
};
use crate::scene_effects::exposure::AdaptiveExposure;
use crate::scene_effects::motion_blur::MotionBlur;
use crate::scene_effects::temporal::TemporalAntialiasing;
use crate::scene_effects::volumetrics::Volumetrics;
use crate::scene_effects::{
    EffectsFrame,
    SceneEffectsOptions,
};

/// Whether one scene effect runs with the current options on this device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneEffectCapability {
    /// Effect name in its camelCase options spelling, such as `motionBlur`.
    pub name: &'static str,
    /// The options enable it.
    pub requested: bool,
    /// It runs on this device.
    pub available: bool,
    /// Why a requested effect does not run.
    pub reason: Option<&'static str>,
}

/// What the current device and options support.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneEffectsCapabilities {
    /// Whether this device runs any scene effect.
    pub available: bool,
    /// One entry per effect, in [`SCENE_EFFECT_NAMES`] order.
    pub effects: Vec<SceneEffectCapability>,
}

/// What scene effects need from a device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EffectSupport {
    /// Compute shaders and storage textures (every backend but WebGL 2).
    pub compute: bool,
    /// Float render targets that blend and filter (the HDR scene format); motion vectors and the fallback exposure
    /// meter render into them.
    pub float_targets: bool,
}

impl EffectSupport {
    /// Why the effect named `name` cannot run here, or `None` when it can.
    fn missing(self, name: &str) -> Option<&'static str> {
        match name {
            TAA_NAME | MOTION_BLUR_NAME if !self.float_targets => Some(FLOAT_TARGETS_REQUIRED),
            ADAPTIVE_EXPOSURE_NAME if !self.float_targets && !self.compute => Some(FLOAT_TARGETS_REQUIRED),
            VOLUMETRICS_NAME if !self.compute => Some(COMPUTE_REQUIRED),
            _ => None,
        }
    }
}

/// Stable diagnostics for effects a device cannot run.
const FLOAT_TARGETS_REQUIRED: &str = "Requires float render targets.";
const COMPUTE_REQUIRED: &str = "Requires compute shaders (WebGPU or a native backend).";

/// Resources the effects share with the renderer at creation.
pub(crate) struct EffectsSetup<'a> {
    pub scene_format: wgpu::TextureFormat,
    pub support: EffectSupport,
    /// The renderer's frame uniform buffer and light textures, which volumetric injection reads.
    pub frame_buffer: &'a wgpu::Buffer,
    pub lights: &'a LightTextures,
    /// The shadow receiver layout, visible to compute when the device has it.
    pub shadow_layout: &'a wgpu::BindGroupLayout,
}

/// The enabled effects that run on this device, and the frame counter temporal jitter follows.
pub(crate) struct SceneEffectsRuntime {
    options: SceneEffectsOptions,
    temporal: Option<TemporalAntialiasing>,
    motion_blur: Option<MotionBlur>,
    exposure: Option<AdaptiveExposure>,
    volumetrics: Option<Volumetrics>,
    frame_index: u32,
}

impl SceneEffectsRuntime {
    /// Create the enabled effects this device supports; the others are skipped (see [`capabilities`]).
    pub(crate) fn new(device: &wgpu::Device, options: SceneEffectsOptions, setup: &EffectsSetup<'_>) -> Self {
        let runs = |name: &str, enabled: bool| enabled && setup.support.missing(name).is_none();
        let temporal =
            runs(TAA_NAME, options.taa.is_some()).then(|| TemporalAntialiasing::new(device, setup.scene_format));
        let motion_blur =
            runs(MOTION_BLUR_NAME, options.motion_blur.is_some()).then(|| MotionBlur::new(device, setup.scene_format));
        let exposure = runs(ADAPTIVE_EXPOSURE_NAME, options.adaptive_exposure.is_some())
            .then(|| AdaptiveExposure::new(device, setup.support.compute));
        let volumetrics = runs(VOLUMETRICS_NAME, options.volumetrics.is_some()).then(|| {
            Volumetrics::new(device, setup.frame_buffer, setup.lights, setup.shadow_layout, setup.scene_format)
        });
        Self {
            options,
            temporal,
            motion_blur,
            exposure,
            volumetrics,
            frame_index: 0,
        }
    }

    /// Frames encoded since creation; the temporal jitter sequence follows it.
    pub(crate) fn frame_index(&self) -> u32 {
        self.frame_index
    }

    /// Whether the raster projection jitters for temporal antialiasing.
    pub(crate) fn jitters(&self) -> bool {
        self.temporal.is_some()
    }

    /// Whether the scene pass writes the motion attachment.
    pub(crate) fn needs_motion_vectors(&self) -> bool {
        self.temporal.is_some() || self.motion_blur.is_some()
    }

    /// Whether an effect runs between opaque surfaces and transparency, so transparency needs its own pass.
    pub(crate) fn has_lighting_effects(&self) -> bool {
        self.volumetrics.is_some()
    }

    /// Restart every effect's history.
    pub(crate) fn reset_history(&mut self) {
        if let Some(temporal) = &mut self.temporal {
            temporal.reset();
        }
        if let Some(exposure) = &mut self.exposure {
            exposure.reset();
        }
        if let Some(volumetrics) = &mut self.volumetrics {
            volumetrics.reset();
        }
    }

    /// Size per-frame resources for a `size` render target before any pass binds them.
    pub(crate) fn prepare(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if let (Some(volumetrics), Some(options)) = (&mut self.volumetrics, &self.options.volumetrics) {
            volumetrics.prepare(device, size, options);
        }
    }

    /// The integrated fog volume and the view depth it covers, when volumetric fog runs; scene passes bind it in the
    /// frame group after [`prepare`](Self::prepare).
    pub(crate) fn fog(&self) -> Option<(&wgpu::TextureView, f32)> {
        let options = self.options.volumetrics.as_ref()?;
        let view = self.volumetrics.as_ref()?.integrated_view()?;
        Some((view, options.max_distance))
    }

    /// Volumetric injection and integration, after the shadow views (`shadows` is their receiver group) and before
    /// the depth pre-pass.
    pub(crate) fn encode_volume(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        shadows: &wgpu::BindGroup,
    ) {
        if let (Some(volumetrics), Some(options)) = (&mut self.volumetrics, &self.options.volumetrics) {
            volumetrics.encode_volume(queue, encoder, frame, options, shadows);
        }
    }

    /// Effects over the opaque scene, before transparency.
    pub(crate) fn encode_lighting(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
    ) {
        if let (Some(volumetrics), Some(options)) = (&mut self.volumetrics, &self.options.volumetrics) {
            volumetrics.encode_apply(device, queue, encoder, frame, options);
        }
    }

    /// Effects over the finished scene: temporal antialiasing, motion blur, and adaptive exposure, which copies its
    /// multiplier into `exposure_texture`. `motion` is the scene pass's motion attachment, when it wrote one. Advances
    /// the frame counter.
    pub(crate) fn encode_composite(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &EffectsFrame<'_>,
        motion: Option<&wgpu::TextureView>,
        exposure_texture: &wgpu::Texture,
    ) {
        if let (Some(temporal), Some(options), Some(motion)) = (&mut self.temporal, &self.options.taa, motion) {
            temporal.encode(device, queue, encoder, frame, motion, options);
        }
        if let (Some(blur), Some(options), Some(motion)) = (&mut self.motion_blur, &self.options.motion_blur, motion) {
            blur.encode(device, queue, encoder, frame, motion, options);
        }
        if let (Some(exposure), Some(options)) = (&mut self.exposure, &self.options.adaptive_exposure) {
            exposure.encode(device, queue, encoder, frame, options, exposure_texture);
        }
        self.frame_index = self.frame_index.wrapping_add(1);
    }
}

/// Which effects the options request and which of them run with `support`.
pub(crate) fn capabilities(options: &SceneEffectsOptions, support: EffectSupport) -> SceneEffectsCapabilities {
    let enabled: Vec<&str> = options.enabled_names().collect();
    let effects = SCENE_EFFECT_NAMES
        .iter()
        .map(|name| {
            let requested = enabled.contains(name);
            let missing = support.missing(name);
            SceneEffectCapability {
                name,
                requested,
                available: requested && missing.is_none(),
                reason: missing.filter(|_| requested),
            }
        })
        .collect();
    SceneEffectsCapabilities {
        available: support.compute || support.float_targets,
        effects,
    }
}

#[cfg(test)]
#[path = "runtime.test.rs"]
mod tests;
