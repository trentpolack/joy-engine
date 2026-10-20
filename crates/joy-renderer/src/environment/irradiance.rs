// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Diffuse image lighting as L2 spherical harmonics.
//!
//! The environment's radiance is projected onto the nine real spherical-harmonic basis functions of bands zero to two
//! on the CPU, straight from the source pixels (so a sun a few pixels wide counts in full), then convolved with the
//! clamped cosine (Ramamoorthi and Hanrahan 2001, "An Efficient Representation for Irradiance Environment Maps"). Nine
//! coefficients describe irradiance in every direction to within a few percent, without the noise of a Monte Carlo
//! cube. A bright, small source rings: its band-two lobe can push irradiance below zero opposite it. The coefficients
//! are smoothed with the sphere's heat kernel just enough that no direction goes negative, which keeps the sun's side
//! exact for soft environments and blurs only the ringing for harsh ones.

use std::f64::consts::PI;

use joy_core::math::deterministic;

/// Irradiance coefficients as `EnvironmentIrradiance` in environment-lighting.wgsl expects them: RGB in XYZ (W unused)
/// for the polynomials 1, y, z, x, xy, yz, 3z² - 1, xz, and x² - y² of the unit direction (Y up), with the basis
/// constants folded in and divided by pi, so the sum is the cosine-weighted average radiance a Lambertian surface
/// multiplies by its albedo.
pub(crate) type IrradianceCoefficients = [[f32; 4]; 9];

/// Normalization constant of each basis function, in the polynomial order above.
const BASIS: [f64; 9] =
    [0.282_095, 0.488_603, 0.488_603, 0.488_603, 1.092_548, 1.092_548, 0.315_392, 1.092_548, 0.546_274];

/// The band (degree) of each basis function.
const BAND: [usize; 9] = [0, 1, 1, 1, 2, 2, 2, 2, 2];

/// The clamped cosine's convolution weight per band: pi, 2pi/3, and pi/4.
const COSINE_LOBE: [f64; 3] = [PI, 2.0 * PI / 3.0, PI / 4.0];

/// Directions, spread evenly over the sphere, where smoothed irradiance must not be negative.
const TEST_DIRECTIONS: usize = 256;

/// Heat-kernel strength at which only the constant band survives; the smoothing search stays below it.
const MAX_SMOOTHING: f64 = 4.0;

/// Halvings in the smoothing search.
const SMOOTHING_STEPS: u32 = 24;

/// Irradiance of an equirectangular environment, `width` by `height` pixels with the top row first (U wraps around +Y
/// from +X, V runs from straight up to straight down, as the specular capture samples it). `radiance` returns the
/// linear RGB radiance of the pixel at a row-major index.
pub(crate) fn irradiance_coefficients(
    width: u32,
    height: u32,
    radiance: impl Fn(usize) -> [f64; 3],
) -> IrradianceCoefficients {
    let projected = project_radiance(width as usize, height as usize, radiance);

    // Convolve with the clamped cosine, then smooth only as much as non-negative irradiance needs.
    let convolved: [[f64; 3]; 9] =
        std::array::from_fn(|index| projected[index].map(|channel| channel * COSINE_LOBE[BAND[index]]));
    let smoothing = minimum_smoothing(&convolved);
    let smoothed = smooth(&convolved, smoothing);

    // Fold in the basis constants and the Lambertian 1/pi for the shader's polynomial sum.
    std::array::from_fn(|index| {
        let [red, green, blue] = smoothed[index].map(|channel| (channel * BASIS[index] / PI) as f32);
        [red, green, blue, 0.0]
    })
}

