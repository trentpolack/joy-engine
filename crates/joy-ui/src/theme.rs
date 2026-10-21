// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Joy game and editor presets, applied to egui on native and browser targets.
//!
//! Edit `assets/ui/theme-default.json` for game/editor palettes, syntax colors, and font choices. Edit
//! `assets/ui/layout.json` for shared dimensions: type sizes, spacing, corners, borders, and control heights.
//! Cargo generates the constants below from those assets for every build; there is no runtime JSON loading or
//! theme switching. Games use [`apply`]; Joy Editor and both labs use [`apply_editor`], [`colors`], [`layout`], and
//! [`text_styles`]. Screen composition and persisted pane sizes remain owned by each tool.

// Cargo regenerates these values when either UI asset changes. No JSON parser ships in the UI runtime.
include!(concat!(env!("OUT_DIR"), "/ui_theme.rs"));

/// Apply the Joy visuals and register shared font families before an egui context's first pass.
pub fn apply(context: &egui::Context) {
    // egui activates fonts at the next pass boundary. Tools can select the editor look during that pass,
    // so its named families must already exist; games keep egui's default proportional font.
    install_editor_fonts(context, false);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = NAVY;
    visuals.window_fill = NAVY;
    visuals.window_stroke = egui::Stroke::new(layout::game_panel::BORDER, NAVY_LINE);
    visuals.selection.bg_fill = ORANGE;
    visuals.hyperlink_color = ORANGE;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(layout::BORDER_THIN, ORANGE);
    visuals.widgets.active.bg_fill = ORANGE;
    context.set_visuals(visuals);
}

/// Standard translucent panel frame for HUD and overlay content.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(NAVY.gamma_multiply(opacity::GAME_PANEL))
        .stroke(egui::Stroke::new(layout::game_panel::BORDER, NAVY_LINE))
        .inner_margin(egui::Margin::same(layout::game_panel::MARGIN))
        .corner_radius(egui::CornerRadius::same(layout::game_panel::RADIUS))
}

/// Semantic text styles the editor and labs share, registered by [`apply_editor`]. Use them through
/// `RichText::new(text).text_style(text_styles::caption())` (or the shared `joy_authoring::components`) instead of
/// literal sizes, so global type changes reach every tool.
pub mod text_styles {
    use egui::{
        FontFamily,
        TextStyle,
    };

    /// Stable family ID bound to the theme's display font.
    pub const FAMILY_DISPLAY: &str = "joy-display";
    /// Stable family ID bound to the theme's emphasized font for the active preset.
    pub const FAMILY_STRONG: &str = "joy-strong";

    /// Style name for application titles.
    pub const DISPLAY: &str = "joy-display";
    /// Style name for panel and section titles.
    pub const TITLE: &str = "joy-title";
    /// Style name for small uppercase field captions.
    pub const CAPTION: &str = "joy-caption";
    /// Style name for status lines and badges.
    pub const STATUS: &str = "joy-status";

    /// Application titles at the configured display size.
    pub fn display() -> TextStyle {
        TextStyle::Name(DISPLAY.into())
    }

    /// Panel and section titles at heading size, in the configured strong face.
    pub fn title() -> TextStyle {
        TextStyle::Name(TITLE.into())
    }

    /// Field captions at small size, in the configured strong face.
    pub fn caption() -> TextStyle {
        TextStyle::Name(CAPTION.into())
    }

    /// Status lines and badges at extra-small size.
    pub fn status() -> TextStyle {
        TextStyle::Name(STATUS.into())
    }

    /// The display font family.
    pub fn display_family() -> FontFamily {
        FontFamily::Name(FAMILY_DISPLAY.into())
    }

    /// The emphasized font family.
    pub fn strong_family() -> FontFamily {
        FontFamily::Name(FAMILY_STRONG.into())
    }
}

