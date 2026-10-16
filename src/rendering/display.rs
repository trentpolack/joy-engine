// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Drawing-buffer density policy.

/// Limits for [`backing_pixel_ratio`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackingLimits {
    /// Largest physical pixels per logical pixel.
    pub maximum_pixel_ratio: f32,
    /// Largest drawing-buffer area in physical pixels.
    pub maximum_backing_pixels: f32,
}

impl Default for BackingLimits {
    /// Ratio 2 and a 3840×2160 pixel budget, matching the TypeScript defaults.
    fn default() -> Self {
        Self { maximum_pixel_ratio: 2.0, maximum_backing_pixels: 3840.0 * 2160.0 }
    }
}

/// Physical pixels per logical pixel for a drawing buffer, independent of the logical viewport a game lays out in.
///
/// Display density changes only raster quality, never how much of the world is visible. The ratio is capped by
/// `limits` so very dense or very large displays do not allocate oversized targets, and never drops below 0.01.
pub fn backing_pixel_ratio(width: f32, height: f32, device_pixel_ratio: f32, limits: BackingLimits) -> f32 {
    let logical_pixels = width.max(1.0) * height.max(1.0);
    device_pixel_ratio
        .min(limits.maximum_pixel_ratio)
        .min((limits.maximum_backing_pixels / logical_pixels).sqrt())
        .max(0.01)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_is_capped_by_ratio_and_pixel_budget() {
        let limits = BackingLimits::default();
        assert_eq!(backing_pixel_ratio(1280.0, 720.0, 1.0, limits), 1.0);
        assert_eq!(backing_pixel_ratio(1280.0, 720.0, 3.0, limits), 2.0);
        // A 4K logical viewport at density 2 exceeds the budget and falls back to 1.
        assert!((backing_pixel_ratio(3840.0, 2160.0, 2.0, limits) - 1.0).abs() < 1e-6);
    }
}
