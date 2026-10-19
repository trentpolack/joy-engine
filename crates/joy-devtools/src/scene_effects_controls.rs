// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Development controls that preview scene effects on a running renderer: presets, quality, per-effect toggles, and
//! HDR bloom. Settings are temporary and never saved.
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
    ClusteredLightingOptions,
    GtaoOptions,
    SceneEffectsOptions,
    SsaoOptions,
    SsgiOptions,
    SsrOptions,
};

/// Effects in display order with their labels.
const EFFECTS: [(&str, &str); 11] = [
    ("depthAwareBlur", "Depth-aware blur"),
    ("ssao", "SSAO"),
    ("gtao", "GTAO"),
    ("ssgi", "SSGI color bleed"),
    ("outlines", "Outlines"),
    ("taa", "Temporal antialiasing"),
    ("motionBlur", "Motion blur"),
    ("ssr", "Screen-space reflections"),
    ("heightFog", "Screen-height fog approximation"),
    ("clusteredLighting", "Clustered volumetric lighting"),
    ("adaptiveExposure", "Adaptive exposure"),
];

/// Effect cost and resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectQuality {
    /// Half resolution, fewer samples.
    Low,
    /// Half resolution, default samples.
    Medium,
    /// Full resolution, more samples.
    High,
}

/// Named effect selections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectPreset {
    /// Nothing enabled.
    Off,
    /// Ambient occlusion and outlines.
    Contact,
    /// Fog, volumetric lighting, and adaptive exposure.
    Atmosphere,
    /// The full lighting set with antialiasing.
    Combined,
    /// A hand-picked selection.
    Custom,
}

impl EffectPreset {
    const ALL: [(EffectPreset, &'static str); 5] = [
        (EffectPreset::Off, "All off"),
        (EffectPreset::Contact, "Contact + edges"),
        (EffectPreset::Atmosphere, "Atmosphere"),
        (EffectPreset::Combined, "Combined"),
        (EffectPreset::Custom, "Custom"),
    ];

    fn effects(self) -> &'static [&'static str] {
        match self {
            Self::Off | Self::Custom => &[],
            Self::Contact => &["gtao", "outlines"],
            Self::Atmosphere => &["heightFog", "clusteredLighting", "adaptiveExposure"],
            Self::Combined => &["gtao", "ssgi", "taa", "ssr", "clusteredLighting", "adaptiveExposure"],
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

    /// Use `tuning` for enabled effects' settings at each quality instead of the engine's quality defaults, the way
    /// the TypeScript scene effects lab selects complete preset entries. Effects the tuning leaves `None` keep the
    /// defaults.
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
        let scale = if self.quality == EffectQuality::High { 1.0 } else { 0.5 };
        let pick = |low: f32, medium: f32, high: f32| match self.quality {
            EffectQuality::Low => low,
            EffectQuality::Medium => medium,
            EffectQuality::High => high,
        };
        let tuned = self.tuning.map_or_else(SceneEffectsOptions::default, |tuning| tuning(self.quality));
        let on = |name: &str| self.enabled.contains(&name);
        SceneEffectsOptions {
            depth_aware_blur: on("depthAwareBlur").then(|| tuned.depth_aware_blur.unwrap_or_default()),
            ssao: on("ssao")
                .then(|| tuned.ssao.unwrap_or(SsaoOptions { resolution_scale: scale, ..Default::default() })),
            gtao: on("gtao")
                .then(|| tuned.gtao.unwrap_or(GtaoOptions { resolution_scale: scale, ..Default::default() })),
            ssgi: on("ssgi").then(|| {
                tuned.ssgi.unwrap_or(SsgiOptions {
                    resolution_scale: scale,
                    ray_count: pick(3.0, 7.0, 12.0),
                    ..Default::default()
                })
            }),
            outlines: on("outlines").then(|| tuned.outlines.unwrap_or_default()),
            taa: on("taa").then(|| tuned.taa.unwrap_or_default()),
            motion_blur: on("motionBlur").then(|| tuned.motion_blur.unwrap_or_default()),
            ssr: on("ssr").then(|| {
                tuned.ssr.unwrap_or(SsrOptions {
                    resolution_scale: scale,
                    sample_count: pick(16.0, 48.0, 80.0),
                    ..Default::default()
                })
            }),
            height_fog: on("heightFog").then(|| tuned.height_fog.unwrap_or_default()),
            clustered_lighting: on("clusteredLighting").then(|| {
                tuned.clustered_lighting.unwrap_or(ClusteredLightingOptions {
                    resolution_scale: scale,
                    sample_count: pick(5.0, 10.0, 16.0),
                    ..Default::default()
                })
            }),
            adaptive_exposure: on("adaptiveExposure").then(|| tuned.adaptive_exposure.unwrap_or_default()),
            color_format: None,
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

    /// Enable or disable one effect; ambient-occlusion choices are exclusive.
    pub fn toggle(&mut self, name: &'static str, enabled: bool) {
        self.enabled.retain(|existing| *existing != name);
        if enabled {
            match name {
                "ssao" => self.enabled.retain(|existing| *existing != "gtao"),
                "gtao" => self.enabled.retain(|existing| *existing != "ssao"),
                _ => {}
            }
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
        self.status = if !report.available {
            "WebGL 2: scene effects require WebGPU and are unavailable here. HDR bloom remains available.".to_owned()
        } else {
            let format =
                report.color_format.map_or("display color".to_owned(), |format| format!("{format:?}").to_lowercase());
            let requested = if self.enabled.is_empty() && !self.bloom {
                "scene effects off".to_owned()
            } else {
                format!("{} scene effects requested", self.enabled.len())
            };
            format!("{} · {format} · {requested}.", renderer.diagnostics().backend)
        };
    }
}

#[cfg(test)]
#[path = "scene_effects_controls.test.rs"]
mod tests;
