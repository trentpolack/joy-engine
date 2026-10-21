// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Joy look for engine UI: navy panels with orange accents, consistent with joy-apps' default theme.
//!
//! Games and development overlays use [`apply`] and the core colors below. Authoring tools (FORM LAB, PARTICLE LAB,
//! Joy Editor) use the shared design tokens in [`tokens`], whose canonical values live in the engine's
//! `assets/theme/joy-tokens.json` (a unit test keeps the two identical), through [`apply_authoring`]. Authoring text
//! uses the Archivo family the HTML tools used (bundled under the SIL Open Font License, with egui's fonts as fallback
//! for symbols) and the semantic text styles in [`text_styles`], so headings, captions, and status lines read the same
//! in every tool.

/// Joy navy panel fill.
pub const NAVY: egui::Color32 = egui::Color32::from_rgb(0x14, 0x24, 0x2c);

/// Slightly lighter navy for panel borders and separators.
pub const NAVY_LINE: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x40, 0x4a);

/// Joy orange accent.
pub const ORANGE: egui::Color32 = egui::Color32::from_rgb(0xff, 0x6a, 0x00);

/// Primary panel text.
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xe8, 0xee, 0xee);

/// Mint status accent.
pub const MINT: egui::Color32 = egui::Color32::from_rgb(0x8f, 0xe3, 0xc0);

/// Apply the Joy visuals and register shared font families before an egui context's first pass.
pub fn apply(context: &egui::Context) {
    // egui activates fonts at the next pass boundary. Tools can select the authoring look during that pass,
    // so its named families must already exist; games keep egui's default proportional font.
    install_authoring_fonts(context, false);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = NAVY;
    visuals.window_fill = NAVY;
    visuals.window_stroke = egui::Stroke::new(1.0, NAVY_LINE);
    visuals.selection.bg_fill = ORANGE;
    visuals.hyperlink_color = ORANGE;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ORANGE);
    visuals.widgets.active.bg_fill = ORANGE;
    context.set_visuals(visuals);
}

/// Standard translucent panel frame for HUD and overlay content.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(NAVY.gamma_multiply(0.92))
        .stroke(egui::Stroke::new(1.0, NAVY_LINE))
        .inner_margin(egui::Margin::same(12))
        .corner_radius(egui::CornerRadius::same(4))
}

/// The shared Joy design tokens (the engine's `assets/theme/joy-tokens.json`) and the script editor palette. Sizes are
/// logical pixels.
pub mod tokens {
    use egui::Color32;

