// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/environment/irradiance.rs`.

use super::*;
use joy_core::math::deterministic::{
    cos,
    sin,
};

const WIDTH: u32 = 128;
const HEIGHT: u32 = 64;

/// The shader's sum: cosine-weighted average radiance in a unit direction.
fn shade(coefficients: &IrradianceCoefficients, direction: [f64; 3]) -> [f64; 3] {
    let polynomials = basis_polynomials(direction);
    let mut sum = [0.0; 3];
    for (coefficient, polynomial) in coefficients.iter().zip(polynomials) {
        for (channel, value) in sum.iter_mut().zip(coefficient) {
            *channel += f64::from(*value) * polynomial;
        }
    }
    sum
}

/// The row of an equirectangular pixel index: zero is straight up.
fn row(index: usize) -> usize {
    index / WIDTH as usize
}

#[test]
fn uniform_and_hemispherical_light_integrate_exactly() {
    // Uniform radiance arrives unchanged from every direction.
    let uniform = irradiance_coefficients(WIDTH, HEIGHT, |_| [0.5, 1.0, 2.0]);
    for direction in [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, -0.6, 0.8]] {
        let shaded = shade(&uniform, direction);
        for (channel, expected) in shaded.iter().zip([0.5, 1.0, 2.0]) {
            assert!((channel - expected).abs() < 2.0e-3, "{direction:?}: {shaded:?}");
        }
    }

    // A white sky over a black ground: a surface facing up sees all of it, facing down none, and facing sideways half.
    let sky =
        irradiance_coefficients(
            WIDTH,
            HEIGHT,
            |index| if row(index) < HEIGHT as usize / 2 { [1.0; 3] } else { [0.0; 3] },
        );
    assert!((shade(&sky, [0.0, 1.0, 0.0])[0] - 1.0).abs() < 0.01);
    assert!(shade(&sky, [0.0, -1.0, 0.0])[0].abs() < 0.01);
    assert!((shade(&sky, [1.0, 0.0, 0.0])[0] - 0.5).abs() < 0.01);
}

#[test]
fn a_small_bright_sun_never_rings_below_zero() {
    // One very bright pixel near the horizon over a dim sky.
    let sun = (HEIGHT as usize / 3 * WIDTH as usize) + 7;
    let coefficients =
        irradiance_coefficients(WIDTH, HEIGHT, |index| if index == sun { [5.0e4; 3] } else { [0.01; 3] });
    let directions = test_directions();
    let shaded: Vec<f64> = directions.iter().map(|direction| shade(&coefficients, *direction)[0]).collect();
    assert!(shaded.iter().all(|value| *value >= -1.0e-6), "irradiance stays non-negative");

    // The brightest test direction still faces the sun.
    let brightest = directions[shaded.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0];
    let sun_polar = ((HEIGHT as f64 / 3.0) + 0.5) * PI / HEIGHT as f64;
    let sun_azimuth = ((7.0 + 0.5) * 2.0 * PI / WIDTH as f64) - PI;
    let toward_sun = [sin(sun_polar) * cos(sun_azimuth), cos(sun_polar), sin(sun_polar) * sin(sun_azimuth)];
    let alignment: f64 = brightest.iter().zip(toward_sun).map(|(a, b)| a * b).sum();
    assert!(alignment > 0.9, "brightest toward the sun ({alignment})");
}
