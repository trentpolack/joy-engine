// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use egui::{
    Rect,
    pos2,
};

use super::*;

#[test]
fn meters_and_sliders_stay_in_range_for_empty_pools_and_repeated_steps() {
    // An empty pool (0 / 0) or an overfull one must not paint outside the track.
    let track = Rect::from_min_max(pos2(10.0, 0.0), pos2(110.0, 4.0));
    assert_eq!(fill_rect(track, f64::NAN).width(), 0.0);
    assert_eq!(fill_rect(track, 1.5).width(), 100.0);
    assert_eq!(fill_rect(track, 0.25).max.x, 35.0);

    // Slider steps snap from the range start without floating-point drift, and degenerate ranges stay at the start.
    assert_eq!(slider_value(0.3, (0.0, 1.0), 0.1), 0.3);
    assert_eq!(slider_value(0.52, (1.0, 60.0), 1.0), 32.0);
    assert_eq!(slider_value(f64::NAN, (2.0, 4.0), 0.5), 2.0);
    assert_eq!(slider_fraction(5.0, (5.0, 5.0)), 0.0);
    assert_eq!(slider_fraction(3.0, (2.0, 4.0)), 0.5);
}
