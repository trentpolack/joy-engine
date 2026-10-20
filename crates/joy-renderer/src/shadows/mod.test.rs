// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/shadows/mod.rs`.

use super::*;

fn camera() -> CameraFrame {
    let eye = Vec3::new(0.0, 8.0, 20.0);
    CameraFrame {
        view: look_at(eye, Vec3::ZERO, Vec3::Y),
        projection: perspective(1.0, 16.0 / 9.0, 0.1, 200.0),
        eye,
        focus: Vec3::ZERO,
        near: 0.1,
        far: 200.0,
    }
}

#[test]
fn cascade_splits_blend_uniform_and_logarithmic_spacing() {
    let splits = cascade_splits(0.1, 100.0, 3);
    // Halfway between the uniform splits (33.4, 66.7, 100) and the logarithmic ones (1, 10, 100).
    let expected = [17.2, 38.35, 100.0];
    for (split, expected) in splits.iter().zip(expected) {
        assert!((split - expected).abs() < 0.05, "{split} vs {expected}");
    }
    assert_eq!(splits[3], 100.0);
    assert!(splits.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn cascades_contain_their_slices_and_reach_only_the_casters_above_them() {
    let camera = camera();
    let splits = cascade_splits(camera.near, 100.0, 4);
    let direction = Vec3::new(0.3, 0.8, 0.2);
    let ground = Bounds {
        min: Vec3::new(-50.0, -0.5, -50.0),
        max: Vec3::new(50.0, 0.0, 50.0),
    };
    let fits = fit_cascades(&camera, direction, &splits, 4, 1024, 100.0, &[ground]);
    assert_eq!(fits.len(), 4);

    // Points along the view ray at each cascade's middle depth project inside that cascade's map.
    let inverse_view = camera.view.inverse();
    let mut near = camera.near;
    for (fit, far) in fits.iter().zip(splits) {
        assert!(fit.texel > 0.0);
        let depth = (near + far) * 0.5;
        let world = inverse_view.transform_point3(Vec3::new(0.0, 0.0, -depth));
        let clip = fit.matrix * world.extend(1.0);
        assert!(clip.x.abs() <= 1.0 && clip.y.abs() <= 1.0, "depth {depth}: {clip}");
        assert!((0.0..=1.0).contains(&clip.z), "depth {depth}: {}", clip.z);
        near = far;
    }

    // Without casters a cascade spans only its receivers; a tall caster above it stretches its depth range toward the
    // light to hold the caster, within the caster distance.
    let bare = fit_cascades(&camera, direction, &splits, 4, 1024, 100.0, &[]);
    let tower = Bounds {
        min: Vec3::new(-1.0, 0.0, 12.0),
        max: Vec3::new(1.0, 40.0, 14.0),
    };
    let towered = fit_cascades(&camera, direction, &splits, 4, 1024, 100.0, &[tower]);
    assert!(
        towered[0].depth_range > bare[0].depth_range + 10.0,
        "{} vs {}",
        towered[0].depth_range,
        bare[0].depth_range
    );
    let top = towered[0].matrix * Vec3::new(0.0, 40.0, 13.0).extend(1.0);
    assert!((0.0..=1.0).contains(&top.z), "the tower's top is inside the cascade: {}", top.z);
    let capped = fit_cascades(&camera, direction, &splits, 4, 1024, 5.0, &[tower]);
    assert!(capped[0].depth_range < towered[0].depth_range);
}

#[test]
fn point_faces_match_the_receiver_projection() {
    let position = Vec3::new(1.0, 2.0, -3.0);
    let range = 12.0;
    let views = point_face_views(position, range);
    let near = point_near_plane(range);
    for target in [Vec3::new(5.0, 2.5, -2.0), Vec3::new(0.0, -4.0, -2.0), Vec3::new(1.5, 2.0, -9.0)] {
        let offset = target - position;
        // The receiver's face selection and projection (shadow-receiver.wgsl `pointFace` and `pointFaceCoordinate`).
        let magnitude = offset.abs();
        let face = if magnitude.x >= magnitude.y && magnitude.x >= magnitude.z {
            if offset.x > 0.0 { 0 } else { 1 }
        } else if magnitude.y >= magnitude.z {
            if offset.y > 0.0 { 2 } else { 3 }
        } else if offset.z > 0.0 {
            4
        } else {
            5
        };
        let (forward, up) = POINT_FACE_ORIENTATIONS[face];
        let right = forward.cross(up);
        let major = forward.dot(offset);
        let uv = [(right.dot(offset) / major) * 0.5 + 0.5, 0.5 - ((up.dot(offset) / major) * 0.5)];
        let depth = (range / (range - near)) - ((range * near) / ((range - near) * major));

        let clip = views[face] * target.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        assert!((ndc.x * 0.5 + 0.5 - uv[0]).abs() < 1e-5 && (0.5 - ndc.y * 0.5 - uv[1]).abs() < 1e-5);
        assert!((ndc.z - depth).abs() < 1e-5, "{} vs {depth}", ndc.z);
        assert!(uv.iter().all(|value| (0.0..=1.0).contains(value)));
    }
}

#[test]
fn point_faces_follow_how_much_of_the_view_a_light_reaches() {
    let camera = camera();
    let largest = 256;
    // A light around the eye gets the largest face; one far away gets the smallest.
    assert_eq!(point_face_size(camera.eye, 5.0, &camera, 720, largest), largest);
    assert_eq!(point_face_size(Vec3::new(0.0, 0.0, -150.0), 1.0, &camera, 720, largest), MIN_POINT_FACE_SIZE);
    let middle = point_face_size(Vec3::new(0.0, 0.0, -10.0), 2.0, &camera, 720, largest);
    assert!(middle.is_power_of_two() && (MIN_POINT_FACE_SIZE..largest).contains(&middle), "{middle}");
}
