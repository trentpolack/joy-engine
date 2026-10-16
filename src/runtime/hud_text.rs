// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! HUD text with web-page typography for egui HUDs: CSS-style pixel sizes, letter spacing, line height, emulated font
//! weight, and the symbols TypeScript game pages use that egui's bundled fonts lack (arrows and element sigils), which
//! are drawn as vector glyphs in their place.
//!
//! Games lay out styled runs with [`layout`], [`single`], or [`wrapped`] and paint the returned [`Text`] with an egui
//! [`Painter`], so ported HUDs keep the look of their TypeScript pages.

use std::sync::Arc;

use egui::text::{Galley, LayoutJob, TextFormat};
use egui::{Align, Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

/// Character that reserves a symbol's space in a layout; drawn transparent.
const PLACEHOLDER: char = '¤';

/// How heavy a run of text looks. egui bundles one light face, so heavier weights repaint the glyphs offset by whole
/// pixels (epaint snaps glyphs to the pixel grid, so smaller offsets would embolden some runs and not others).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Weight {
    /// The bundled face.
    #[default]
    Regular,
    /// Semi-bold and bold (CSS 600 to 750).
    Bold,
    /// Extra bold (CSS 800 and above).
    Heavy,
}

/// Typography for one run of text.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// Font size in logical pixels (CSS `font-size`).
    pub size: f32,
    /// Text color.
    pub color: Color32,
    /// Extra spacing between letters in logical pixels (CSS `letter-spacing`).
    pub spacing: f32,
    /// Emulated weight.
    pub weight: Weight,
    /// Line height in logical pixels; `None` uses the font's.
    pub line_height: Option<f32>,
    /// Use the monospace face.
    pub monospace: bool,
}

impl Style {
    /// Regular text of `size` pixels in `color`.
    pub const fn new(size: f32, color: Color32) -> Self {
        Self { size, color, spacing: 0.0, weight: Weight::Regular, line_height: None, monospace: false }
    }

    /// With letter spacing in pixels.
    pub const fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// Bold.
    pub const fn bold(mut self) -> Self {
        self.weight = Weight::Bold;
        self
    }

    /// Extra bold.
    pub const fn heavy(mut self) -> Self {
        self.weight = Weight::Heavy;
        self
    }

    /// With a line height as a multiple of the size (CSS unitless `line-height`).
    pub fn line_height(mut self, multiple: f32) -> Self {
        self.line_height = Some(self.size * multiple);
        self
    }

    /// Monospace face.
    pub const fn monospace(mut self) -> Self {
        self.monospace = true;
        self
    }

    /// With another color.
    pub const fn color(mut self, color: Color32) -> Self {
        self.color = color;
        self
    }

    /// With another size.
    pub const fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    fn font(&self) -> FontId {
        FontId::new(self.size, if self.monospace { FontFamily::Monospace } else { FontFamily::Proportional })
    }
}

/// A symbol drawn with vector shapes instead of a font glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Symbol {
    /// → (U+2192)
    ArrowRight,
    /// ← (U+2190)
    ArrowLeft,
    /// ↑ (U+2191)
    ArrowUp,
    /// ↓ (U+2193)
    ArrowDown,
    /// ↗ (U+2197)
    ArrowUpRight,
    /// ✕ (U+2715)
    Cross,
    /// ◆ (U+25C6)
    Diamond,
    /// ✦ (U+2726)
    Star,
    /// ✧ (U+2727)
    StarOutline,
    /// ◈ (U+25C8)
    DiamondInset,
    /// ❋ (U+274B)
    Flower,
    /// ϟ (U+03DF)
    Lightning,
    /// ☾ (U+263E)
    Crescent,
    /// ♨ (U+2668)
    HotSprings,
    /// ❄ (U+2744)
    Snowflake,
    /// ╱ (U+2571)
    Slash,
}

impl Symbol {
    /// The symbol a character stands for, when the bundled fonts cannot draw it.
    pub fn for_char(character: char) -> Option<Self> {
        Some(match character {
            '→' => Symbol::ArrowRight,
            '←' => Symbol::ArrowLeft,
            '↑' => Symbol::ArrowUp,
            '↓' => Symbol::ArrowDown,
            '↗' => Symbol::ArrowUpRight,
            '✕' => Symbol::Cross,
            '◆' => Symbol::Diamond,
            '✦' => Symbol::Star,
            '✧' => Symbol::StarOutline,
            '◈' => Symbol::DiamondInset,
            '❋' => Symbol::Flower,
            'ϟ' => Symbol::Lightning,
            '☾' => Symbol::Crescent,
            '♨' => Symbol::HotSprings,
            '❄' => Symbol::Snowflake,
            '╱' => Symbol::Slash,
            _ => return None,
        })
    }

