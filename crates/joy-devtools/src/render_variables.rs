// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The engine's `r.` console variables and how they reach the renderer: render scale, debug view, profiling, deferred
//! lighting, the sky, shadows, and each scene effect.
//!
//! The shell applies changed values before the game renders, then mirrors the renderer's actual state back, so the
//! console always reports what runs, including changes a game made itself. Turning shadows or a scene effect off parks
//! its settings, and turning it back on restores them rather than the defaults.

use joy_renderer::scene_effects::SceneEffectsOptions;
use joy_renderer::{
    DebugView,
    MAX_RENDER_SCALE,
    MIN_RENDER_SCALE,
    Renderer,
    ShadowOptions,
};

use crate::console_variables::{
    ConsoleVariables,
    VariableDefinition,
    VariableValue,
};

/// Scene resolution as a percentage of the drawing buffer along each axis.
pub const SCREEN_PERCENTAGE: &str = "r.ScreenPercentage";
/// What mesh surfaces draw as ([`DebugView::name`]).
pub const DEBUG_VIEW: &str = "r.DebugView";
/// Frame profiling ([`Renderer::set_profiling`]).
pub const PROFILE: &str = "r.Profile";
/// Deferred lighting where the device supports it ([`Renderer::set_deferred`]).
pub const DEFERRED: &str = "r.Deferred";
/// The procedural sky.
pub const ATMOSPHERE: &str = "r.Atmosphere";
/// Directional and point-light shadows.
pub const SHADOWS: &str = "r.Shadows";
/// Temporal antialiasing.
pub const TAA: &str = "r.TAA";
/// Motion blur.
pub const MOTION_BLUR: &str = "r.MotionBlur";
/// Adaptive exposure.
pub const ADAPTIVE_EXPOSURE: &str = "r.AdaptiveExposure";
/// Volumetric fog and lighting.
pub const VOLUMETRICS: &str = "r.Volumetrics";

/// The prefix every rendering variable shares; its changes belong to [`RenderVariables`].
const PREFIX: &str = "r.";

/// Percentages per unit of render scale.
const PERCENT: f64 = 100.0;

/// Register the rendering variables at the renderer's defaults.
pub(crate) fn register(variables: &mut ConsoleVariables) {
    let views: Vec<&str> = DebugView::ALL.iter().map(|view| view.name()).collect();
    let definitions = [
        VariableDefinition::float(
            SCREEN_PERCENTAGE,
            "Scene resolution as a percentage of the drawing buffer along each axis; effects and bloom follow it.",
            f64::from(MAX_RENDER_SCALE) * PERCENT,
            f64::from(MIN_RENDER_SCALE) * PERCENT,
            f64::from(MAX_RENDER_SCALE) * PERCENT,
        ),
        VariableDefinition::choice(
            DEBUG_VIEW,
            "What mesh surfaces draw as: the lit scene or one surface attribute.",
            &views,
            DebugView::Lit.name(),
        ),
        VariableDefinition::bool(
            PROFILE,
            "Time each frame phase on the CPU, and on the GPU where timestamp queries exist (see stat gpu).",
            false,
        ),
        VariableDefinition::bool(
            DEFERRED,
            "Shade opaque surfaces with deferred lighting where the device supports it (not WebGL 2); off shades forward.",
            true,
        ),
        VariableDefinition::bool(ATMOSPHERE, "Draw the procedural sky.", true),
        VariableDefinition::bool(SHADOWS, "Directional and point-light shadows.", true),
        VariableDefinition::bool(TAA, "Temporal antialiasing.", false),
        VariableDefinition::bool(MOTION_BLUR, "Camera and object motion blur.", false),
        VariableDefinition::bool(ADAPTIVE_EXPOSURE, "Exposure that adapts to the scene's brightness.", false),
        VariableDefinition::bool(VOLUMETRICS, "Volumetric fog and lighting (needs compute shaders).", false),
    ];
    for definition in definitions {
        variables.register(definition).expect("the engine's rendering variables are valid and registered once");
    }
}

/// Shadow and scene-effect settings parked while their variables turn them off.
#[derive(Clone, Debug, Default)]
pub(crate) struct RenderVariables {
    parked_shadows: Option<ShadowOptions>,
    parked_effects: SceneEffectsOptions,
}