/// The radiance's projection onto each basis function, integrated over every pixel's solid angle.
fn project_radiance(width: usize, height: usize, radiance: impl Fn(usize) -> [f64; 3]) -> [[f64; 3]; 9] {
    let azimuth_step = 2.0 * PI / width as f64;
    let polar_step = PI / height as f64;

    // Each column's azimuth, measured around +Y from +X so that U = atan2(z, x) / 2pi + 1/2.
    let columns: Vec<(f64, f64)> =
        (0..width).map(|column| deterministic::sin_cos(((column as f64 + 0.5) * azimuth_step) - PI)).collect();

    let mut projected = [[0.0_f64; 3]; 9];
    for row in 0..height {
        let (polar_sine, polar_cosine) = deterministic::sin_cos((row as f64 + 0.5) * polar_step);
        let solid_angle = azimuth_step * polar_step * polar_sine;
        for (column, (azimuth_sine, azimuth_cosine)) in columns.iter().enumerate() {
            let direction = [polar_sine * azimuth_cosine, polar_cosine, polar_sine * azimuth_sine];
            let polynomials = basis_polynomials(direction);
            let color = radiance((row * width) + column);
            for (coefficient, polynomial) in projected.iter_mut().zip(polynomials) {
                let weight = polynomial * solid_angle;
                for (sum, channel) in coefficient.iter_mut().zip(color) {
                    *sum += channel * weight;
                }
            }
        }
    }

    // Each projection uses the normalized basis function: its constant times its polynomial.
    std::array::from_fn(|index| projected[index].map(|channel| channel * BASIS[index]))
}

/// The basis polynomials of a unit direction `[x, y, z]`, in coefficient order.
fn basis_polynomials([x, y, z]: [f64; 3]) -> [f64; 9] {
    [1.0, y, z, x, x * y, y * z, (3.0 * z * z) - 1.0, x * z, (x * x) - (y * y)]
}

/// Convolved coefficients damped per band by the heat kernel `exp(-strength * l(l + 1))`.
fn smooth(coefficients: &[[f64; 3]; 9], strength: f64) -> [[f64; 3]; 9] {
    std::array::from_fn(|index| {
        let band = BAND[index] as f64;
        let damping = deterministic::exp(-strength * band * (band + 1.0));
        coefficients[index].map(|channel| channel * damping)
    })
}

/// The least heat-kernel strength at which irradiance is non-negative in every test direction.
fn minimum_smoothing(coefficients: &[[f64; 3]; 9]) -> f64 {
    let directions = test_directions();
    let non_negative = |strength: f64| {
        let smoothed = smooth(coefficients, strength);
        directions.iter().all(|direction| evaluate(&smoothed, *direction).iter().all(|channel| *channel >= -1.0e-9))
    };
    if non_negative(0.0) {
        return 0.0;
    }

    // Only the constant band remains at the upper bound, and non-negative radiance has a non-negative average.
    let (mut low, mut high) = (0.0, MAX_SMOOTHING);
    for _ in 0..SMOOTHING_STEPS {
        let middle = 0.5 * (low + high);
        if non_negative(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// Irradiance from convolved coefficients in a unit direction.
fn evaluate(coefficients: &[[f64; 3]; 9], direction: [f64; 3]) -> [f64; 3] {
    let polynomials = basis_polynomials(direction);
    let mut irradiance = [0.0; 3];
    for (index, polynomial) in polynomials.iter().enumerate() {
        for (sum, channel) in irradiance.iter_mut().zip(coefficients[index]) {
            *sum += channel * BASIS[index] * polynomial;
        }
    }
    irradiance
}

/// Evenly spread unit directions: a Fibonacci spiral over the sphere.
fn test_directions() -> Vec<[f64; 3]> {
    let golden_angle = PI * (3.0 - 5.0_f64.sqrt());
    (0..TEST_DIRECTIONS)
        .map(|index| {
            let y = 1.0 - ((2.0 * (index as f64 + 0.5)) / TEST_DIRECTIONS as f64);
            let radius = (1.0 - (y * y)).max(0.0).sqrt();
            let (sine, cosine) = deterministic::sin_cos(golden_angle * index as f64);
            [radius * cosine, y, radius * sine]
        })
        .collect()
}

#[cfg(test)]
#[path = "irradiance.test.rs"]
mod tests;
