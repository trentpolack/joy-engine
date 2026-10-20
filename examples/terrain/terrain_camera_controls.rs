// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Orbit, flight, zoom, and pan controls for the terrain example.
//!
//! Left-drag orbits around the focus. Holding the right button flies: pointer movement looks around, W/S move along
//! the view direction, A/D strafe, and Q/E move down and up in world Y. Scrolling zooms; Command (macOS) or Alt
//! (elsewhere) with scrolling pans in the view plane. Camera coordinates are Y-up world units, angles are radians,
//! and updates use seconds. The policy stays local to this example until another one needs it.

use std::collections::HashSet;

use joy_engine::math::{
    Vec2,
    Vec3,
};
use joy_engine::rendering::OrbitCamera;
use joy_engine::runtime::InputManager;

use crate::terrain_config::CameraControlsConfig;

/// Input action names bound by the example.
pub mod actions {
    /// Primary button: orbit drag.
    pub const ORBIT: &str = "orbit";
    /// Secondary button: flight.
    pub const FLY: &str = "fly";
    /// Fly forward along the view direction.
    pub const FORWARD: &str = "forward";
    /// Fly backward along the view direction.
    pub const BACKWARD: &str = "backward";
    /// Strafe left.
    pub const LEFT: &str = "left";
    /// Strafe right.
    pub const RIGHT: &str = "right";
    /// Move down in world Y.
    pub const DOWN: &str = "down";
    /// Move up in world Y.
    pub const UP: &str = "up";
    /// Every flight movement action.
    pub const MOVEMENT: [&str; 6] = [FORWARD, BACKWARD, LEFT, RIGHT, DOWN, UP];
}

/// One scroll gesture, already converted to DOM conventions: CSS pixels, positive Y scrolls down (zooms out).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelGesture {
    /// Scroll delta in CSS pixels.
    pub delta: Vec2,
    /// Whether Control was held; pinch gestures arrive as Control-scroll and are ignored.
    pub control: bool,
    /// Whether the pan modifier (Command on macOS, Alt elsewhere) was held.
    pub pan: bool,
}

/// Which button owns the current drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Drag {
    Orbit,
    Fly,
}

/// Camera control state. Borrows the camera per call and mutates it in place.
pub struct TerrainCameraControls {
    config: CameraControlsConfig,
    drag: Option<Drag>,
    pointer: Vec2,
    /// Movement keys pressed during the current flight; keys held before flight began do not move the camera.
    keys: HashSet<&'static str>,
}

impl TerrainCameraControls {
    /// Controls with the given speeds and limits.
    pub fn new(config: CameraControlsConfig) -> Self {
        Self {
            config,
            drag: None,
            pointer: Vec2::ZERO,
            keys: HashSet::new(),
        }
    }

    /// Whether the secondary button is flying the camera.
    pub fn flying(&self) -> bool {
        self.drag == Some(Drag::Fly)
    }

    /// Begin, continue, or end a drag from this frame's input, and track flight keys.
    pub fn input(&mut self, camera: &mut OrbitCamera, input: &InputManager) {
        let pointer = input.pointer().position;
        match self.drag {
            None => {
                // The first button pressed owns the drag; the other is ignored until release.
                if input.action(actions::ORBIT).pressed {
                    self.begin(Drag::Orbit, pointer);
                } else if input.action(actions::FLY).pressed {
                    self.begin(Drag::Fly, pointer);
                }
            }
            Some(drag) => {
                let held = match drag {
                    Drag::Orbit => input.action(actions::ORBIT).down,
                    Drag::Fly => input.action(actions::FLY).down,
                };
                if !held {
                    self.stop_flight();
                    return;
                }
                let delta = pointer - self.pointer;
                self.pointer = pointer;
                self.drag_by(camera, delta);
            }
        }
        if self.flying() {
            for action in actions::MOVEMENT {
                let state = input.action(action);
                if state.pressed {
                    self.keys.insert(action);
                } else if !state.down {
                    self.keys.remove(action);
                }
            }
        }
    }

    /// Translate the eye and focus together while flying; `elapsed_seconds` is the frame duration.
    ///
    /// Forward follows pitch as well as yaw; Q/E are world down/up. Combined input is normalized so diagonal flight
    /// has the same speed as a single direction.
    pub fn update(&mut self, camera: &mut OrbitCamera, elapsed_seconds: f32) {
        if !self.flying() {
            return;
        }
        let axis = |positive: &str, negative: &str| {
            f32::from(u8::from(self.keys.contains(positive))) - f32::from(u8::from(self.keys.contains(negative)))
        };
        let forward = axis(actions::FORWARD, actions::BACKWARD);
        let strafe = axis(actions::RIGHT, actions::LEFT);
        let vertical = axis(actions::UP, actions::DOWN);
        let basis = camera.basis();
        let direction = (basis.right * strafe) - (basis.backward * forward) + Vec3::new(0.0, vertical, 0.0);
        let length = direction.length();
        if length == 0.0 {
            return;
        }
        camera.target += direction * ((self.config.fly_speed * elapsed_seconds) / length);
    }

    /// Zoom or pan from one scroll gesture; `viewport_height` is the viewport height in CSS pixels.
    pub fn wheel(&mut self, camera: &mut OrbitCamera, gesture: WheelGesture, viewport_height: f32) {
        // Flight owns the camera, and pinch-generated Control-scroll never changes it.
        if self.flying() || gesture.control {
            return;
        }
        if gesture.pan {
            // Scale translation by the visible height at the focus distance so panning follows the gesture.
            let basis = camera.basis();
            let world_per_pixel =
                (2.0 * camera.distance * (self.config.vertical_fov_radians / 2.0).tan()) / viewport_height.max(1.0);
            camera.target += ((-basis.right * gesture.delta.x) + (basis.up * gesture.delta.y)) * world_per_pixel;
            return;
        }
        camera.distance = (camera.distance * (gesture.delta.y * self.config.zoom_sensitivity).exp())
            .clamp(self.config.min_distance, self.config.max_distance);
    }

    /// Cancel orbit or flight dragging and held flight keys without moving the camera.
    pub fn stop_flight(&mut self) {
        self.drag = None;
        self.keys.clear();
    }

    fn begin(&mut self, drag: Drag, pointer: Vec2) {
        self.drag = Some(drag);
        self.pointer = pointer;
        self.keys.clear();
    }

    /// Apply one pointer movement in CSS pixels.
    fn drag_by(&mut self, camera: &mut OrbitCamera, delta: Vec2) {
        let config = self.config;
        if !self.flying() {
            // Orbit: keep the focus fixed and rotate the eye around it with separate sensitivities and bounds.
            camera.yaw += delta.x * config.orbit_yaw_sensitivity;
            camera.pitch = (camera.pitch + (delta.y * config.orbit_pitch_sensitivity))
                .clamp(config.min_orbit_pitch, config.max_orbit_pitch);
            return;
        }

        // The orbit camera derives its eye from the focus, so rebuild the focus after rotating: mouse look pivots
        // around the eye instead of swinging the eye around the old focus.
        let eye = camera.basis().eye;
        camera.yaw -= delta.x * config.look_sensitivity;
        camera.pitch = (camera.pitch + (delta.y * config.look_sensitivity)).clamp(-config.max_pitch, config.max_pitch);
        camera.target = eye - (camera.basis().backward * camera.distance);
    }
}