impl RenderVariables {
    /// Apply changed rendering variables to `renderer`, then mirror its state into `variables`. Returns a message for
    /// each change the renderer refused.
    pub(crate) fn apply(&mut self, variables: &mut ConsoleVariables, renderer: &mut Renderer) -> Vec<String> {
        let mut refusals = Vec::new();
        for name in variables.take_changes(PREFIX) {
            let Some(value) = variables.get(&name).cloned() else {
                continue;
            };
            if let Err(message) = self.apply_one(&name, &value, renderer) {
                refusals.push(message);
            }
        }
        mirror(variables, renderer);
        refusals
    }

    fn apply_one(&mut self, name: &str, value: &VariableValue, renderer: &mut Renderer) -> Result<(), String> {
        match (name, value) {
            (SCREEN_PERCENTAGE, VariableValue::Float(percent)) => renderer.set_render_scale((percent / PERCENT) as f32),
            (DEBUG_VIEW, VariableValue::Text(view)) => {
                renderer.set_debug_view(DebugView::from_name(view).unwrap_or_default());
            }
            (PROFILE, VariableValue::Bool(enabled)) => renderer.set_profiling(*enabled),
            (DEFERRED, VariableValue::Bool(enabled)) => renderer.set_deferred(*enabled),
            (ATMOSPHERE, VariableValue::Bool(enabled)) => renderer.set_atmosphere(*enabled),
            (SHADOWS, VariableValue::Bool(enabled)) => self.set_shadows(*enabled, renderer),
            (TAA | MOTION_BLUR | ADAPTIVE_EXPOSURE | VOLUMETRICS, VariableValue::Bool(enabled)) => {
                return self.set_effect(name, *enabled, renderer);
            }
            _ => {}
        }
        Ok(())
    }

    fn set_shadows(&mut self, enabled: bool, renderer: &mut Renderer) {
        let current = renderer.shadow_options();
        if enabled && current.is_none() {
            renderer.set_shadow_options(Some(self.parked_shadows.take().unwrap_or_default()));
        } else if !enabled && current.is_some() {
            self.parked_shadows = current;
            renderer.set_shadow_options(None);
        }
    }

    /// Turn one scene effect on (with its parked settings, else its defaults) or off (parking its settings).
    fn set_effect(&mut self, name: &str, enabled: bool, renderer: &mut Renderer) -> Result<(), String> {
        let mut options = renderer.scene_effects().copied().unwrap_or_default();
        let parked = &mut self.parked_effects;
        match name {
            TAA => toggle(&mut options.taa, &mut parked.taa, enabled),
            MOTION_BLUR => toggle(&mut options.motion_blur, &mut parked.motion_blur, enabled),
            ADAPTIVE_EXPOSURE => toggle(&mut options.adaptive_exposure, &mut parked.adaptive_exposure, enabled),
            _ => toggle(&mut options.volumetrics, &mut parked.volumetrics, enabled),
        }
        renderer.set_scene_effects(Some(options)).map_err(|error| format!("{name}: {error}"))
    }
}

/// Enable `active` from `parked` (or the defaults), or park it.
fn toggle<T: Default>(active: &mut Option<T>, parked: &mut Option<T>, enabled: bool) {
    if enabled && active.is_none() {
        *active = Some(parked.take().unwrap_or_default());
    } else if !enabled && active.is_some() {
        *parked = active.take();
    }
}

/// Show the renderer's actual state in the rendering variables.
fn mirror(variables: &mut ConsoleVariables, renderer: &Renderer) {
    let percent = (f64::from(renderer.render_scale()) * PERCENT).round();
    variables.mirror(SCREEN_PERCENTAGE, VariableValue::Float(percent));
    variables.mirror(DEBUG_VIEW, VariableValue::Text(renderer.debug_view().name().to_owned()));
    variables.mirror(PROFILE, VariableValue::Bool(renderer.profiling()));
    variables.mirror(DEFERRED, VariableValue::Bool(renderer.deferred()));
    variables.mirror(ATMOSPHERE, VariableValue::Bool(renderer.atmosphere()));
    variables.mirror(SHADOWS, VariableValue::Bool(renderer.shadow_options().is_some()));
    let effects = renderer.scene_effects().copied().unwrap_or_default();
    variables.mirror(TAA, VariableValue::Bool(effects.taa.is_some()));
    variables.mirror(MOTION_BLUR, VariableValue::Bool(effects.motion_blur.is_some()));
    variables.mirror(ADAPTIVE_EXPOSURE, VariableValue::Bool(effects.adaptive_exposure.is_some()));
    variables.mirror(VOLUMETRICS, VariableValue::Bool(effects.volumetrics.is_some()));
}