    /// Advance width as a fraction of the font size.
    fn advance(self) -> f32 {
        match self {
            Symbol::ArrowRight | Symbol::ArrowLeft => 1.0,
            Symbol::ArrowUp | Symbol::ArrowDown | Symbol::Slash => 0.6,
            Symbol::Lightning => 0.62,
            Symbol::Crescent => 0.75,
            _ => 0.9,
        }
    }
}

/// Laid-out text ready to paint: the galley and the vector symbols placed in it.
#[derive(Clone)]
pub struct Text {
    galley: Arc<Galley>,
    symbols: Vec<(Rect, Symbol, Color32, f32)>,
    weight: Weight,
}

impl Text {
    /// Size in logical pixels.
    pub fn size(&self) -> Vec2 {
        self.galley.size()
    }

    /// Each laid-out line's bounds, relative to the top-left corner [`Text::paint`] places.
    pub fn line_rects(&self) -> Vec<Rect> {
        let origin = self.galley.rect.min.to_vec2();
        self.galley.rows.iter().map(|placed| placed.rect().translate(-origin)).collect()
    }

    /// Paint with the top-left corner of the text's bounds at `position`.
    pub fn paint(&self, painter: &Painter, position: Pos2) {
        // Galleys are positioned relative to their alignment anchor; move the bounds' corner to `position`, on the pixel
        // grid so emboldening offsets land on distinct pixels.
        let origin = (position - self.galley.rect.min.to_vec2()).round();
        let offsets: &[f32] = match self.weight {
            Weight::Regular => &[0.0],
            Weight::Bold => &[0.0, 1.0],
            Weight::Heavy => &[0.0, 1.0, 2.0],
        };
        for offset in offsets {
            painter.add(Shape::galley(origin + vec2(*offset, 0.0), self.galley.clone(), Color32::PLACEHOLDER));
        }
        for (rect, symbol, color, size) in &self.symbols {
            paint_symbol(painter, rect.translate(origin.to_vec2()), *symbol, *color, *size);
        }
    }

    /// Paint a soft dark copy under the text (CSS `text-shadow`), offset by `offset`; draw before [`Text::paint`].
    /// Text with vector symbols gets no shadow, since the override color would reveal their placeholders.
    pub fn paint_shadow(&self, painter: &Painter, position: Pos2, offset: Vec2, color: Color32) {
        if !self.symbols.is_empty() {
            return;
        }
        let origin = position - self.galley.rect.min.to_vec2() + offset;
        for spread in [vec2(0.0, 0.0), vec2(0.6, 0.6), vec2(-0.6, 0.6)] {
            painter.add(Shape::galley_with_override_text_color(origin + spread, self.galley.clone(), color));
        }
    }

    /// Paint aligned inside `rect` (`horizontal` and `vertical` alignment).
    pub fn paint_in(&self, painter: &Painter, rect: Rect, horizontal: Align, vertical: Align) -> Rect {
        let size = self.size();
        let x = match horizontal {
            Align::Min => rect.left(),
            Align::Center => rect.center().x - (size.x * 0.5),
            Align::Max => rect.right() - size.x,
        };
        let y = match vertical {
            Align::Min => rect.top(),
            Align::Center => rect.center().y - (size.y * 0.5),
            Align::Max => rect.bottom() - size.y,
        };
        let position = pos2(x, y);
        self.paint(painter, position);
        Rect::from_min_size(position, size)
    }
}

