// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Revolve, tube, and smooth-normal generators. Ported from `src/form/procedural-shapes.ts`.

use super::store::{
    self,
    Store,
    count,
    number,
    numbers,
};
use crate::{
    math::hypot,
    value::integer_range,
};
use joy_core::math::deterministic;
use serde_json::{
    Value as Json,
    json,
};

pub fn revolve(store: &mut Store, profile: &Json, segments: f64) -> Result<Json, String> {
    let segments = integer_range(segments, 3.0, 96.0)?;
    let points = ordered_points(profile)?;
    if points.iter().any(|p| p[0] < 0.0 || p[2] != 0.0) {
        return Err("Revolve profiles use (nonnegative radius, height, 0).".into());
    }
    let mut result = store::create();
    let mut rings = Vec::new();
    for point in points {
        let radius = point[0];
        let height = point[1];
        if radius == 0.0 {
            let pole = store.add_point(&mut result, &[0.0, height, 0.0])?;
            rings.push(vec![pole; segments]);
        } else {
            let mut ring = Vec::new();
            for side in 0..segments {
                let angle = side as f64 / segments as f64 * std::f64::consts::PI * 2.0;
                ring.push(store.add_point(
                    &mut result,
                    &[deterministic::cos(angle) * radius, height, deterministic::sin(angle) * radius],
                )?);
            }
            rings.push(ring);
        }
    }
    connect_rings(store, &mut result, &rings)?;
    smooth_normals(store, &mut result)?;
    Ok(result)
}

pub fn tube(store: &mut Store, path: &Json, radius: f64, sides: f64) -> Result<Json, String> {
    let sides = integer_range(sides, 3.0, 64.0)?;
    if !radius.is_finite() || radius <= 0.0 || radius > 1e6 {
        return Err("Tube radius must be positive and at most 1,000,000.".into());
    }
    let points = ordered_points(path)?;
    let mut result = store::create();
    let mut rings = Vec::new();
    let mut radial: Option<Vec<f64>> = None;
    for row in 0..points.len() {
        let before = normalize(&subtract(&points[row], &points[row.saturating_sub(1)]));
        let after = normalize(&subtract(&points[(row + 1).min(points.len() - 1)], &points[row]));
        let tangent = normalize(&before.iter().zip(after).map(|(a, b)| a + b).collect::<Vec<_>>());
        if hypot(&tangent) < 0.5 {
            return Err("Tube paths cannot reverse direction at a point.".into());
        }
        let fallback = if tangent[0].abs() < 0.8 { vec![1.0, 0.0, 0.0] } else { vec![0.0, 0.0, 1.0] };
        let previous = radial.as_ref().unwrap_or(&fallback);
        let alignment = dot(previous, &tangent);
        let mut frame = normalize(&previous.iter().zip(&tangent).map(|(a, b)| a - b * alignment).collect::<Vec<_>>());
        if hypot(&frame) < 0.5 {
            frame = normalize(&cross(&tangent, &fallback));
        }
        let binormal = cross(&frame, &tangent);
        let mut ring = Vec::new();
        for side in 0..sides {
            let angle = side as f64 / sides as f64 * std::f64::consts::PI * 2.0;
            let position = (0..3)
                .map(|axis| {
                    points[row][axis]
                        + radius
                            * (frame[axis] * deterministic::cos(angle) + binormal[axis] * deterministic::sin(angle))
                })
                .collect::<Vec<_>>();
            ring.push(store.add_point(&mut result, &position)?);
        }
        radial = Some(frame);
        rings.push(ring);
    }
    connect_rings(store, &mut result, &rings)?;
    smooth_normals(store, &mut result)?;
    Ok(result)
}

pub fn smooth_normals(store: &mut Store, geometry: &mut Json) -> Result<(), String> {
    let points = geometry["attributes"]["points"]["position"]["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(numbers)
        .collect::<Vec<_>>();
    let mut normals = vec![vec![0.0; 3]; count(geometry, "points")];
    for face in geometry["faces"].as_array().unwrap() {
        let face = face.as_array().unwrap().iter().map(|v| number(v) as usize).collect::<Vec<_>>();
        let normal =
            cross(&subtract(&points[face[1]], &points[face[0]]), &subtract(&points[face[2]], &points[face[0]]));
        for index in face {
            for (axis, component) in normal.iter().enumerate() {
                normals[index][axis] += component;
            }
        }
    }
    for domain in ["faces", "corners"] {
        geometry["attributes"][domain].as_object_mut().unwrap().shift_remove("normal");
    }
    for (index, normal) in normals.iter().enumerate() {
        store.set(geometry, "points", "normal", index, &json!(normalize(normal)), false)?;
    }
    Ok(())
}

fn ordered_points(geometry: &Json) -> Result<Vec<Vec<f64>>, String> {
    let points = geometry["attributes"]["points"]["position"]["values"]
        .as_array()
        .unwrap()
        .iter()
        .map(numbers)
        .collect::<Vec<_>>();
    if points.len() < 2 || points.len() > 512 {
        return Err("Profiles and paths require 2–512 ordered points.".into());
    }
    if points.windows(2).any(|pair| hypot(&subtract(&pair[1], &pair[0])) < 1e-8) {
        return Err("Adjacent profile/path points must be distinct.".into());
    }
    Ok(points)
}
fn connect_rings(store: &mut Store, geometry: &mut Json, rings: &[Vec<usize>]) -> Result<(), String> {
    let sides = rings[0].len();
    for row in 0..rings.len() - 1 {
        let v = row as f64 / (rings.len() - 1) as f64;
        let w = (row + 1) as f64 / (rings.len() - 1) as f64;
        for side in 0..sides {
            let next = (side + 1) % sides;
            let u = side as f64 / sides as f64;
            let t = (side + 1) as f64 / sides as f64;
            let a = rings[row][side];
            let b = rings[row][next];
            let c = rings[row + 1][side];
            let d = rings[row + 1][next];
            append_face(store, geometry, &[a, c, b], &[[u, v, 0.0], [u, w, 0.0], [t, v, 0.0]])?;
            append_face(store, geometry, &[b, c, d], &[[t, v, 0.0], [u, w, 0.0], [t, w, 0.0]])?;
        }
    }
    Ok(())
}
fn append_face(
    store: &mut Store,
    geometry: &mut Json,
    indices: &[usize; 3],
    uvs: &[[f64; 3]; 3],
) -> Result<(), String> {
    if indices[0] == indices[1] || indices[1] == indices[2] || indices[0] == indices[2] {
        return Ok(());
    }
    let face = store.add_face(geometry, indices)?;
    for (corner, uv) in uvs.iter().enumerate() {
        store.set(geometry, "corners", "uv", face * 3 + corner, &json!(uv), false)?;
    }
    Ok(())
}
pub fn subtract(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(a, b)| a - b).collect()
}
pub fn cross(a: &[f64], b: &[f64]) -> Vec<f64> {
    vec![a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn normalize(value: &[f64]) -> Vec<f64> {
    let length = hypot(value);
    if length > 1e-12 { value.iter().map(|v| v / length).collect() } else { vec![0.0; 3] }
}
