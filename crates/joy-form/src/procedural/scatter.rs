// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Area- and weight-proportional point scattering.

use super::{
    geometry::{
        Column,
        DomainKind,
        Element,
        Geometry,
        POSITION_ATTRIBUTE,
    },
    shapes::{
        cross,
        subtract,
    },
    store::Store,
};
use crate::{
    math::{
        hypot,
        to_u32,
    },
    value::integer_range,
};
use joy_core::math::deterministic;

/// Area times linearly interpolated point density; its PRNG is local to this call.
pub fn scatter(store: &mut Store, source: &Geometry, amount: f64, seed: f64, weight: &str) -> Result<Geometry, String> {
    let amount = integer_range(amount, 0.0, 150_000.0)?;
    let weights = if weight.is_empty() {
        None
    } else {
        match source.points.get(weight) {
            Some(Column::Number {
                size: 1,
                values,
            }) if values.iter().all(|value| *value >= 0.0) => Some(values.as_slice()),
            _ => return Err("Scatter weight must name a nonnegative scalar point attribute.".into()),
        }
    };
    let mut result = Geometry::default();
    if !seed.is_finite() || seed.fract() != 0.0 {
        return Err("Random seed must be a finite integer.".into());
    }
    let mut state = to_u32(seed);
    let mut random = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        state as f64 / 4294967296.0
    };
    let density = |point: usize| weights.map_or(1.0, |weights| weights[point]);
    let mut total = 0.0;
    let mut cumulative = Vec::with_capacity(source.faces.len());
    for face in &source.faces {
        let [a, b, c] = face.map(|point| source.position(point));
        let normal = cross(&subtract(&b, &a), &subtract(&c, &a));
        let face_density = face.iter().map(|point| density(*point)).sum::<f64>() / 3.0;
        total += hypot(&normal) * 0.5 * face_density;
        if !total.is_finite() {
            return Err("Scatter area/weight overflow.".into());
        }
        cumulative.push(total);
    }
    if total == 0.0 {
        return Ok(result);
    }
    for _ in 0..amount {
        let target = random() * total;
        let mut low = 0;
        let mut high = cumulative.len() - 1;
        while low < high {
            let middle = (low + high) / 2;
            if cumulative[middle] <= target {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let face = source.faces[low];
        let densities = face.map(density);
        let choice = random() * densities.iter().sum::<f64>();
        let selected = if choice < densities[0] {
            0
        } else if choice < densities[0] + densities[1] {
            1
        } else {
            2
        };
        let mut barycentric = [0.0; 3];
        for (corner, sample) in barycentric.iter_mut().enumerate() {
            *sample = -deterministic::ln(random().max(f64::from_bits(1)));
            if corner == selected {
                *sample += -deterministic::ln(random().max(f64::from_bits(1)));
            }
        }
        let sum = barycentric.iter().sum::<f64>();
        let blend = barycentric.map(|sample| sample / sum);
        let interpolate = |column: &Column| -> Element {
            match column {
                // Text and true/false cannot blend; samples take the value of the corner they were drawn toward.
                Column::Number {
                    size,
                    ..
                } => Element::Numbers(
                    (0..*size)
                        .map(|axis| {
                            face.iter()
                                .enumerate()
                                .fold(0.0, |sum, (corner, point)| sum + column.numbers(*point)[axis] * blend[corner])
                        })
                        .collect(),
                ),
                _ => column.get(face[selected]),
            }
        };
        let position = interpolate(source.points.get(POSITION_ATTRIBUTE).expect("geometry always has positions"));
        let Element::Numbers(position) = position else { unreachable!("positions are numeric") };
        let index = store.add_point(&mut result, &position)?;
        for (name, column) in source.points.iter() {
            if name == POSITION_ATTRIBUTE || name == "normal" {
                continue;
            }
            let mut value = interpolate(column);
            if name == "color"
                && let Element::Numbers(channels) = &mut value
            {
                // Interpolation cannot leave the corners' range, but rounding can step just outside it.
                for (channel, component) in channels.iter_mut().enumerate() {
                    *component = if channel == 3 { component.clamp(0.0, 1.0) } else { component.max(0.0) };
                }
            }
            store.set(&mut result, DomainKind::Points, name, index, &value, false)?;
        }
        let [a, b, c] = face.map(|point| source.position(point));
        let normal = cross(&subtract(&b, &a), &subtract(&c, &a));
        let length = hypot(&normal);
        store.set(
            &mut result,
            DomainKind::Points,
            "normal",
            index,
            &Element::Numbers(normal.iter().map(|component| component / length).collect()),
            false,
        )?;
    }
    Ok(result)
}