/// Lay out runs of styled text, wrapping at `wrap_width` (infinite for one line) with `align` per line.
pub fn layout(painter: &Painter, runs: &[(&str, Style)], wrap_width: f32, align: Align) -> Text {
    let mut job = LayoutJob { halign: align, ..Default::default() };
    job.wrap.max_width = wrap_width;

    // egui adds letter spacing before every glyph but the first of a section, where CSS adds it after every letter;
    // each section after the first therefore leads with the previous section's spacing.
    let mut pending: Vec<(Symbol, Color32, f32, f32)> = Vec::new();
    let mut previous_spacing: Option<f32> = None;
    let mut append = |job: &mut LayoutJob, text: &str, leading: f32, format: TextFormat, spacing: f32| {
        job.append(text, previous_spacing.unwrap_or(0.0) + leading, format);
        previous_spacing = Some(spacing);
    };
    for (text, style) in runs {
        let format = TextFormat {
            font_id: style.font(),
            extra_letter_spacing: style.spacing,
            line_height: style.line_height,
            color: style.color,
            ..Default::default()
        };
        let mut plain = String::new();
        for character in text.chars() {
            let Some(symbol) = Symbol::for_char(character) else {
                plain.push(character);
                continue;
            };
            if !plain.is_empty() {
                append(&mut job, &plain, 0.0, format.clone(), style.spacing);
                plain.clear();
            }
            // Reserve the symbol's advance: leading space plus a transparent placeholder glyph.
            let placeholder_width = painter.fonts_mut(|fonts| fonts.glyph_width(&style.font(), PLACEHOLDER));
            let reserve = ((symbol.advance() * style.size) - placeholder_width).max(0.0);
            let placeholder = TextFormat { color: Color32::TRANSPARENT, ..format.clone() };
            append(&mut job, &PLACEHOLDER.to_string(), reserve, placeholder, style.spacing);
            pending.push((symbol, style.color, style.size, reserve));
        }
        if !plain.is_empty() {
            append(&mut job, &plain, 0.0, format, style.spacing);
        }
    }
    let weight = runs.iter().map(|(_, style)| style.weight).max_by_key(|weight| *weight as u8).unwrap_or_default();
    let galley = painter.layout_job(job);

    // Placeholders appear in text order; pair each with its symbol.
    let mut symbols = Vec::new();
    let mut queue = pending.into_iter();
    for placed in &galley.rows {
        for glyph in &placed.row.glyphs {
            if glyph.chr != PLACEHOLDER {
                continue;
            }
            let Some((symbol, color, size, reserve)) = queue.next() else {
                break;
            };
            let logical = glyph.logical_rect().translate(placed.pos.to_vec2());
            let rect = Rect::from_min_size(
                pos2(logical.min.x - reserve, logical.min.y),
                vec2(symbol.advance() * size, logical.height()),
            );
            symbols.push((rect, symbol, color, size));
        }
    }
    Text { galley, symbols, weight }
}

/// Lay out one run of text.
pub fn single(painter: &Painter, text: &str, style: Style) -> Text {
    layout(painter, &[(text, style)], f32::INFINITY, Align::Min)
}

/// Lay out one run of text wrapped at `width`.
pub fn wrapped(painter: &Painter, text: &str, style: Style, width: f32, align: Align) -> Text {
    layout(painter, &[(text, style)], width, align)
}

