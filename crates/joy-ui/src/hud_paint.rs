// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Painter primitives for game HUDs and menus: gradients, shadows, rules, meters, glows, dashes, check marks, and the
//! value math behind custom sliders.
//!
//! Games keep their own look (palettes, fonts, and screen composition); this kit is the shape vocabulary they share, so
//! each game's interface module only says what to draw and in which colors. Everything here paints immediately into
//! an egui [`Painter`] in logical points and owns no state. Pair it with [`crate::hud_text`] for styled text.

use egui::{
    Color32,
    CornerRadius,
    Mesh,
    Painter,
    Pos2,
    Rect,
    Shadow,
    Shape,
    Stroke,
    StrokeKind,
    pos2,
    vec2,
};

/// The direction a linear gradient runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientAxis {
    /// Top to bottom.
    Vertical,
    /// Left to right.
    Horizontal,
}

/// An edge of a rectangle, for rules painted along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The left edge.
    Left,
    /// The top edge.
    Top,
    /// The right edge.
    Right,
    /// The bottom edge.
    Bottom,
}

// ---------------------------------------------------------------------------------------------------------------------
// Gradients and color
// ---------------------------------------------------------------------------------------------------------------------

/// Fill `rect` with a multi-stop linear gradient along `axis`; stop positions run 0 to 1 and must be ascending. Empty
/// rectangles and fewer than two stops paint nothing.
pub fn gradient(painter: &Painter, rect: Rect, stops: &[(f32, Color32)], axis: GradientAxis) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 || stops.len() < 2 {
        return;
    }
    let mut mesh = Mesh::default();
    for (position, color) in stops {
        match axis {
            GradientAxis::Horizontal => {
                let x = rect.left() + (rect.width() * position);
                mesh.colored_vertex(pos2(x, rect.top()), *color);
                mesh.colored_vertex(pos2(x, rect.bottom()), *color);
            }
            GradientAxis::Vertical => {
                let y = rect.top() + (rect.height() * position);
                mesh.colored_vertex(pos2(rect.left(), y), *color);
                mesh.colored_vertex(pos2(rect.right(), y), *color);
            }
        }
    }
    // Each band between neighboring stops is one quad of two triangles.
    for band in 0..(stops.len() as u32 - 1) {
        let base = band * 2;
        mesh.add_triangle(base, base + 1, base + 3);
        mesh.add_triangle(base, base + 3, base + 2);
    }
    painter.add(Shape::mesh(mesh));
}

/// Fill `rect` with a top-to-bottom gradient.
pub fn vertical_gradient(painter: &Painter, rect: Rect, top: Color32, bottom: Color32) {
    gradient(painter, rect, &[(0.0, top), (1.0, bottom)], GradientAxis::Vertical);
}

/// Fill `rect` with a left-to-right gradient.
pub fn horizontal_gradient(painter: &Painter, rect: Rect, left: Color32, right: Color32) {
    gradient(painter, rect, &[(0.0, left), (1.0, right)], GradientAxis::Horizontal);
}

/// A disc of `radius` around `center` filled with a radial gradient from `inner` at `focus` to `outer` at the
/// farthest corner of the disc's bounding box (CSS `radial-gradient(circle at <focus>, inner, outer)`).
pub fn radial_disc(painter: &Painter, center: Pos2, radius: f32, focus: Pos2, inner: Color32, outer: Color32) {
    const SEGMENTS: u32 = 48;
    const RINGS: u32 = 3;
    let box_corners = [
        center + vec2(-radius, -radius),
        center + vec2(radius, -radius),
        center + vec2(radius, radius),
        center + vec2(-radius, radius),
    ];
    let extent = box_corners.iter().map(|corner| corner.distance(focus)).fold(0.0_f32, f32::max).max(1.0);
    let color_at = |point: Pos2| lerp_color(inner, outer, (point.distance(focus) / extent).clamp(0.0, 1.0));

    // A fan from the focus, split into rings so the color interpolates along each spoke.
    let mut mesh = Mesh::default();
    mesh.colored_vertex(focus, inner);
    for segment in 0..SEGMENTS {
        let angle = (segment as f32 / SEGMENTS as f32) * std::f32::consts::TAU;
        let rim = center + (vec2(angle.cos(), angle.sin()) * radius);
        for ring in 1..=RINGS {
            let point = focus + ((rim - focus) * (ring as f32 / RINGS as f32));
            mesh.colored_vertex(point, color_at(point));
        }
    }
    let index = |segment: u32, ring: u32| 1 + ((segment % SEGMENTS) * RINGS) + (ring - 1);
    for segment in 0..SEGMENTS {
        mesh.add_triangle(0, index(segment, 1), index(segment + 1, 1));
        for ring in 1..RINGS {
            let (a, b) = (index(segment, ring), index(segment + 1, ring));
            let (c, d) = (index(segment, ring + 1), index(segment + 1, ring + 1));
            mesh.add_triangle(a, c, d);
            mesh.add_triangle(a, d, b);
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// Linear interpolation between two colors (premultiplied channels, rounded), `t` from 0 to 1.
pub fn lerp_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| (f32::from(a) + ((f32::from(b) - f32::from(a)) * t)).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
        mix(from.a(), to.a()),
    )
}

// ---------------------------------------------------------------------------------------------------------------------
// Panels and rules
// ---------------------------------------------------------------------------------------------------------------------

/// A soft drop shadow under `rect`, `offset_y` points down and blurred over `blur` points
/// (CSS `box-shadow: 0 <offset> <blur> <color>`).
pub fn drop_shadow(painter: &Painter, rect: Rect, offset_y: i8, blur: u8, color: Color32) {
    let shadow = Shadow {
        offset: [0, offset_y],
        blur,
        spread: 0,
        color,
    };
    painter.add(shadow.as_shape(rect, 0.0));
}

/// A one-point border inside `rect`.
pub fn inner_border(painter: &Painter, rect: Rect, color: Color32) {
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, color), StrokeKind::Inside);
}

