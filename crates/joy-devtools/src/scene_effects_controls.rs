// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Development controls that preview scene effects on a running renderer: presets, quality, per-effect toggles, and
//! HDR bloom. Settings are temporary and never saved.
//!
//! The status line names any requested effect this device cannot run (volumetric fog needs compute shaders, which
//! WebGL 2 lacks), so authors see the fallback instead of a silently missing effect.
//!
//! Games draw [`ui`](SceneEffectsControls::ui) inside a HUD panel during the UI phase and call
//! [`apply`](SceneEffectsControls::apply) with the renderer during the render phase, which commits any change. The bloom
//! toggle adjusts post-processing, which games set every frame, so they pass their settings through
//! [`post_process`](SceneEffectsControls::post_process) before handing them to the renderer.
//! Labs that compare effects on purpose-built scenes supply their own per-quality settings with
//! [`with_tuning`](SceneEffectsControls::with_tuning).

use egui::RichText;

use crate::dock;
use joy_renderer::Renderer;
use joy_renderer::post_process::{
    BloomQuality,
    PostProcessSettings,
};
use joy_renderer::scene_effects::{
    ADAPTIVE_EXPOSURE_NAME,
    MOTION_BLUR_NAME,
    MotionBlurOptions,
    SceneEffectsOptions,
    TAA_NAME,
    VOLUMETRICS_NAME,
    VolumetricOptions,
    VolumetricQuality,
};

/// Effects in display order with their labels.
const EFFECTS: [(&str, &str); 4] = [
    (TAA_NAME, "Temporal antialiasing"),
    (MOTION_BLUR_NAME, "Motion blur"),
    (ADAPTIVE_EXPOSURE_NAME, "Adaptive exposure"),
    (VOLUMETRICS_NAME, "Volumetric fog and lighting"),
];

/// Effect cost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectQuality {
    /// Fewer motion-blur samples and a coarse fog volume.
    Low,
    /// The engine defaults.
    Medium,
    /// More motion-blur samples and a fine fog volume.
    High,
}

/// Named effect selections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectPreset {
    /// Nothing enabled.
    Off,
    /// A camera's look: temporal antialiasing, motion blur, and adaptive exposure.
    Cinematic,
    /// Volumetric fog and lighting with adaptive exposure.
    Atmosphere,
    /// Every effect, with bloom.
    Combined,
    /// A hand-picked selection.
    Custom,
}

impl EffectPreset {
    const ALL: [(EffectPreset, &'static str); 5] = [
        (EffectPreset::Off, "All off"),
        (EffectPreset::Cinematic, "Cinematic"),
        (EffectPreset::Atmosphere, "Atmosphere"),
        (EffectPreset::Combined, "Combined"),
        (EffectPreset::Custom, "Custom"),
    ];

    fn effects(self) -> &'static [&'static str] {
        match self {
            Self::Off | Self::Custom => &[],
            Self::Cinematic => &[TAA_NAME, MOTION_BLUR_NAME, ADAPTIVE_EXPOSURE_NAME],
            Self::Atmosphere => &[VOLUMETRICS_NAME, ADAPTIVE_EXPOSURE_NAME],
            Self::Combined => &[TAA_NAME, MOTION_BLUR_NAME, ADAPTIVE_EXPOSURE_NAME, VOLUMETRICS_NAME],
        }
    }

    fn label(self) -> &'static str {
        Self::ALL.iter().find(|(preset, _)| *preset == self).map_or("Custom", |(_, label)| label)
    }
}

/// Per-quality effect settings: entries supply each enabled effect's options; `None` entries keep the defaults.
pub type EffectTuning = fn(EffectQuality) -> SceneEffectsOptions;

/// A requested renderer change waiting for the render phase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Pending {
    effects: bool,
    bloom: bool,
    quality: bool,
}

/// Scene effect preview controls.
#[derive(Clone, Debug)]
pub struct SceneEffectsControls {
    enabled: Vec<&'static str>,
    preset: EffectPreset,
    quality: EffectQuality,
    bloom: bool,
    bloom_strength: f32,
    /// Whether the author has chosen bloom or its quality here; until then post-processing passes through unchanged.
    bloom_chosen: bool,
    tuning: Option<EffectTuning>,
    default_open: bool,
    pending: Pending,
    synced: bool,
    status: String,
    error: bool,
}

