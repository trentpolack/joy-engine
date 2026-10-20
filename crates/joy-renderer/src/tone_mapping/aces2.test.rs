// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/tone_mapping/aces2.rs`.

use super::*;

/// Linear Rec.709 scene colors and the display-linear output OpenColorIO 2.6 computes for them with the built-in
/// "ACES 2.0 SDR 100 nits Rec.709" output transform (Rec.709 to AP1 with Bradford adaptation, AP1 to AP0, the output
/// transform, then CIE XYZ D65 back to linear Rec.709).
const SDR_REC709: [([f64; 3], [f64; 3]); 18] = [
    ([0.000000, 0.000000, 0.000000], [0.0000000, 0.0000000, 0.0000000]),
    ([0.180000, 0.180000, 0.180000], [0.0999991, 0.0999994, 0.0999993]),
    ([1.000000, 1.000000, 1.000000], [0.4575653, 0.4575660, 0.4575658]),
    ([4.000000, 4.000000, 4.000000], [0.7871516, 0.7871515, 0.7871516]),
    ([16.000000, 16.000000, 16.000000], [0.9434750, 0.9434750, 0.9434751]),
    ([100.000000, 100.000000, 100.000000], [0.9976227, 0.9976225, 0.9976225]),
    ([1.000000, 0.000000, 0.000000], [0.5540078, 0.0088254, 0.0033028]),
    ([0.000000, 1.000000, 0.000000], [0.0472083, 0.5143793, 0.0339152]),
    ([0.000000, 0.000000, 1.000000], [0.0000000, 0.0000000, 0.5134001]),
    ([0.500000, 0.250000, 0.050000], [0.2948587, 0.1471075, 0.0287527]),
    ([10.000000, 0.500000, 0.100000], [1.0000000, 0.4766966, 0.3737049]),
    ([0.050000, 0.200000, 0.800000], [0.0255990, 0.1119426, 0.4629686]),
    ([2.000000, 2.000000, 0.100000], [0.6713251, 0.6850863, 0.1914686]),
    ([0.020000, 0.010000, 0.005000], [0.0019561, 0.0007827, 0.0002616]),
    ([0.800000, 0.100000, 0.900000], [0.4528975, 0.0624370, 0.4997995]),
    ([3.000000, 0.200000, 0.200000], [1.0000000, 0.2066070, 0.1805343]),
    ([0.100000, 0.600000, 0.550000], [0.0907238, 0.3260096, 0.3027714]),
    ([0.900000, 0.850000, 0.700000], [0.4250449, 0.4133965, 0.3765604]),
];

/// The same inputs through OpenColorIO's "ACES 2.0 HDR 1000 nits P3-D65" output transform, in linear P3-D65.
const HDR_P3: [([f64; 3], [f64; 3]); 7] = [
    ([0.180000, 0.180000, 0.180000], [0.1451152, 0.1451158, 0.1451155]),
    ([1.000000, 1.000000, 1.000000], [1.0656356, 1.0656397, 1.0656382]),
    ([16.000000, 16.000000, 16.000000], [7.0521636, 7.0521717, 7.0521693]),
    ([1.000000, 0.000000, 0.000000], [0.7444588, 0.0210625, 0.0110680]),
    ([10.000000, 0.500000, 0.100000], [6.7264667, 1.3522489, 0.7016307]),
    ([0.050000, 0.200000, 0.800000], [0.0549448, 0.1631678, 0.6875358]),
    ([0.800000, 0.100000, 0.900000], [0.6592781, 0.1120542, 0.8143104]),
];

/// Largest per-channel difference from the reference, in display-linear units (one is SDR white). Table searches and
/// matrix construction run at slightly different precision than the reference, whose values keep OpenColorIO's printed
/// digits (hence `f64`).
const TOLERANCE: f32 = 2e-3;

fn assert_matches(transform: &Aces2, references: &[([f64; 3], [f64; 3])]) {
    for (input, expected) in references {
        let (input, expected) = (input.map(|channel| channel as f32), expected.map(|channel| channel as f32));
        let rendered = transform.render(input);
        let error = rendered.iter().zip(expected).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        let scale = expected.iter().copied().fold(1.0, f32::max);
        assert!(error <= TOLERANCE * scale, "{input:?}: rendered {rendered:?}, reference {expected:?}");
    }
}

#[test]
fn sdr_output_matches_the_opencolorio_reference() {
    let transform = Aces2::rec709(100.0);
    assert_matches(&transform, &SDR_REC709);

    // Middle gray lands at 10 nits, and output never exceeds the peak.
    assert!((transform.render([0.18; 3])[1] - 0.1).abs() < 1e-3);
    assert!(transform.render([1.0e6, 0.0, 0.0]).iter().all(|channel| *channel <= 1.0));
}

#[test]
fn hdr_tables_follow_the_display_peak() {
    let transform = Aces2::new(1000.0, &P3_D65);
    assert_matches(&transform, &HDR_P3);
    assert_eq!(transform.peak_luminance(), 1000.0);
}
