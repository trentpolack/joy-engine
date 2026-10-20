// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/lighting/clusters.rs`.

use joy_core::LinearColor;

use super::*;
use crate::camera::{
    look_at,
    perspective,
};

fn camera() -> CameraFrame {
    let eye = Vec3::new(0.0, 6.0, 18.0);
    CameraFrame {
        view: look_at(eye, Vec3::ZERO, Vec3::Y),
        projection: perspective(1.0, 16.0 / 9.0, 0.1, 300.0),
        eye,
        focus: Vec3::ZERO,
        near: 0.1,
        far: 300.0,
    }
}

fn light(position: Vec3, range: f32) -> PointLight {
    PointLight {
        position,
        color: LinearColor::WHITE,
        range,
        casts_shadow: false,
    }
}

/// The cell the shader's `lightCluster` reads for a world position.
fn cell_at(camera: &CameraFrame, depth: ClusterDepth, point: Vec3) -> Option<usize> {
    let clip = camera.view_projection() * point.extend(1.0);
    let view_depth = -camera.view.transform_point3(point).z;
    if clip.w <= 0.0 || view_depth <= 0.0 || view_depth > depth.far {
        return None;
    }
    let screen = [((clip.x / clip.w) * 0.5 + 0.5).clamp(0.0, 1.0), (0.5 - (clip.y / clip.w) * 0.5).clamp(0.0, 1.0)];
    let column = ((screen[0] * CLUSTER_COLUMNS as f32) as u32).min(CLUSTER_COLUMNS - 1);
    let row = ((screen[1] * CLUSTER_ROWS as f32) as u32).min(CLUSTER_ROWS - 1);
    let slice = (((view_depth.log2() * depth.scale) + depth.bias).max(0.0) as u32).min(CLUSTER_SLICES - 1);
    Some((column + (row * CLUSTER_COLUMNS) + (slice * CLUSTER_COLUMNS * CLUSTER_ROWS)) as usize)
}

fn listed(clusters: &LightClusters, cell: usize) -> &[u16] {
    let [first, count] = clusters.ranges[cell];
    &clusters.indices[first as usize..(first + count) as usize]
}

#[test]
fn every_point_a_light_reaches_lists_it_and_distant_cells_do_not() {
    let camera = camera();
    let lights = [light(Vec3::new(-4.0, 1.0, 2.0), 3.0), light(Vec3::new(5.0, 0.5, -6.0), 6.0)];
    let clusters = LightClusters::build(&lights, &[None, Some(0)], &camera);
    assert_eq!(clusters.positions.len(), 2);
    assert_eq!(clusters.colors[1][3], 1.0, "the second light's shadow slot is stored plus one");
    assert_eq!(clusters.dropped, 0);

    // Points on a lattice through each light's sphere land in cells listing that light.
    for (index, light) in lights.iter().enumerate() {
        let steps = 6;
        for x in -steps..=steps {
            for y in -steps..=steps {
                for z in -steps..=steps {
                    let offset = Vec3::new(x as f32, y as f32, z as f32) * (light.range / steps as f32);
                    if offset.length() >= light.range {
                        continue;
                    }
                    let point = light.position + offset;
                    let Some(cell) = cell_at(&camera, clusters.depth, point) else {
                        continue;
                    };
                    assert!(listed(&clusters, cell).contains(&(index as u16)), "light {index} missing at {point}");
                }
            }
        }
    }

    // A point well outside both ranges, near the camera on the far side of the view, lists neither.
    let away = Vec3::new(0.0, 5.0, 14.0);
    let cell = cell_at(&camera, clusters.depth, away).expect("in view");
    assert!(listed(&clusters, cell).is_empty());
}

#[test]
fn full_lists_drop_the_weakest_lights_and_stay_bounded() {
    let camera = camera();
    // Every light covers the whole view, so each one adds a reference to every cell it reaches.
    let lights: Vec<PointLight> =
        (0..1024).map(|index| light(Vec3::new(0.0, 0.0, index as f32 * 0.001), 400.0)).collect();
    let clusters = LightClusters::build(&lights, &[], &camera);
    assert!(clusters.reference_count() <= MAX_CLUSTER_INDICES);
    assert!(clusters.dropped > 0);

    // Lists keep light order, and the strongest light is listed everywhere it reaches.
    let center = cell_at(&camera, clusters.depth, Vec3::ZERO).expect("in view");
    let list = listed(&clusters, center);
    assert_eq!(list[0], 0);
    assert!(list.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!list.contains(&1023), "the weakest light lost its cells");
}
