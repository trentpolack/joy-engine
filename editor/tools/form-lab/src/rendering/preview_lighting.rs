// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Preview-only inspection lighting and the procedural studio environment. Ported from `preview-lighting.ts`.
//!
//! These settings light the preview alone: they never enter a document, a `.formlab` save, or an export. The studio
//! environment is an original equirectangular image (two soft panels over a floor-to-ceiling gradient) that the engine
//! integrates into image-based lighting once; intensity and rotation are per-frame inputs that reuse it.

use joy_engine::core::js_math;
use joy_engine::rendering::{EnvironmentMapOptions, ShadowQuality};

/// Editor-only ambient irradiance under the studio light.
pub const EDITOR_AMBIENT: f32 = 0.03;

/// Linear background behind the preview.
pub const EDITOR_BACKGROUND: [f32; 4] = [0.08, 0.10, 0.13, 1.0];

/// Every preview lighting setting.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewLightingSettings {
    /// World-space direction toward the direct light (X); need not be normalized.
    pub direction_x: f64,
    /// Direction toward the direct light (Y).
    pub direction_y: f64,
    /// Direction toward the direct light (Z).
    pub direction_z: f64,
    /// Linear red of the direct light.
    pub color_r: f64,
    /// Linear green of the direct light.
    pub color_g: f64,
    /// Linear blue of the direct light.
    pub color_b: f64,
    /// Direct light intensity multiplier.
    pub intensity: f64,
    /// Studio environment radiance multiplier.
    pub environment_intensity: f64,
    /// Studio environment rotation about +Y, in radians.
    pub environment_rotation: f64,
    /// Whether the direct light casts shadows.
    pub casts_shadow: bool,
    /// Shadow quality preset.
    pub quality: ShadowQuality,
}

/// The inspection defaults (`DEFAULT_PREVIEW_LIGHTING`).
pub const DEFAULT_PREVIEW_LIGHTING: PreviewLightingSettings = PreviewLightingSettings {
    direction_x: 0.35,
    direction_y: 0.85,
    direction_z: 0.4,
    color_r: 1.0,
    color_g: 1.0,
    color_b: 1.0,
    intensity: 3.0,
    environment_intensity: 1.0,
    environment_rotation: 0.0,
    casts_shadow: true,
    quality: ShadowQuality::Low,
};

impl Default for PreviewLightingSettings {
    fn default() -> Self {
        DEFAULT_PREVIEW_LIGHTING
    }
}

/// A numeric lighting setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LightingKey {
    /// [`PreviewLightingSettings::direction_x`].
    DirectionX,
    /// [`PreviewLightingSettings::direction_y`].
    DirectionY,
    /// [`PreviewLightingSettings::direction_z`].
    DirectionZ,
    /// [`PreviewLightingSettings::color_r`].
    ColorR,
    /// [`PreviewLightingSettings::color_g`].
    ColorG,
    /// [`PreviewLightingSettings::color_b`].
    ColorB,
    /// [`PreviewLightingSettings::intensity`].
    Intensity,
    /// [`PreviewLightingSettings::environment_intensity`].
    EnvironmentIntensity,
    /// [`PreviewLightingSettings::environment_rotation`].
    EnvironmentRotation,
}

/// A numeric lighting control: its setting, title, bounds, and step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightingField {
    /// The setting.
    pub key: LightingKey,
    /// Control title.
    pub title: &'static str,
    /// Inclusive minimum.
    pub minimum: f64,
    /// Inclusive maximum.
    pub maximum: f64,
    /// Control step.
    pub step: f64,
}