    /// Primary accent: focus rings, modified values, primary buttons, and the active gutter line.
    pub const ACCENT: Color32 = Color32::from_rgb(0xed, 0x6c, 0x39);
    /// Accent under the pointer.
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xff, 0x9b, 0x70);
    /// Page background behind panels.
    pub const CANVAS: Color32 = Color32::from_rgb(0x10, 0x17, 0x22);
    /// Text and number field background.
    pub const FIELD: Color32 = Color32::from_rgb(0x10, 0x1a, 0x27);
    /// Script editor background.
    pub const SCRIPT: Color32 = Color32::from_rgb(0x0c, 0x15, 0x22);
    /// Panel background.
    pub const PANEL: Color32 = Color32::from_rgb(0x1b, 0x29, 0x3d);
    /// Raised surfaces inside panels (read-only code, selected rows).
    pub const SURFACE: Color32 = Color32::from_rgb(0x25, 0x35, 0x4c);
    /// Button fill.
    pub const BUTTON: Color32 = Color32::from_rgb(0x2b, 0x40, 0x5b);
    /// Borders and separators.
    pub const BORDER: Color32 = Color32::from_rgb(0x40, 0x53, 0x6d);
    /// Primary text.
    pub const TEXT: Color32 = Color32::from_rgb(0xe3, 0xe9, 0xf1);
    /// Secondary text: labels, units, and hints.
    pub const MUTED: Color32 = Color32::from_rgb(0xa1, 0xb2, 0xc9);
    /// Errors and destructive actions.
    pub const DANGER: Color32 = Color32::from_rgb(0xef, 0x6a, 0x62);
    /// Success and live states.
    pub const SUCCESS: Color32 = Color32::from_rgb(0xb0, 0xcf, 0x81);
    /// Text cursor and highlights on dark fills.
    pub const CREAM: Color32 = Color32::from_rgb(0xff, 0xf3, 0xcf);
    /// Text selection.
    pub const SELECTION: Color32 = Color32::from_rgb(0x47, 0x60, 0x7b);
    /// Separators between workspace regions, quieter than control borders.
    pub const LINE: Color32 = Color32::from_rgb(0x2c, 0x39, 0x4b);
    /// Application header and status bar fill.
    pub const HEADER: Color32 = Color32::from_rgb(0x15, 0x1e, 0x2b);
    /// Document area fill behind tabs' content.
    pub const DOCUMENT: Color32 = Color32::from_rgb(0x11, 0x1b, 0x29);
    /// Tab strip fill.
    pub const TABS: Color32 = Color32::from_rgb(0x10, 0x19, 0x25);
    /// Selected list rows (Content Browser, outliner, emitter stack).
    pub const SELECTED_ROW: Color32 = Color32::from_rgb(0x2a, 0x36, 0x47);
    /// Text and icons drawn on the accent fill.
    pub const ON_ACCENT: Color32 = CANVAS;
    /// Development overlay panels drawn over running games.
    pub const OVERLAY: Color32 = Color32::from_rgb(0x08, 0x0b, 0x16);
    /// Fields inside development overlay panels.
    pub const OVERLAY_FIELD: Color32 = Color32::from_rgb(0x05, 0x07, 0x11);

    /// Script comments.
    pub const SCRIPT_COMMENT: Color32 = Color32::from_rgb(0x90, 0xa4, 0xbf);
    /// Script keywords and boolean literals.
    pub const SCRIPT_KEYWORD: Color32 = Color32::from_rgb(0xff, 0x9b, 0x70);
    /// Script numbers.
    pub const SCRIPT_NUMBER: Color32 = Color32::from_rgb(0xb0, 0xcf, 0x81);
    /// Script built-in functions and strings.
    pub const SCRIPT_FUNCTION: Color32 = Color32::from_rgb(0xff, 0xd1, 0x66);
    /// Script parameters (declared tool inputs).
    pub const SCRIPT_PARAMETER: Color32 = Color32::from_rgb(0x87, 0xdc, 0xe3);
    /// Script operators and punctuation.
    pub const SCRIPT_OPERATOR: Color32 = Color32::from_rgb(0xc4, 0xd4, 0xe8);
    /// Script line numbers.
    pub const SCRIPT_GUTTER: Color32 = Color32::from_rgb(0x69, 0x7e, 0x99);

    /// Extra-small type (10 px): component labels and badges.
    pub const TYPE_XS: f32 = 10.0;
    /// Small type (11 px): field titles and hints.
    pub const TYPE_SM: f32 = 11.0;
    /// Body type (13 px).
    pub const TYPE_MD: f32 = 13.0;
    /// Heading type (16 px).
    pub const TYPE_LG: f32 = 16.0;
    /// Code and numeric readouts (12 px).
    pub const TYPE_CODE: f32 = 12.0;
    /// Application titles in tool headers (30 px).
    pub const TYPE_DISPLAY: f32 = 30.0;

    /// Extra-small spacing (4 px).
    pub const SPACE_XS: f32 = 4.0;
    /// Small spacing (6 px).
    pub const SPACE_SM: f32 = 6.0;
    /// Medium spacing (10 px).
    pub const SPACE_MD: f32 = 10.0;
    /// Large spacing (16 px).
    pub const SPACE_LG: f32 = 16.0;
    /// Extra-large spacing (24 px).
    pub const SPACE_XL: f32 = 24.0;

    /// Square corners, the default for fields and panels.
    pub const RADIUS_NONE: u8 = 0;
    /// Slightly rounded corners for buttons and badges.
    pub const RADIUS_SM: u8 = 2;

    /// Hairline borders.
    pub const BORDER_THIN: f32 = 1.0;
    /// Keyboard focus rings.
    pub const BORDER_FOCUS: f32 = 2.0;
    /// Accent bars (selected list rows, panel headings).
    pub const BORDER_ACCENT: f32 = 4.0;

    /// Minimum height of interactive controls such as fields and buttons.
    pub const CONTROL_HEIGHT: f32 = 30.0;
    /// Height of an application header.
    pub const HEADER_HEIGHT: f32 = 64.0;
    /// Height of a status bar.
    pub const STATUS_HEIGHT: f32 = 26.0;
}