// Archivo and Archivo Black, the HTML tools' type, under the SIL Open Font License 1.1 (see the
// license files beside them in `fonts/`).
const ARCHIVO_REGULAR: &[u8] = include_bytes!("../fonts/Archivo-Regular.ttf");
const ARCHIVO_SEMIBOLD: &[u8] = include_bytes!("../fonts/Archivo-SemiBold.ttf");
// Official static weights from Omnibus-Type/Archivo, covered by `fonts/Archivo-OFL.txt`.
const ARCHIVO_MEDIUM: &[u8] = include_bytes!("../fonts/Archivo-Medium.ttf");
const ARCHIVO_BOLD: &[u8] = include_bytes!("../fonts/Archivo-Bold.ttf");
const ARCHIVO_BLACK: &[u8] = include_bytes!("../fonts/ArchivoBlack-Regular.ttf");
// Unmodified Meslo LG S from https://github.com/andreberg/Meslo-Font; license and attribution in `fonts/Meslo-LICENSE.txt`.
// The small line-gap variant keeps the Menlo-like metrics compact; script editors specify their own leading.
const MESLO_REGULAR: &[u8] = include_bytes!("../fonts/MesloLGS-Regular.ttf");

// Font data names registered with egui.
const FONT_ARCHIVO: &str = "Archivo";
const FONT_ARCHIVO_SEMIBOLD: &str = "Archivo SemiBold";
const FONT_ARCHIVO_MEDIUM: &str = "Archivo Medium";
const FONT_ARCHIVO_BOLD: &str = "Archivo Bold";
const FONT_ARCHIVO_BLACK: &str = "Archivo Black";
const FONT_MESLO: &str = "Meslo LG S";

/// Bind the asset's font selections to stable UI family IDs, preserving egui's symbol fallbacks. Only the editor
/// preset replaces the default body and monospace faces; game UI keeps egui's defaults.
fn install_editor_fonts(context: &egui::Context, use_editor_body: bool) {
    use std::sync::Arc;

    use egui::{
        FontData,
        FontDefinitions,
        FontFamily,
    };

    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    fonts.font_data.insert(FONT_ARCHIVO.into(), Arc::new(FontData::from_static(ARCHIVO_REGULAR)));
    fonts.font_data.insert(FONT_ARCHIVO_SEMIBOLD.into(), Arc::new(FontData::from_static(ARCHIVO_SEMIBOLD)));
    fonts.font_data.insert(FONT_ARCHIVO_MEDIUM.into(), Arc::new(FontData::from_static(ARCHIVO_MEDIUM)));
    fonts.font_data.insert(FONT_ARCHIVO_BOLD.into(), Arc::new(FontData::from_static(ARCHIVO_BOLD)));
    fonts.font_data.insert(FONT_ARCHIVO_BLACK.into(), Arc::new(FontData::from_static(ARCHIVO_BLACK)));
    fonts.font_data.insert(FONT_MESLO.into(), Arc::new(FontData::from_static(MESLO_REGULAR)));

    let family = |lead: &str| -> Vec<String> {
        let mut names = vec![lead.to_owned()];
        names.extend(fallbacks.iter().cloned());
        names
    };
    if use_editor_body {
        fonts.families.insert(FontFamily::Proportional, family(font_faces::UI));
        // Retain egui's monospace/symbol fallbacks for glyphs outside the bundled code font.
        fonts.families.entry(FontFamily::Monospace).or_default().insert(0, font_faces::CODE.to_owned());
    }
    let strong = if use_editor_body { font_faces::STRONG } else { font_faces::GAME_STRONG };
    fonts.families.insert(text_styles::strong_family(), family(strong));
    fonts.families.insert(text_styles::display_family(), family(font_faces::DISPLAY));
    context.set_fonts(fonts);
}

