// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A cached editor reference grid on the XZ plane.
//!
//! Camera-facing ribbons keep lines readable across zoom and orbit. World units are never rescaled: minor spacing
//! follows a 1/2/5 sequence and major lines mark five divisions. The origin axes stay at world zero while the finite
//! grid follows pan. Colors are linear and every triangle is translucent, so submit the triangles as
//! [`FrameInputs::transparent_triangles`](crate::FrameInputs) and they never become lit surfaces or write
//! depth over authored geometry.

use crate::RenderError;
use crate::frame::ColorVertex;
use joy_core::math::deterministic;

const HALF_LINE_COUNT: i64 = 24;
const MINOR_COLOR: [f64; 3] = [0.20, 0.25, 0.32];
const MAJOR_COLOR: [f64; 3] = [0.31, 0.38, 0.47];
const X_AXIS_COLOR: [f64; 3] = [0.60, 0.32, 0.20];
const Z_AXIS_COLOR: [f64; 3] = [0.24, 0.43, 0.60];

/// The camera the grid follows, in Y-up world coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorGridView {
    /// Camera orbit target.
    pub target: [f64; 3],
    /// Unit-or-longer direction from the target toward the eye.
    pub backward: [f64; 3],
    /// Distance from the target to the eye.
    pub distance: f64,
    /// Viewport height in pixels.
    pub viewport_height_pixels: f64,
    /// Height of the XZ reference plane.
    pub height: f64,
}

/// One frame of grid geometry.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditorGridFrame {
    /// Translucent unlit triangles, three vertices each.
    pub triangles: Vec<ColorVertex>,
    /// Minor spacing in world units; major spacing is five times this value.
    pub spacing: f64,
}

/// Owns the cached grid; unchanged cameras reuse the same geometry.
#[derive(Clone, Debug, Default)]
pub struct EditorGrid {
    key: Option<[f64; 8]>,
    cached: EditorGridFrame,
}

impl EditorGrid {
    /// An empty grid.
    pub fn new() -> Self {
        Self::default()
    }

    /// Geometry for `view`. Invalid views fail without replacing the cached frame.
    pub fn frame(&mut self, view: &EditorGridView) -> Result<&EditorGridFrame, RenderError> {
        let EditorGridView { target, backward, distance, viewport_height_pixels, height } = *view;
        let finite = |vector: [f64; 3]| vector.iter().all(|value| value.is_finite());
        if !distance.is_finite()
            || distance <= 0.0
            || !viewport_height_pixels.is_finite()
            || viewport_height_pixels <= 0.0
            || !height.is_finite()
            || !finite(target)
            || !finite(backward)
            || backward.iter().all(|value| *value == 0.0)
        {
            return Err(invalid());
        }
        let world_per_pixel = 2.0 * distance * deterministic::tan(std::f64::consts::PI / 8.0) / viewport_height_pixels;
        let desired = (world_per_pixel * 32.0).max(1e-6);
        let power = deterministic::powf(10_f64, deterministic::log10(desired).floor());
        let ratio = desired / power;
        let spacing = power
            * if ratio >= 5.0 {
                5.0
            } else if ratio >= 2.0 {
                2.0
            } else {
                1.0
            };
        let center_x = (target[0] / spacing).round();
        let center_z = (target[2] / spacing).round();
        let key = [spacing, center_x, center_z, world_per_pixel, height, backward[0], backward[1], backward[2]];
        if self.key == Some(key) {
            return Ok(&self.cached);
        }

        let mut triangles = Vec::new();
        let extent = HALF_LINE_COUNT as f64 * spacing;
        for line in -HALF_LINE_COUNT..=HALF_LINE_COUNT {
            let fade = 1.0 - (0.7 * line.abs() as f64 / HALF_LINE_COUNT as f64);
            for along_x in [true, false] {
                let division = (if along_x { center_z } else { center_x }) + line as f64;
                let position = division * spacing;
                let axis = division == 0.0;
                let major = division % 5.0 == 0.0;
                let color = match (axis, along_x, major) {
                    (true, true, _) => X_AXIS_COLOR,
                    (true, false, _) => Z_AXIS_COLOR,
                    (false, _, true) => MAJOR_COLOR,
                    (false, _, false) => MINOR_COLOR,
                };
                let half_width = world_per_pixel
                    * if axis {
                        0.9
                    } else if major {
                        0.65
                    } else {
                        0.4
                    };
                let alpha = (if axis {
                    0.9
                } else if major {
                    0.75
                } else {
                    0.55
                }) * fade;
                let (start, end) = if along_x {
                    ([center_x * spacing - extent, height, position], [center_x * spacing + extent, height, position])
                } else {
                    ([position, height, center_z * spacing - extent], [position, height, center_z * spacing + extent])
                };
                append_line(&mut triangles, start, end, backward, half_width, color, alpha)?;
            }
        }
        self.key = Some(key);
        self.cached = EditorGridFrame { triangles, spacing };
        Ok(&self.cached)
    }
}

fn invalid() -> RenderError {
    RenderError::InvalidFrame("Editor grid requires a finite camera, positive distance and viewport height.".into())
}

/// Two camera-facing triangles for a segment; edge-on lines project to nothing.
fn append_line(
    out: &mut Vec<ColorVertex>,
    start: [f64; 3],
    end: [f64; 3],
    backward: [f64; 3],
    half_width: f64,
    color: [f64; 3],
    alpha: f64,
) -> Result<(), RenderError> {
    let direction = [end[0] - start[0], end[1] - start[1], end[2] - start[2]];
    let side = [
        direction[1] * backward[2] - direction[2] * backward[1],
        direction[2] * backward[0] - direction[0] * backward[2],
        direction[0] * backward[1] - direction[1] * backward[0],
    ];
    let length = deterministic::hypot(deterministic::hypot(side[0], side[1]), side[2]);
    if length < 1e-10 {
        return Ok(());
    }
    let offset = side.map(|value| value * half_width / length);
    let corner = |point: [f64; 3], sign: f64| -> Result<[f32; 3], RenderError> {
        let values = [0, 1, 2].map(|axis| (point[axis] + sign * offset[axis]) as f32);
        if values.iter().all(|value| value.is_finite()) { Ok(values) } else { Err(invalid()) }
    };
    let corners = [corner(start, -1.0)?, corner(start, 1.0)?, corner(end, 1.0)?, corner(end, -1.0)?];
    let color = [color[0] as f32, color[1] as f32, color[2] as f32, alpha as f32];
    for index in [0, 1, 2, 0, 2, 3] {
        out.push(ColorVertex { position: corners[index], color });
    }
    Ok(())
}

#[cfg(test)]
#[path = "editor_grid.test.rs"]
mod tests;