/// Semantic text styles authoring tools share, registered by [`apply_authoring`]. Use them through
/// `RichText::new(text).text_style(text_styles::caption())` (or the shared `joy_authoring::components`) instead of
/// literal sizes, so global type changes reach every tool.
pub mod text_styles {
    use egui::{
        FontFamily,
        TextStyle,
    };

    /// Font family for display type: Archivo Black.
    pub const FAMILY_DISPLAY: &str = "joy-display";
    /// Font family for emphasized UI type: Archivo SemiBold.
    pub const FAMILY_STRONG: &str = "joy-strong";

    /// Style name for application titles.
    pub const DISPLAY: &str = "joy-display";
    /// Style name for panel and section titles.
    pub const TITLE: &str = "joy-title";
    /// Style name for small uppercase field captions.
    pub const CAPTION: &str = "joy-caption";
    /// Style name for status lines and badges.
    pub const STATUS: &str = "joy-status";

    /// Application titles: Archivo Black at display size.
    pub fn display() -> TextStyle {
        TextStyle::Name(DISPLAY.into())
    }

    /// Panel and section titles: Archivo SemiBold at heading size.
    pub fn title() -> TextStyle {
        TextStyle::Name(TITLE.into())
    }

    /// Field captions: Archivo SemiBold at small size.
    pub fn caption() -> TextStyle {
        TextStyle::Name(CAPTION.into())
    }

