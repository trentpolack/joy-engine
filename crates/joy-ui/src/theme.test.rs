// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

/// The canonical design tokens; the Rust constants must match them exactly.
const TOKENS: &str = include_str!("../../../assets/theme/joy-tokens.json");

fn color(hex: &str) -> egui::Color32 {
    let digits = hex.trim_start_matches('#');
    let channel = |index: usize| u8::from_str_radix(&digits[index..index + 2], 16).unwrap();
    egui::Color32::from_rgb(channel(0), channel(2), channel(4))
}

fn pixels(value: &str) -> f32 {
    value.trim_end_matches("px").parse().unwrap()
}

#[test]
fn rust_tokens_match_the_canonical_token_file() {
    let document: serde_json::Value = serde_json::from_str(TOKENS).unwrap();
    let text = |group: &str, key: &str| document[group][key].as_str().unwrap_or_else(|| panic!("{group}.{key}"));
    let colors = [
        ("accent", tokens::ACCENT),
        ("accentHover", tokens::ACCENT_HOVER),
        ("canvas", tokens::CANVAS),
        ("field", tokens::FIELD),
        ("script", tokens::SCRIPT),
        ("panel", tokens::PANEL),
        ("surface", tokens::SURFACE),
        ("button", tokens::BUTTON),
        ("border", tokens::BORDER),
        ("text", tokens::TEXT),
        ("muted", tokens::MUTED),
        ("danger", tokens::DANGER),
        ("success", tokens::SUCCESS),
        ("cream", tokens::CREAM),
        ("selection", tokens::SELECTION),
        ("line", tokens::LINE),
        ("header", tokens::HEADER),
        ("document", tokens::DOCUMENT),
        ("tabs", tokens::TABS),
        ("selectedRow", tokens::SELECTED_ROW),
        ("overlay", tokens::OVERLAY),
        ("overlayField", tokens::OVERLAY_FIELD),
    ];
    for (key, value) in colors {
        assert_eq!(color(text("color", key)), value, "color.{key}");
    }
    let sizes = [
        ("type", "scaleXs", tokens::TYPE_XS),
        ("type", "scaleSm", tokens::TYPE_SM),
        ("type", "scaleMd", tokens::TYPE_MD),
        ("type", "scaleLg", tokens::TYPE_LG),
        ("type", "scaleCode", tokens::TYPE_CODE),
        ("type", "scaleDisplay", tokens::TYPE_DISPLAY),
        ("space", "xs", tokens::SPACE_XS),
        ("space", "sm", tokens::SPACE_SM),
        ("space", "md", tokens::SPACE_MD),
        ("space", "lg", tokens::SPACE_LG),
        ("space", "xl", tokens::SPACE_XL),
        ("radius", "none", f32::from(tokens::RADIUS_NONE)),
        ("radius", "sm", f32::from(tokens::RADIUS_SM)),
        ("borderWidth", "thin", tokens::BORDER_THIN),
        ("borderWidth", "focus", tokens::BORDER_FOCUS),
        ("borderWidth", "accent", tokens::BORDER_ACCENT),
        ("size", "control", tokens::CONTROL_HEIGHT),
        ("size", "header", tokens::HEADER_HEIGHT),
        ("size", "status", tokens::STATUS_HEIGHT),
    ];
    for (group, key, value) in sizes {
        assert_eq!(pixels(text(group, key)), value, "{group}.{key}");
    }
}

#[test]
fn authoring_look_registers_fonts_and_semantic_styles() {
    let context = egui::Context::default();
    apply(&context);

    // Tools select their theme inside the first UI pass, after egui has activated its fonts.
    for pass in 0..2 {
        let mut output = context.run_ui(egui::RawInput::default(), |root| {
            if pass == 0 {
                apply_authoring(root.ctx());
            }
            root.set_style(root.ctx().style_of(egui::Theme::Dark));
            egui::CentralPanel::default().show(root, |ui| {
                for style in
                    [text_styles::display(), text_styles::title(), text_styles::caption(), text_styles::status()]
                {
                    ui.label(egui::RichText::new("Authoring text").text_style(style));
                }
            });
        });
        output.textures_delta.clear();
        assert!(!context.tessellate(output.shapes, output.pixels_per_point).is_empty());
    }

    let style = context.style_of(egui::Theme::Dark);
    for name in [text_styles::DISPLAY, text_styles::TITLE, text_styles::CAPTION, text_styles::STATUS] {
        assert!(style.text_styles.contains_key(&egui::TextStyle::Name(name.into())), "{name}");
    }
    assert_eq!(style.text_styles[&text_styles::display()].family, text_styles::display_family());
}
