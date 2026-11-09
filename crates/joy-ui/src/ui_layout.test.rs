// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;
use taffy::prelude::*;

#[test]
fn nested_rectangles_include_host_origin_and_reject_unrelated_nodes() {
    let mut tree: TaffyTree = TaffyTree::new();
    tree.disable_rounding();
    let leaf = tree
        .new_leaf(Style {
            size: Size {
                width: length(80.0),
                height: length(25.0),
            },
            ..Default::default()
        })
        .unwrap();
    let group = tree
        .new_with_children(
            Style {
                padding: taffy::Rect::length(12.0),
                ..Default::default()
            },
            &[leaf],
        )
        .unwrap();
    let root = tree
        .new_with_children(
            Style {
                padding: taffy::Rect::length(7.0),
                ..Default::default()
            },
            &[group],
        )
        .unwrap();
    tree.compute_layout(root, Size::MAX_CONTENT).unwrap();
    let origin = egui::pos2(91.0, 137.0);
    assert_eq!(
        node_rect(&tree, root, leaf, origin).unwrap(),
        egui::Rect::from_min_size(origin + egui::vec2(19.0, 19.0), egui::vec2(80.0, 25.0))
    );
    let other = tree.new_leaf(Style::default()).unwrap();
    assert!(matches!(node_rect(&tree, root, other, origin), Err(LayoutError::OutsideRoot)));
}

#[test]
fn measured_text_wraps_when_the_available_width_changes() {
    let context = egui::Context::default();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        let font = egui::FontId::proportional(16.0);
        let text = "A parameter description that must wrap inside a narrow inspector.";
        let natural = measure_text(ui.ctx(), &font, text, Size::NONE, Size::MAX_CONTENT);
        let narrow = measure_text(
            ui.ctx(),
            &font,
            text,
            Size {
                width: Some(90.0),
                height: None,
            },
            Size::MAX_CONTENT,
        );
        assert_eq!(narrow.width, 90.0);
        assert!(narrow.height > natural.height);
        let minimum = measure_text(ui.ctx(), &font, text, Size::NONE, Size::MIN_CONTENT);
        assert!(minimum.width.is_finite() && minimum.height.is_finite());
    });
    output.textures_delta.clear();
}

#[test]
fn field_identity_and_focus_survive_resizing_and_hidden_siblings() {
    let context = egui::Context::default();
    let mut text = "draft".to_owned();
    let mut previous = None;
    for frame in 0..3 {
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            if frame == 0 {
                show_region(ui, "left", egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(90.0, 100.0)), |ui| {
                    let _ = ui.button("sibling");
                });
            }
            let rect =
                egui::Rect::from_min_size(egui::pos2(150.0, 20.0), egui::vec2(240.0 - frame as f32 * 30.0, 100.0));
            let response = show_region(ui, "inspector", rect, |ui| ui.text_edit_singleline(&mut text)).inner;
            if let Some(id) = previous {
                assert_eq!(response.id, id);
                assert!(response.has_focus());
            } else {
                response.request_focus();
                previous = Some(response.id);
            }
        });
        output.textures_delta.clear();
    }
    assert_eq!(text, "draft");
}

#[test]
fn oversized_contents_do_not_receive_clicks_outside_their_region() {
    let context = egui::Context::default();
    let point = egui::pos2(170.0, 35.0);
    for pressed in [None, Some(true), Some(false)] {
        let mut events = vec![egui::Event::PointerMoved(point)];
        if let Some(pressed) = pressed {
            events.push(egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            });
        }
        let mut output = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                let rect = egui::Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(60.0, 50.0));
                let response = show_region(ui, "clipped", rect, |ui| {
                    assert_eq!(ui.clip_rect(), rect.intersect(ui.clip_rect()));
                    ui.add_sized([250.0, 30.0], egui::Button::new("Wide button"))
                })
                .inner;
                assert!(!response.clicked());
                assert!(!response.hovered());
            },
        );
        output.textures_delta.clear();
    }
}