    /// Status lines and badges: Archivo at extra-small size.
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

// Archivo (regular and semibold) and Archivo Black, the HTML tools' type, under the SIL Open Font License 1.1 (see the
// license files beside them in `fonts/`).
const ARCHIVO_REGULAR: &[u8] = include_bytes!("../fonts/Archivo-Regular.ttf");
const ARCHIVO_SEMIBOLD: &[u8] = include_bytes!("../fonts/Archivo-SemiBold.ttf");
const ARCHIVO_BLACK: &[u8] = include_bytes!("../fonts/ArchivoBlack-Regular.ttf");

// Font data names registered with egui.
const FONT_ARCHIVO: &str = "Archivo";
const FONT_ARCHIVO_SEMIBOLD: &str = "Archivo SemiBold";
const FONT_ARCHIVO_BLACK: &str = "Archivo Black";

/// Register Archivo's strong and display families with egui's symbol fallbacks. Only the authoring look replaces
/// the default proportional family with Archivo; game UI retains egui's default body text.
fn install_authoring_fonts(context: &egui::Context, use_authoring_body: bool) {
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
    fonts.font_data.insert(FONT_ARCHIVO_BLACK.into(), Arc::new(FontData::from_static(ARCHIVO_BLACK)));

    let family = |lead: &str| -> Vec<String> {
        let mut names = vec![lead.to_owned()];
        names.extend(fallbacks.iter().cloned());
        names
    };
    if use_authoring_body {
        fonts.families.insert(FontFamily::Proportional, family(FONT_ARCHIVO));
    }
    fonts.families.insert(text_styles::strong_family(), family(FONT_ARCHIVO_SEMIBOLD));
    fonts.families.insert(text_styles::display_family(), family(FONT_ARCHIVO_BLACK));
    context.set_fonts(fonts);
}

/// Apply the authoring tools' look (the shared Joy design tokens) to an egui context: navy panels, square fields with
/// thin borders, orange focus and accents, the Archivo fonts, and the lab type scale with the semantic
/// [`text_styles`]. Tools call this during their first UI pass, after the shell has called [`apply`] before starting
/// that pass. It replaces the game look for the whole context. Standalone contexts can call this before their first
/// pass to install both the fonts and the look together.
pub fn apply_authoring(context: &egui::Context) {
    use egui::{
        CornerRadius,
        FontFamily,
        FontId,
        Stroke,
        TextStyle,
    };

    let square = CornerRadius::same(tokens::RADIUS_NONE);
    let rounded = CornerRadius::same(tokens::RADIUS_SM);
    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = None;
    visuals.panel_fill = tokens::PANEL;
    visuals.window_fill = tokens::PANEL;
    visuals.window_stroke = Stroke::new(tokens::BORDER_THIN, tokens::BORDER);
    visuals.window_corner_radius = rounded;
    visuals.menu_corner_radius = rounded;
    visuals.extreme_bg_color = tokens::FIELD;
    visuals.text_edit_bg_color = Some(tokens::FIELD);
    visuals.code_bg_color = tokens::SCRIPT;
    visuals.faint_bg_color = tokens::SURFACE;
    visuals.hyperlink_color = tokens::ACCENT;
    visuals.warn_fg_color = tokens::ACCENT_HOVER;
    visuals.error_fg_color = tokens::DANGER;
    visuals.selection.bg_fill = tokens::SELECTION;
    visuals.selection.stroke = Stroke::new(tokens::BORDER_THIN, tokens::CREAM);
    visuals.text_cursor.stroke = Stroke::new(tokens::BORDER_FOCUS, tokens::CREAM);
    visuals.slider_trailing_fill = true;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = tokens::PANEL;
    widgets.noninteractive.weak_bg_fill = tokens::PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::BORDER);
    widgets.noninteractive.fg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::TEXT);
    widgets.noninteractive.corner_radius = square;
    widgets.inactive.bg_fill = tokens::BUTTON;
    widgets.inactive.weak_bg_fill = tokens::BUTTON;
    widgets.inactive.bg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::BORDER);
    widgets.inactive.fg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::TEXT);
    widgets.inactive.corner_radius = square;
    widgets.hovered.bg_fill = tokens::BUTTON;
    widgets.hovered.weak_bg_fill = tokens::BUTTON;
    widgets.hovered.bg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::ACCENT);
    widgets.hovered.fg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::CREAM);
    widgets.hovered.corner_radius = square;
    widgets.hovered.expansion = 0.0;
    widgets.active.bg_fill = tokens::ACCENT;
    widgets.active.weak_bg_fill = tokens::ACCENT;
    widgets.active.bg_stroke = Stroke::new(tokens::BORDER_FOCUS, tokens::ACCENT);
    widgets.active.fg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::CREAM);
    widgets.active.corner_radius = square;
    widgets.active.expansion = 0.0;
    widgets.open.bg_fill = tokens::SURFACE;
    widgets.open.weak_bg_fill = tokens::SURFACE;
    widgets.open.bg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::ACCENT);
    widgets.open.fg_stroke = Stroke::new(tokens::BORDER_THIN, tokens::TEXT);
    widgets.open.corner_radius = square;
    context.set_visuals(visuals);

    install_authoring_fonts(context, true);
    context.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Small, FontId::new(tokens::TYPE_SM, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(tokens::TYPE_MD, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(tokens::TYPE_MD, text_styles::strong_family())),
            (TextStyle::Heading, FontId::new(tokens::TYPE_LG, text_styles::strong_family())),
            (TextStyle::Monospace, FontId::new(tokens::TYPE_CODE, FontFamily::Monospace)),
            (text_styles::display(), FontId::new(tokens::TYPE_DISPLAY, text_styles::display_family())),
            (text_styles::title(), FontId::new(tokens::TYPE_LG, text_styles::strong_family())),
            (text_styles::caption(), FontId::new(tokens::TYPE_SM, text_styles::strong_family())),
            (text_styles::status(), FontId::new(tokens::TYPE_XS, FontFamily::Proportional)),
        ]
        .into();
        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(tokens::SPACE_SM, tokens::SPACE_SM);
        spacing.button_padding = egui::vec2(tokens::SPACE_MD, tokens::SPACE_XS);
        spacing.interact_size.y = tokens::CONTROL_HEIGHT - (2.0 * tokens::SPACE_XS);
        spacing.window_margin = egui::Margin::same(tokens::SPACE_MD as i8);
        spacing.menu_margin = egui::Margin::same(tokens::SPACE_SM as i8);
        // Scroll bars reserve their width so they never cover a panel's right-aligned fields.
        spacing.scroll = egui::style::ScrollStyle::solid();
        spacing.scroll.bar_width = tokens::SPACE_SM;
    });
}

#[cfg(test)]
#[path = "theme.test.rs"]
mod tests;