impl Default for SceneEffectsControls {
    fn default() -> Self {
        Self {
            enabled: Vec::new(),
            preset: EffectPreset::Off,
            quality: EffectQuality::Medium,
            bloom: false,
            bloom_strength: 0.5,
            bloom_chosen: false,
            tuning: None,
            default_open: false,
            pending: Pending::default(),
            synced: false,
            status: "Waiting for renderer initialization…".to_owned(),
            error: false,
        }
    }
}

impl SceneEffectsControls {
    /// Controls with every effect off.
    pub fn new() -> Self {
        Self::default()
    }

    /// Use `tuning` for enabled effects' settings at each quality instead of the engine's quality defaults, so each
    /// quality selects a complete preset entry (the scene effects example does this). Effects the tuning leaves `None`
    /// keep the defaults.
    pub fn with_tuning(mut self, tuning: EffectTuning) -> Self {
        self.tuning = Some(tuning);
        self
    }

    /// Bloom strength the HDR bloom toggle applies (0.5 by default). A renderer that already blooms when the controls
    /// first sync keeps its own strength.
    pub fn with_bloom_strength(mut self, strength: f32) -> Self {
        self.bloom_strength = strength.max(0.0);
        self
    }

    /// Show the controls expanded the first time they are drawn, for tools whose main purpose is comparing effects.
    pub fn with_default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// The selected quality.
    pub fn quality(&self) -> EffectQuality {
        self.quality
    }

    /// The scene effects these controls request.
    pub fn configuration(&self) -> SceneEffectsOptions {
        let (blur_samples, fog_quality) = match self.quality {
            EffectQuality::Low => (8, VolumetricQuality::Low),
            EffectQuality::Medium => (12, VolumetricQuality::Medium),
            EffectQuality::High => (16, VolumetricQuality::High),
        };
        let tuned = self.tuning.map_or_else(SceneEffectsOptions::default, |tuning| tuning(self.quality));
        let on = |name: &str| self.enabled.contains(&name);
        SceneEffectsOptions {
            taa: on(TAA_NAME).then(|| tuned.taa.unwrap_or_default()),
            motion_blur: on(MOTION_BLUR_NAME).then(|| {
                tuned.motion_blur.unwrap_or(MotionBlurOptions {
                    sample_count: blur_samples,
                    ..Default::default()
                })
            }),
            adaptive_exposure: on(ADAPTIVE_EXPOSURE_NAME).then(|| tuned.adaptive_exposure.unwrap_or_default()),
            volumetrics: on(VOLUMETRICS_NAME).then(|| {
                tuned.volumetrics.unwrap_or(VolumetricOptions {
                    quality: fog_quality,
                    ..Default::default()
                })
            }),
        }
    }