/// Apply the configured editor colors, fonts, and layout metrics to an egui context, including semantic
/// [`text_styles`]. Tools call this during their first UI pass, after the shell has called [`apply`] before starting
/// that pass. It replaces the game look for the whole context. Standalone contexts can call this before their first
/// pass to install both the fonts and the look together.
pub fn apply_editor(context: &egui::Context) {
    use egui::{
        CornerRadius,
        FontFamily,
        FontId,
        Stroke,
        TextStyle,
    };

    let square = CornerRadius::same(layout::RADIUS_NONE);
    let rounded = CornerRadius::same(layout::RADIUS_SM);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = None;
    visuals.panel_fill = colors::PANEL;
    visuals.window_fill = colors::PANEL;
    visuals.window_stroke = Stroke::new(layout::BORDER_THIN, colors::BORDER);
    visuals.window_corner_radius = rounded;
    visuals.menu_corner_radius = rounded;
    visuals.extreme_bg_color = colors::FIELD;
    visuals.text_edit_bg_color = Some(colors::FIELD);
    visuals.code_bg_color = colors::SCRIPT;
    visuals.faint_bg_color = colors::SURFACE;
    visuals.hyperlink_color = colors::ACCENT;
    visuals.warn_fg_color = colors::ACCENT_HOVER;
    visuals.error_fg_color = colors::DANGER;
    visuals.selection.bg_fill = colors::SELECTION;
    visuals.selection.stroke = Stroke::new(layout::BORDER_THIN, colors::CREAM);
    visuals.text_cursor.stroke = Stroke::new(layout::BORDER_FOCUS, colors::CREAM);
    visuals.slider_trailing_fill = true;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = colors::PANEL;
    widgets.noninteractive.weak_bg_fill = colors::PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(layout::BORDER_THIN, colors::BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(layout::BORDER_THIN, colors::TEXT);
    widgets.noninteractive.corner_radius = square;
    // Fields stay dark; buttons use the browser workbench's blue fill and visible one-pixel edges.
    widgets.inactive.bg_fill = colors::FIELD;
    widgets.inactive.weak_bg_fill = colors::BUTTON;
    widgets.inactive.bg_stroke = Stroke::new(layout::BORDER_THIN, colors::BORDER);
    widgets.inactive.fg_stroke = Stroke::new(layout::BORDER_THIN, colors::TEXT);
    widgets.inactive.corner_radius = square;
    widgets.hovered.bg_fill = colors::SURFACE;
    widgets.hovered.weak_bg_fill = colors::SURFACE;
    widgets.hovered.bg_stroke = Stroke::new(layout::BORDER_THIN, colors::ACCENT);
    widgets.hovered.fg_stroke = Stroke::new(layout::BORDER_THIN, colors::CREAM);
    widgets.hovered.corner_radius = square;
    widgets.hovered.expansion = 0.0;
    widgets.active.bg_fill = colors::SURFACE;
    widgets.active.weak_bg_fill = colors::SURFACE;
    widgets.active.bg_stroke = Stroke::new(layout::BORDER_FOCUS, colors::ACCENT);
    widgets.active.fg_stroke = Stroke::new(layout::BORDER_THIN, colors::CREAM);
    widgets.active.corner_radius = square;
    widgets.active.expansion = 0.0;
    widgets.open.bg_fill = colors::SURFACE;
    widgets.open.weak_bg_fill = colors::SURFACE;
    widgets.open.bg_stroke = Stroke::new(layout::BORDER_THIN, colors::ACCENT);
    widgets.open.fg_stroke = Stroke::new(layout::BORDER_THIN, colors::TEXT);
    widgets.open.corner_radius = square;
    context.set_visuals(visuals);

    install_editor_fonts(context, true);
    context.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(layout::TYPE_SM, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(layout::TYPE_MD, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(layout::TYPE_MD, text_styles::strong_family())),
            (TextStyle::Heading, FontId::new(layout::TYPE_LG, text_styles::strong_family())),
            (TextStyle::Monospace, FontId::new(layout::TYPE_CODE, FontFamily::Monospace)),
            (text_styles::display(), FontId::new(layout::TYPE_DISPLAY, text_styles::display_family())),
            (text_styles::title(), FontId::new(layout::TYPE_LG, text_styles::strong_family())),
            (text_styles::caption(), FontId::new(layout::TYPE_SM, text_styles::strong_family())),
            (text_styles::status(), FontId::new(layout::TYPE_XS, FontFamily::Proportional)),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(layout::SPACE_SM, layout::SPACE_SM);
        spacing.button_padding = egui::vec2(layout::SPACE_MD, layout::SPACE_XS);
        spacing.interact_size.y = layout::CONTROL_HEIGHT;
        spacing.window_margin = egui::Margin::same(layout::SPACE_MD as i8);
        spacing.menu_margin = egui::Margin::same(layout::SPACE_SM as i8);
        // Scroll bars reserve their width so they never cover a panel's right-aligned fields.
        spacing.scroll = egui::style::ScrollStyle::solid();
        spacing.scroll.bar_width = layout::SPACE_SM;
    });
}

#[cfg(test)]
#[path = "theme.test.rs"]
mod tests;
