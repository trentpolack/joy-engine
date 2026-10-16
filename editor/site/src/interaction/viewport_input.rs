// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The level viewport's camera response to gestures. The authoring kit's
//! `development::viewport_input` reads the gestures (orbit with the primary button, pan with Shift or another button,
//! wheel zoom, F/0/arrow shortcuts, clicks); this module applies them to an [`OrbitCamera`] with the editor's speeds
//! and limits, and snaps placements to the grid. Deltas are logical pixels; wheel deltas follow the DOM
//! convention (positive scrolls toward the author and zooms out).

use joy_engine::development::{ViewportGesture, ViewportShortcut};
use joy_engine::glam::Vec3;
use joy_engine::rendering::OrbitCamera;

use super::constants::{MAXIMUM_PITCH, MINIMUM_PITCH, ORBIT_RADIANS_PER_PIXEL, PAN_SPEED_PER_DISTANCE};

/// Apply one drag step of `gesture` to `camera`; deltas are logical pixels.
pub fn drag_camera(camera: &mut OrbitCamera, gesture: ViewportGesture, dx: f32, dy: f32) {
    match gesture {
        ViewportGesture::Pan => {
            let basis = camera.basis();
            let speed = camera.distance * PAN_SPEED_PER_DISTANCE;
            camera.target += ((-basis.right * dx) + (basis.up * dy)) * speed;
        }
        ViewportGesture::Orbit => {
            camera.yaw -= dx * ORBIT_RADIANS_PER_PIXEL;
            camera.pitch = (camera.pitch + (dy * ORBIT_RADIANS_PER_PIXEL)).clamp(MINIMUM_PITCH, MAXIMUM_PITCH);
        }
    }
}

/// Zoom by a DOM-style wheel delta in pixels: positive moves the camera away. Distance stays between 1 and 400.
pub fn zoom_camera(camera: &mut OrbitCamera, delta_pixels: f32) {
    let delta = delta_pixels.clamp(-200.0, 200.0);
    camera.distance = (camera.distance * (delta * 0.001).exp()).clamp(1.0, 400.0);
}

/// Apply an orbit shortcut; frame and reset belong to the viewport owner, which knows its content.
pub fn orbit_shortcut(camera: &mut OrbitCamera, shortcut: ViewportShortcut) {
    match shortcut {
        ViewportShortcut::Left => camera.yaw -= 0.1,
        ViewportShortcut::Right => camera.yaw += 0.1,
        ViewportShortcut::Up => camera.pitch = (camera.pitch + 0.1).min(MAXIMUM_PITCH),
        ViewportShortcut::Down => camera.pitch = (camera.pitch - 0.1).max(MINIMUM_PITCH),
        ViewportShortcut::Frame | ViewportShortcut::Reset => {}
    }
}

/// Round each coordinate of `point` to the nearest multiple of `snap` meters; zero disables snapping.
pub fn snap_point(point: Vec3, snap: f32) -> Vec3 {
    if snap > 0.0 { (point / snap).round() * snap } else { point }
}

/// Round one coordinate to the nearest multiple of `snap`; zero disables snapping.
pub fn snap_value(value: f32, snap: f32) -> f32 {
    if snap > 0.0 { (value / snap).round() * snap } else { value }
}