    /// Draw the collapsible controls into a HUD panel.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let count = self.enabled.len() + usize::from(self.bloom);
        let summary = if count > 0 { format!("EFFECTS · {count} REQUESTED") } else { "EFFECTS · OFF".to_owned() };
        egui::CollapsingHeader::new(RichText::new(summary).monospace().small())
            .id_salt("joy-scene-effects")
            .default_open(self.default_open)
            .show(ui, |ui| {
                ui.label(RichText::new("Scene effects preview").strong());
                dock::note(ui, "Temporary viewport settings. Not saved with the level or project.");
                ui.label(RichText::new(&self.status).small().color(if self.error { dock::ERROR } else { dock::MUTED }));
                ui.add_enabled_ui(self.synced, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Preset");
                        let mut preset = self.preset;
                        egui::ComboBox::from_id_salt("joy-scene-effects-preset").selected_text(preset.label()).show_ui(
                            ui,
                            |ui| {
                                for (option, label) in EffectPreset::ALL {
                                    ui.selectable_value(&mut preset, option, label);
                                }
                            },
                        );
                        if preset != self.preset {
                            self.select_preset(preset);
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Quality");
                        let mut quality = self.quality;
                        let label = |quality: EffectQuality| match quality {
                            EffectQuality::Low => "Low",
                            EffectQuality::Medium => "Medium",
                            EffectQuality::High => "High",
                        };
                        egui::ComboBox::from_id_salt("joy-scene-effects-quality")
                            .selected_text(label(quality))
                            .show_ui(ui, |ui| {
                                for option in [EffectQuality::Low, EffectQuality::Medium, EffectQuality::High] {
                                    ui.selectable_value(&mut quality, option, label(option));
                                }
                            });
                        if quality != self.quality {
                            self.quality = quality;
                            self.pending.effects = true;
                            self.pending.quality = true;
                        }
                    });
                    for (name, label) in EFFECTS {
                        let mut checked = self.enabled.contains(&name);
                        if ui.checkbox(&mut checked, label).changed() {
                            self.toggle(name, checked);
                        }
                    }
                    let mut bloom = self.bloom;
                    if ui.checkbox(&mut bloom, "HDR bloom").changed() {
                        self.bloom = bloom;
                        self.pending.bloom = true;
                    }
                });
            });
    }

    /// Enable or disable one effect by its options name (such as [`TAA_NAME`]).
    pub fn toggle(&mut self, name: &'static str, enabled: bool) {
        self.enabled.retain(|existing| *existing != name);
        if enabled {
            self.enabled.push(name);
        }
        self.preset = EffectPreset::Custom;
        self.pending.effects = true;
    }

    /// Replace the selection with a preset; named presets also set bloom (on only for Combined).
    pub fn select_preset(&mut self, preset: EffectPreset) {
        self.preset = preset;
        if preset != EffectPreset::Custom {
            self.enabled = preset.effects().to_vec();
            self.bloom = preset == EffectPreset::Combined;
            self.pending.bloom = true;
        }
        self.pending.effects = true;
    }

    /// Sync from the renderer on first use, then commit pending changes and refresh the status line.
    pub fn apply(&mut self, renderer: &mut Renderer) {
        if !self.synced {
            // Preserve the renderer's display settings until the user opts in.
            let display = renderer.post_process();
            self.bloom = display.bloom_strength > 0.0;
            if self.bloom {
                self.bloom_strength = display.bloom_strength;
                self.preset = EffectPreset::Custom;
            }
            self.quality = match display.bloom_quality {
                BloomQuality::Low => EffectQuality::Low,
                BloomQuality::Medium => EffectQuality::Medium,
                BloomQuality::High | BloomQuality::Ultra => EffectQuality::High,
            };
            self.synced = true;
            self.update_status(renderer);
        }
        let pending = std::mem::take(&mut self.pending);
        if pending == Pending::default() {
            return;
        }
        if let Err(error) = renderer.set_scene_effects(Some(self.configuration())) {
            self.status = format!("Scene effects unavailable: {error}");
            self.error = true;
            return;
        }
        self.bloom_chosen |= pending.bloom || pending.quality;
        renderer.reset_history();
        self.update_status(renderer);
    }

    /// `settings` with the bloom choice made here applied: the HDR bloom toggle's strength and the selected quality.
    /// Settings pass through unchanged until the author changes bloom or quality.
    pub fn with_bloom_choice(&self, mut settings: PostProcessSettings) -> PostProcessSettings {
        if !self.bloom_chosen {
            return settings;
        }
        settings.bloom_strength = if self.bloom { self.bloom_strength } else { 0.0 };
        settings.bloom_quality = match self.quality {
            EffectQuality::Low => BloomQuality::Low,
            EffectQuality::Medium => BloomQuality::Medium,
            EffectQuality::High => BloomQuality::High,
        };
        settings
    }

    fn update_status(&mut self, renderer: &Renderer) {
        self.error = false;
        let report = renderer.scene_effects_capabilities();
        let backend = renderer.diagnostics().backend;
        if !report.available {
            self.status =
                format!("{backend}: scene effects need float render targets here. HDR bloom remains available.");
            return;
        }
        let requested = if self.enabled.is_empty() && !self.bloom {
            "scene effects off".to_owned()
        } else {
            format!("{} scene effects requested", self.enabled.len())
        };
        self.status = format!("{backend} · {requested}.");

        // Requested effects this device skips, with the reason.
        for effect in report.effects.iter().filter(|effect| effect.requested && !effect.available) {
            let label = EFFECTS.iter().find(|(name, _)| *name == effect.name).map_or(effect.name, |(_, label)| label);
            let reason = effect.reason.unwrap_or("Unavailable on this device.");
            self.status.push_str(&format!(" {label}: {reason}"));
        }
    }
}

#[cfg(test)]
#[path = "scene_effects_controls.test.rs"]
mod tests;