/// The numeric controls in inspector order (`PREVIEW_LIGHTING_FIELDS`).
pub const PREVIEW_LIGHTING_FIELDS: [LightingField; 9] = [
    LightingField { key: LightingKey::DirectionX, title: "Direction X", minimum: -1.0, maximum: 1.0, step: 0.05 },
    LightingField { key: LightingKey::DirectionY, title: "Direction Y", minimum: -1.0, maximum: 1.0, step: 0.05 },
    LightingField { key: LightingKey::DirectionZ, title: "Direction Z", minimum: -1.0, maximum: 1.0, step: 0.05 },
    LightingField { key: LightingKey::ColorR, title: "Light red", minimum: 0.0, maximum: 4.0, step: 0.05 },
    LightingField { key: LightingKey::ColorG, title: "Light green", minimum: 0.0, maximum: 4.0, step: 0.05 },
    LightingField { key: LightingKey::ColorB, title: "Light blue", minimum: 0.0, maximum: 4.0, step: 0.05 },
    LightingField { key: LightingKey::Intensity, title: "Direct intensity", minimum: 0.0, maximum: 12.0, step: 0.1 },
    LightingField {
        key: LightingKey::EnvironmentIntensity,
        title: "Environment intensity",
        minimum: 0.0,
        maximum: 4.0,
        step: 0.1,
    },
    LightingField {
        key: LightingKey::EnvironmentRotation,
        title: "Environment rotation (rad)",
        minimum: 0.0,
        maximum: std::f64::consts::TAU,
        step: 0.05,
    },
];

impl PreviewLightingSettings {
    /// A numeric setting's value.
    pub fn get(&self, key: LightingKey) -> f64 {
        match key {
            LightingKey::DirectionX => self.direction_x,
            LightingKey::DirectionY => self.direction_y,
            LightingKey::DirectionZ => self.direction_z,
            LightingKey::ColorR => self.color_r,
            LightingKey::ColorG => self.color_g,
            LightingKey::ColorB => self.color_b,
            LightingKey::Intensity => self.intensity,
            LightingKey::EnvironmentIntensity => self.environment_intensity,
            LightingKey::EnvironmentRotation => self.environment_rotation,
        }
    }

    /// A copy with one numeric setting replaced.
    pub fn with(mut self, key: LightingKey, value: f64) -> Self {
        let slot = match key {
            LightingKey::DirectionX => &mut self.direction_x,
            LightingKey::DirectionY => &mut self.direction_y,
            LightingKey::DirectionZ => &mut self.direction_z,
            LightingKey::ColorR => &mut self.color_r,
            LightingKey::ColorG => &mut self.color_g,
            LightingKey::ColorB => &mut self.color_b,
            LightingKey::Intensity => &mut self.intensity,
            LightingKey::EnvironmentIntensity => &mut self.environment_intensity,
            LightingKey::EnvironmentRotation => &mut self.environment_rotation,
        };
        *slot = value;
        self
    }

    /// Validate before changing the accepted preview lighting (`validatePreviewLighting`).
    pub fn validate(&self) -> Result<(), String> {
        for field in PREVIEW_LIGHTING_FIELDS {
            let value = self.get(field.key);
            if !value.is_finite() || value < field.minimum || value > field.maximum {
                return Err(format!(
                    "{} must be between {} and {}.",
                    field.title,
                    joy_engine::core::format_js_number(field.minimum),
                    joy_engine::core::format_js_number(field.maximum)
                ));
            }
        }
        if joy_engine::core::js_hypot(&[self.direction_x, self.direction_y, self.direction_z]) < 0.0001 {
            return Err("Light direction must be nonzero.".into());
        }
        Ok(())
    }

    /// Unit direction toward the light.
    pub fn direction(&self) -> [f32; 3] {
        let length = joy_engine::core::js_hypot(&[self.direction_x, self.direction_y, self.direction_z]).max(1e-12);
        [(self.direction_x / length) as f32, (self.direction_y / length) as f32, (self.direction_z / length) as f32]
    }
}

/// The studio image's size and lights (`STUDIO_ENVIRONMENT`).
pub struct StudioEnvironment {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Radiance at the bottom row.
    pub floor: f64,
    /// Radiance at the top row.
    pub ceiling: f64,
    /// Panel radiance multiplier.
    pub panel_power: f64,
    /// Panel half-width as a fraction of the longitude.
    pub panel_width: f64,
    /// Panel half-height as a fraction of the latitude.
    pub panel_height: f64,
    /// Panel centers as longitude fractions.
    pub panels: [f64; 2],
    /// Panel linear colors.
    pub panel_colors: [[f64; 3]; 2],
}

