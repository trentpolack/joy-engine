// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn editor_style_uses_assets_and_registers_fonts_before_its_first_pass() {
    let context = egui::Context::default();
    apply(&context);

    // Tools select their theme inside the first UI pass, after egui has activated its fonts.
    for pass in 0..2 {
        let mut output = context.run_ui(egui::RawInput::default(), |root| {
            if pass == 0 {
                apply_editor(root.ctx());
            }
            root.set_style(root.ctx().style_of(egui::Theme::Dark));
            egui::CentralPanel::default().show(root, |ui| {
                for style in [
                    text_styles::display(),
                    text_styles::title(),
                    text_styles::caption(),
                    text_styles::status(),
                    egui::TextStyle::Monospace,
                ] {
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
    // Read independently from the generated constants: asset edits must reach the actual egui style.
    let theme: serde_json::Value = serde_json::from_str(include_str!("../../../assets/ui/theme-default.json")).unwrap();
    let layout: serde_json::Value = serde_json::from_str(include_str!("../../../assets/ui/layout.json")).unwrap();
    assert_eq!(style.visuals.panel_fill, egui::Color32::from_hex(theme["editor"]["panel"].as_str().unwrap()).unwrap());
    assert_eq!(style.text_styles[&egui::TextStyle::Monospace].size, layout["type"]["code"].as_f64().unwrap() as f32);
    assert_eq!(style.spacing.interact_size.y, layout["size"]["control_height"].as_f64().unwrap() as f32);
    let fonts = context.fonts(|fonts| fonts.definitions().clone());
    assert_eq!(fonts.families[&egui::FontFamily::Monospace][0], theme["fonts"]["code"].as_str().unwrap());
}
