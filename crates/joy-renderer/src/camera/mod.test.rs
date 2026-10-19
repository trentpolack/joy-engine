// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/camera/mod.rs`.

use super::*;

#[test]
fn projection_and_picking_round_trip() {
    let camera = OrbitCamera::default();
    let frame = camera.camera_frame(16.0 / 9.0);
    let viewport = Vec2::new(1600.0, 900.0);

    // The orbit target projects to the viewport center.
    let center = project_to_viewport(camera.target, frame.view_projection(), viewport).unwrap();
    assert!((center - (viewport * 0.5)).length() < 0.01);

    // A ray through the center hits the ground plane at the target.
    let ray = screen_ray(viewport * 0.5, frame.view_projection(), viewport).unwrap();
    let hit = intersect_ray_plane(ray, Vec3::Y, 0.0).unwrap();
    assert!(hit.length() < 0.01);
}

#[test]
fn region_cameras_draw_into_their_region_with_its_aspect() {
    let camera = OrbitCamera::default();
    let viewport = Vec2::new(1280.0, 800.0);
    let (region_min, region_size) = (Vec2::new(320.0, 100.0), Vec2::new(700.0, 250.0));
    let region = RegionCamera::new(&camera, viewport, region_min, region_size);
    let frame = region.camera_frame(viewport.x / viewport.y);

    // The target lands at the region's center, and picking through the whole viewport returns to it.
    let center = project_to_viewport(camera.target, frame.view_projection(), viewport).unwrap();
    assert!((center - (region_min + (region_size * 0.5))).length() < 0.01, "{center:?}");
    let ray = screen_ray(center, frame.view_projection(), viewport).unwrap();
    assert!(intersect_ray_plane(ray, Vec3::Y, 0.0).unwrap().length() < 0.01);

    // A point at the region camera's own frustum edge projects to the region's edge: the image fills the region
    // exactly as the same camera would fill a canvas of the region's size.
    let alone = camera.camera_frame(region_size.x / region_size.y);
    let basis = camera.basis();
    let half_height = (camera.vertical_fov_radians * 0.5).tan() * camera.distance;
    let top = camera.target + (basis.up * half_height);
    let in_region = project_to_viewport(top, frame.view_projection(), viewport).unwrap();
    let in_canvas = project_to_viewport(top, alone.view_projection(), region_size).unwrap();
    assert!((in_region - (region_min + in_canvas)).length() < 0.05, "{in_region:?} vs {in_canvas:?}");
    assert!((in_region.y - region_min.y).abs() < 0.05);
    assert_eq!(frame.view, alone.view);

    let degenerate = RegionCamera::new(&camera, viewport, region_min, Vec2::ZERO).camera_frame(1.6);
    assert_eq!(degenerate, camera.camera_frame(1.6));
}

#[test]
fn shifted_cameras_center_the_image_between_insets() {
    let camera = OrbitCamera::default();
    let viewport = Vec2::new(1280.0, 800.0);
    let shifted = ShiftedCamera::between_insets(&camera, viewport.x, 0.0, 330.0);
    let frame = shifted.camera_frame(viewport.x / viewport.y);

    // The target lands at the center of the uncovered area, and picking through it returns to the target.
    let center = project_to_viewport(camera.target, frame.view_projection(), viewport).unwrap();
    assert!((center - Vec2::new((1280.0 - 330.0) * 0.5, 400.0)).length() < 0.01, "{center:?}");
    let ray = screen_ray(center, frame.view_projection(), viewport).unwrap();
    assert!(intersect_ray_plane(ray, Vec3::Y, 0.0).unwrap().length() < 0.01);

    // Perspective is unchanged: the view matrix and depth mapping match the unshifted camera.
    let original = camera.camera_frame(viewport.x / viewport.y);
    assert_eq!(frame.view, original.view);
    assert_eq!(frame.projection.z_axis.z, original.projection.z_axis.z);
    assert_eq!(ShiftedCamera::between_insets(&camera, 0.0, 10.0, 0.0).shift, Vec2::ZERO);
}

#[test]
fn behind_camera_points_do_not_project() {
    let camera = OrbitCamera::default();
    let frame = camera.camera_frame(1.0);
    let behind = camera.basis().eye + (camera.basis().backward * 10.0);
    assert!(project_to_viewport(behind, frame.view_projection(), Vec2::splat(100.0)).is_none());
}