/// The studio the TypeScript tool lights its preview with.
pub const STUDIO_ENVIRONMENT: StudioEnvironment = StudioEnvironment {
    width: 256,
    height: 128,
    floor: 0.045,
    ceiling: 0.22,
    panel_power: 7.0,
    panel_width: 0.08,
    panel_height: 0.55,
    panels: [0.12, 0.58],
    panel_colors: [[1.0, 0.89, 0.74], [0.65, 0.8, 1.0]],
};

/// Integration settings for the studio (specular 64, irradiance 16, BRDF 128, 64 samples), the engine defaults.
pub fn studio_environment_options() -> EnvironmentMapOptions {
    EnvironmentMapOptions { size: 64, irradiance_size: 16, brdf_lut_size: 128, sample_count: 64, ..Default::default() }
}

/// Generate the studio's linear HDR equirectangular RGBA pixels, top row first (`createStudioPixels`).
pub fn create_studio_pixels() -> Vec<f32> {
    let config = &STUDIO_ENVIRONMENT;
    let (width, height) = (config.width as usize, config.height as usize);
    let mut data = vec![0.0_f32; width * height * 4];
    for y in 0..height {
        let latitude = (y as f64 + 0.5) / height as f64;
        let ambient = config.floor + ((config.ceiling - config.floor) * (1.0 - latitude));
        for x in 0..width {
            let longitude = (x as f64 + 0.5) / width as f64;
            let offset = ((y * width) + x) * 4;
            for channel in 0..3 {
                let mut radiance = ambient;
                for (panel, center) in config.panels.iter().enumerate() {
                    let distance = (longitude - center).abs().min(1.0 - (longitude - center).abs());
                    let vertical = (latitude - 0.38) / config.panel_height;
                    let shape = js_math::exp(-(distance / config.panel_width).powi(8) - vertical.powi(8));
                    radiance += config.panel_power * config.panel_colors[panel][channel] * shape;
                }
                data[offset + channel] = radiance as f32;
            }
            data[offset + 3] = 1.0;
        }
    }
    data
}

/// The engine shadow preset for a quality name used by the TypeScript tool (`low`, `balanced`, `cinematic`).
pub fn quality_name(quality: ShadowQuality) -> &'static str {
    match quality {
        ShadowQuality::Low => "low",
        ShadowQuality::Balanced => "balanced",
        ShadowQuality::Cinematic => "cinematic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_validate_like_the_typescript_tool() {
        assert!(DEFAULT_PREVIEW_LIGHTING.validate().is_ok());
        let error = DEFAULT_PREVIEW_LIGHTING.with(LightingKey::Intensity, 13.0).validate().unwrap_err();
        assert_eq!(error, "Direct intensity must be between 0 and 12.");
        let dark = DEFAULT_PREVIEW_LIGHTING
            .with(LightingKey::DirectionX, 0.0)
            .with(LightingKey::DirectionY, 0.0)
            .with(LightingKey::DirectionZ, 0.0);
        assert_eq!(dark.validate().unwrap_err(), "Light direction must be nonzero.");
        assert!(DEFAULT_PREVIEW_LIGHTING.with(LightingKey::ColorR, f64::NAN).validate().is_err());
    }

    #[test]
    fn the_studio_has_two_bright_panels_over_a_gradient() {
        let pixels = create_studio_pixels();
        assert_eq!(pixels.len(), 256 * 128 * 4);
        let at = |x: usize, y: usize| pixels[((y * 256) + x) * 4];
        let panel = at((0.12 * 256.0) as usize, (0.38 * 128.0) as usize);
        let wall = at(64, (0.38 * 128.0) as usize);
        assert!(panel > 5.0 && wall < 0.5, "panel {panel}, wall {wall}");
        assert!(at(64, 0) > at(64, 127), "the ceiling is brighter than the floor");
    }
}
