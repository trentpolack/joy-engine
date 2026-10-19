// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/hud_text.rs`.

use super::*;

/// Run one egui pass and lay out text with its painter.
fn laid_out(runs: &[(&str, Style)]) -> (Text, Text) {
    let context = egui::Context::default();
    let mut result = None;
    let mut output = context.run_ui(egui::RawInput::default(), |root| {
        let painter = root.painter().clone();
        let plain = layout(&painter, &[("AB", runs[0].1)], f32::INFINITY, Align::Min);
        result = Some((layout(&painter, runs, f32::INFINITY, Align::Min), plain));
    });
    output.textures_delta.clear();
    result.unwrap()
}

#[test]
fn symbols_reserve_their_advance_and_paint_as_vectors() {
    let style = Style::new(20.0, Color32::WHITE);
    let (text, plain) = laid_out(&[("A→B", style)]);
    assert_eq!(text.symbols.len(), 1);
    let (rect, symbol, _, size) = text.symbols[0];
    assert_eq!(symbol, Symbol::ArrowRight);
    assert_eq!(size, 20.0);
    assert!((rect.width() - 20.0).abs() < 0.01, "an arrow advances one em: {rect:?}");
    assert!(text.size().x >= plain.size().x + 19.0, "the arrow widens the run");
    assert_eq!(Symbol::for_char('a'), None);
}

#[test]
fn spacing_and_weight_follow_the_style() {
    let style = Style::new(16.0, Color32::WHITE);
    let (regular, _) = laid_out(&[("SPACED", style)]);
    let (spaced, _) = laid_out(&[("SPACED", style.spacing(2.0).heavy())]);
    assert!(spaced.size().x > regular.size().x + 9.0, "letter spacing applies between letters");
    assert_eq!(spaced.weight, Weight::Heavy);
    assert_eq!(regular.line_rects().len(), 1);
}
