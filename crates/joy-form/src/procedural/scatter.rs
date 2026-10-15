// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Area- and weight-proportional point scattering. Ported from `src/form/procedural-scatter.ts`.

use super::{
    shapes::{cross, subtract},
    store::{self, Store, attribute_at, number, numbers},
};
use crate::{
    math::{hypot, to_u32},
    value::integer_range,
};
use serde_json::{Value as Json, json};

/// Area times linearly interpolated point density; its PRNG is local to this call.
pub fn scatter(store: &mut Store, source: &Json, amount: f64, seed: f64, weight: &str) -> Result<Json, String> {
    let amount = integer_range(amount, 0.0, 150_000.0)?;
    let attributes = &source["attributes"]["points"];
    let positions = attributes["position"]["values"].as_array().unwrap();
    let weights = if weight.is_empty() { None } else { attributes.get(weight) };
    if !weight.is_empty()
        && weights.is_none_or(|weights| {
            weights["size"] != 1
                || weights["values"].as_array().unwrap().iter().any(|v| v.as_f64().is_none_or(|v| v < 0.0))
        })
    {
        return Err("Scatter weight must name a nonnegative scalar point attribute.".into());
    }
    let mut result = store::create();
    if !seed.is_finite() || seed.fract() != 0.0 {
        return Err("Random seed must be a finite integer.".into());
    }
    let mut state = to_u32(seed);
    let mut random = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        state as f64 / 4294967296.0
    };
    let faces = source["faces"].as_array().unwrap();
    let mut total = 0.0;
    let mut cumulative = Vec::new();
    for face in faces {
        let indices = face.as_array().unwrap().iter().map(|v| number(v) as usize).collect::<Vec<_>>();
        let [a, b, c] =
            [numbers(&positions[indices[0]]), numbers(&positions[indices[1]]), numbers(&positions[indices[2]])];
        let normal = cross(&subtract(&b, &a), &subtract(&c, &a));
        let density =
            weights.map_or(1.0, |w| indices.iter().map(|index| number(&w["values"][index])).sum::<f64>() / 3.0);
        total += hypot(&normal) * 0.5 * density;
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
        let indices = faces[low].as_array().unwrap().iter().map(|v| number(v) as usize).collect::<Vec<_>>();
        let densities =
            indices.iter().map(|index| weights.map_or(1.0, |w| number(&w["values"][index]))).collect::<Vec<_>>();
        let choice = random() * densities.iter().sum::<f64>();
        let selected = if choice < densities[0] {
            0
        } else if choice < densities[0] + densities[1] {
            1
        } else {
            2
        };
        let mut barycentric = Vec::new();
        for corner in 0..3 {
            let mut sample = -random().max(f64::from_bits(1)).ln();
            if corner == selected {
                sample += -random().max(f64::from_bits(1)).ln();
            }
            barycentric.push(sample);
        }
        let sum = barycentric.iter().sum::<f64>();
        let blend = barycentric.iter().map(|v| v / sum).collect::<Vec<_>>();
        let interpolate = |name: &str| -> Result<Json, String> {
            let values = indices
                .iter()
                .map(|index| attribute_at(source, "points", name, *index as f64))
                .collect::<Result<Vec<_>, _>>()?;
            if values[0].is_number() {
                Ok(json!(
                    values.iter().enumerate().fold(0.0, |sum, (corner, value)| sum + number(value) * blend[corner])
                ))
            } else {
                Ok(json!(
                    (0..values[0].as_array().unwrap().len())
                        .map(|axis| values
                            .iter()
                            .enumerate()
                            .fold(0.0, |sum, (corner, value)| sum + number(&value[axis]) * blend[corner]))
                        .collect::<Vec<_>>()
                ))
            }
        };
        let index = store.add_point(&mut result, &numbers(&interpolate("position")?))?;
        for name in attributes.as_object().unwrap().keys() {
            if name != "position" && name != "normal" {
                let mut value = interpolate(name)?;
                if name == "color" {
                    value = json!(numbers(&value).iter().map(|v| v.clamp(0.0, 1.0)).collect::<Vec<_>>());
                }
                store.set(&mut result, "points", name, index, &value, false)?;
            }
        }
        let [a, b, c] =
            [numbers(&positions[indices[0]]), numbers(&positions[indices[1]]), numbers(&positions[indices[2]])];
        let normal = cross(&subtract(&b, &a), &subtract(&c, &a));
        let length = hypot(&normal);
        store.set(
            &mut result,
            "points",
            "normal",
            index,
            &json!(normal.iter().map(|v| v / length).collect::<Vec<_>>()),
            false,
        )?;
    }
    Ok(result)
}