/// A solid rule `width` points thick along one edge of `rect`, inside it (CSS `border-left` and friends).
pub fn edge_rule(painter: &Painter, rect: Rect, edge: Edge, width: f32, color: Color32) {
    let rule = match edge {
        Edge::Left => Rect::from_min_size(rect.min, vec2(width, rect.height())),
        Edge::Top => Rect::from_min_size(rect.min, vec2(rect.width(), width)),
        Edge::Right => Rect::from_min_max(pos2(rect.right() - width, rect.top()), rect.max),
        Edge::Bottom => Rect::from_min_max(pos2(rect.left(), rect.bottom() - width), rect.max),
    };
    painter.rect_filled(rule, 0.0, color);
}

/// A horizontal dashed rule from `left` to `right` at `y`: `dash`-point dashes every `period` points.
pub fn dashed_line(painter: &Painter, left: f32, right: f32, y: f32, dash: f32, period: f32, stroke: Stroke) {
    if period <= 0.0 {
        return;
    }
    let mut x = left;
    while x < right {
        painter.line_segment([pos2(x, y), pos2((x + dash).min(right), y)], stroke);
        x += period;
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Meters
// ---------------------------------------------------------------------------------------------------------------------

/// The left part of `rect` a meter at `fraction` fills. Fractions are clamped to 0–1; non-finite fractions (an empty
/// pool divided by zero) fill nothing.
pub fn fill_rect(rect: Rect, fraction: f64) -> Rect {
    let fraction = if fraction.is_finite() { fraction.clamp(0.0, 1.0) as f32 } else { 0.0 };
    Rect::from_min_size(rect.min, vec2(rect.width() * fraction, rect.height()))
}

/// A flat meter: `track` across `rect`, then `fraction` of it in `fill`.
pub fn bar(painter: &Painter, rect: Rect, fraction: f64, fill: Color32, track: Color32) {
    painter.rect_filled(rect, 0.0, track);
    let filled = fill_rect(rect, fraction);
    if filled.width() > 0.0 {
        painter.rect_filled(filled, 0.0, fill);
    }
}

/// A soft glow around `rect`: each `(grow, alpha)` layer paints `color` at `alpha` opacity, expanded by `grow` points
/// with matching rounding. List the widest layer first (CSS `box-shadow: 0 0 <blur> <color>`, approximated). A
/// zero-width `rect` still glows, as an empty meter's edge does.
pub fn glow(painter: &Painter, rect: Rect, color: Color32, layers: &[(f32, f32)]) {
    for (grow, alpha) in layers {
        painter.rect_filled(rect.expand(*grow), CornerRadius::same(*grow as u8), color.gamma_multiply(*alpha));
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Controls
// ---------------------------------------------------------------------------------------------------------------------

/// A check mark inside `square` (a checkbox's box), drawn with `stroke`.
pub fn check_mark(painter: &Painter, square: Rect, stroke: Stroke) {
    let points = vec![
        pos2(square.left() + 3.0, square.center().y),
        pos2(square.left() + 6.0, square.bottom() - 3.5),
        pos2(square.right() - 3.0, square.top() + 3.5),
    ];
    painter.add(Shape::line(points, stroke));
}

/// The value a custom slider over `range` holds at `fraction` (0–1) of its track, snapped to `step` from the range's
/// start and to six decimal places so repeated steps never drift (`0.1 * 3` reads as `0.3`). Clamped to the range.
pub fn slider_value(fraction: f64, range: (f64, f64), step: f64) -> f64 {
    let fraction = if fraction.is_finite() { fraction.clamp(0.0, 1.0) } else { 0.0 };
    let raw = range.0 + (fraction * (range.1 - range.0));
    let stepped = if step > 0.0 { range.0 + (((raw - range.0) / step).round() * step) } else { raw };
    ((stepped * 1e6).round() / 1e6).clamp(range.0, range.1)
}

/// Where `value` sits along a slider over `range`, from 0 to 1.
pub fn slider_fraction(value: f64, range: (f64, f64)) -> f64 {
    let span = range.1 - range.0;
    if span.abs() <= f64::EPSILON { 0.0 } else { ((value - range.0) / span).clamp(0.0, 1.0) }
}

#[cfg(test)]
#[path = "hud_paint.test.rs"]
mod tests;