/// Draw `symbol` in the glyph box `rect` for text of `size` pixels.
pub fn paint_symbol(painter: &Painter, rect: Rect, symbol: Symbol, color: Color32, size: f32) {
    // Glyph boxes span the font's line height; symbols sit on the x-height band like the page fonts draw them.
    let center = pos2(rect.center().x, rect.center().y + (size * 0.04));
    let half = size * 0.36;
    let stroke = Stroke::new((size * 0.085).max(1.0), color);
    let at = |x: f32, y: f32| pos2(center.x + (x * half), center.y + (y * half));
    match symbol {
        Symbol::ArrowRight | Symbol::ArrowLeft | Symbol::ArrowUp | Symbol::ArrowDown | Symbol::ArrowUpRight => {
            let direction = match symbol {
                Symbol::ArrowRight => vec2(1.0, 0.0),
                Symbol::ArrowLeft => vec2(-1.0, 0.0),
                Symbol::ArrowUp => vec2(0.0, -1.0),
                Symbol::ArrowDown => vec2(0.0, 1.0),
                _ => vec2(std::f32::consts::FRAC_1_SQRT_2, -std::f32::consts::FRAC_1_SQRT_2),
            };
            let length = if symbol == Symbol::ArrowUpRight { 0.8 } else { 1.0 };
            let tip = center + (direction * half * length);
            let tail = center - (direction * half * length);
            painter.line_segment([tail, tip], stroke);
            let side = vec2(-direction.y, direction.x);
            let head = half * 0.55;
            painter.line_segment([tip, tip - (direction * head) + (side * head)], stroke);
            painter.line_segment([tip, tip - (direction * head) - (side * head)], stroke);
        }
        Symbol::Cross => {
            painter.line_segment([at(-0.7, -0.7), at(0.7, 0.7)], stroke);
            painter.line_segment([at(0.7, -0.7), at(-0.7, 0.7)], stroke);
        }
        Symbol::Diamond => {
            let points = vec![at(0.0, -0.95), at(0.85, 0.0), at(0.0, 0.95), at(-0.85, 0.0)];
            painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
        }
        Symbol::Star | Symbol::StarOutline => {
            let tip = 1.15;
            let inner = 0.3;
            let points = [
                at(0.0, -tip),
                at(inner, -inner),
                at(tip, 0.0),
                at(inner, inner),
                at(0.0, tip),
                at(-inner, inner),
                at(-tip, 0.0),
                at(-inner, -inner),
            ];
            if symbol == Symbol::Star {
                // Four convex kites make the concave star.
                for quarter in 0..4 {
                    let previous = points[((quarter * 2) + 7) % 8];
                    let tip_point = points[quarter * 2];
                    let next = points[(quarter * 2) + 1];
                    painter.add(Shape::convex_polygon(vec![center, previous, tip_point, next], color, Stroke::NONE));
                }
            } else {
                painter.add(Shape::closed_line(points.to_vec(), Stroke::new(stroke.width * 0.8, color)));
            }
        }
        Symbol::DiamondInset => {
            let outline = vec![at(0.0, -1.05), at(1.0, 0.0), at(0.0, 1.05), at(-1.0, 0.0)];
            painter.add(Shape::closed_line(outline, Stroke::new(stroke.width * 0.8, color)));
            let inner = vec![at(0.0, -0.55), at(0.5, 0.0), at(0.0, 0.55), at(-0.5, 0.0)];
            painter.add(Shape::convex_polygon(inner, color, Stroke::NONE));
        }
        Symbol::Flower => {
            for petal in 0..8 {
                let angle = (petal as f32) * std::f32::consts::FRAC_PI_4;
                let (sin, cos) = angle.sin_cos();
                let along =
                    |distance: f32, side: f32| at((cos * distance) - (sin * side), (sin * distance) + (cos * side));
                let points = vec![along(0.15, 0.0), along(0.55, 0.22), along(1.05, 0.0), along(0.55, -0.22)];
                painter.add(Shape::convex_polygon(points, color, Stroke::NONE));
            }
            painter.circle_filled(center, half * 0.18, color);
        }
        Symbol::Lightning => {
            let points = vec![at(0.25, -1.1), at(-0.45, 0.1), at(0.25, 0.0), at(-0.35, 1.15)];
            painter.add(Shape::line(points, Stroke::new(stroke.width * 1.1, color)));
        }
        Symbol::Crescent => paint_crescent(painter, center, half * 1.05, color),
        Symbol::HotSprings => {
            for column in [-0.45, 0.0, 0.45] {
                let points: Vec<Pos2> = (0..=8)
                    .map(|step| {
                        let t = step as f32 / 8.0;
                        at(column + ((t * std::f32::consts::TAU).sin() * 0.12), -1.0 + (t * 1.1))
                    })
                    .collect();
                painter.add(Shape::line(points, Stroke::new(stroke.width * 0.8, color)));
            }
            let bowl: Vec<Pos2> = (0..=12)
                .map(|step| {
                    let angle = std::f32::consts::PI * (step as f32 / 12.0);
                    at(-angle.cos() * 0.9, 0.35 + (angle.sin() * 0.6))
                })
                .collect();
            painter.add(Shape::line(bowl, Stroke::new(stroke.width * 0.8, color)));
        }
        Symbol::Snowflake => {
            let thin = Stroke::new(stroke.width * 0.75, color);
            for spoke in 0..6 {
                let angle = (spoke as f32) * (std::f32::consts::PI / 3.0) - std::f32::consts::FRAC_PI_2;
                let (sin, cos) = angle.sin_cos();
                painter.line_segment([center, at(cos * 1.05, sin * 1.05)], thin);
                let branch = at(cos * 0.6, sin * 0.6);
                for turn in [-0.6_f32, 0.6] {
                    let (branch_sin, branch_cos) = (angle + turn).sin_cos();
                    painter.line_segment([branch, branch + vec2(branch_cos, branch_sin) * (half * 0.35)], thin);
                }
            }
        }
        Symbol::Slash => {
            painter.line_segment([at(-0.8, 1.35), at(0.8, -1.35)], Stroke::new(stroke.width * 0.6, color));
        }
    }
}

/// A crescent moon opening to the right, built as a strip between the outer and inner arcs.
fn paint_crescent(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    let inner_center = center + vec2(radius * 0.3, -radius * 0.06);
    let inner_radius = radius * 0.9;
    let mut mesh = egui::Mesh::default();
    let steps = 24;
    for step in 0..=steps {
        let y = -radius + ((2.0 * radius) * (step as f32 / steps as f32));
        let outer_x = center.x - (radius * radius - (y * y)).max(0.0).sqrt();
        let inner_dy = (center.y + y) - inner_center.y;
        let inner_x = if inner_dy.abs() < inner_radius {
            (inner_center.x - (inner_radius * inner_radius - (inner_dy * inner_dy)).sqrt()).max(outer_x)
        } else {
            center.x + (radius * radius - (y * y)).max(0.0).sqrt()
        };
        let inner_x = inner_x.min(center.x + (radius * radius - (y * y)).max(0.0).sqrt());
        mesh.colored_vertex(pos2(outer_x, center.y + y), color);
        mesh.colored_vertex(pos2(inner_x, center.y + y), color);
        if step > 0 {
            let base = (step as u32 - 1) * 2;
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base + 1, base + 3, base + 2);
        }
    }
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
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
}
