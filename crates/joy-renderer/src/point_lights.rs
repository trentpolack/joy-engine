// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Point-light selection: frames may supply any number of lights, and the renderer draws the ones that matter most to
//! the view.
//!
//! A light whose range sphere lies outside the camera frustum cannot light anything on screen (its contribution reaches
//! zero at its range), so it is skipped. The rest are ranked by an estimate of how strongly they light the view, their
//! luminance attenuated by how far the camera is from their reach, and the strongest [`MAX_POINT_LIGHTS`] are kept. Ties
//! keep the caller's order, so a stable light list draws the same lights every frame.

use joy_render_types::PointLight;

use crate::camera::CameraFrame;
use crate::culling::sphere_outside_view;
use crate::frame::MAX_POINT_LIGHTS;

/// The point lights a frame draws, strongest first, and how many it left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SelectedLights {
    /// At most [`MAX_POINT_LIGHTS`] valid lights that can reach the view, ranked by estimated contribution.
    pub(crate) lights: Vec<PointLight>,
    /// Lights skipped because they were invalid, could not reach the view, or fell below the cap.
    pub(crate) omitted: usize,
}

/// Choose the point lights to draw from the camera's point of view.
pub(crate) fn select_point_lights(lights: &[PointLight], camera: &CameraFrame) -> SelectedLights {
    let view_projection = camera.view_projection();
    let mut ranked: Vec<(f32, PointLight)> = lights
        .iter()
        .filter(|light| light.is_valid() && !sphere_outside_view(light.position, light.range, view_projection))
        .map(|light| (view_importance(light, camera), *light))
        .collect();

    // A stable sort keeps the caller's order among equal estimates.
    ranked.sort_by(|left, right| right.0.total_cmp(&left.0));
    ranked.truncate(MAX_POINT_LIGHTS);
    let selected: Vec<PointLight> = ranked.into_iter().map(|(_, light)| light).collect();
    SelectedLights {
        omitted: lights.len() - selected.len(),
        lights: selected,
    }
}

/// How strongly a light can light the view: its luminance, scaled down by the camera's distance from its reach
/// relative to that reach. A light whose range contains the eye counts fully; one twice its range away counts a quarter.
fn view_importance(light: &PointLight, camera: &CameraFrame) -> f32 {
    let gap = (light.position.distance(camera.eye) - light.range).max(0.0);
    let reach = light.range / (light.range + gap);
    light.luminance() * reach * reach
}

#[cfg(test)]
#[path = "point_lights.test.rs"]
mod tests;
