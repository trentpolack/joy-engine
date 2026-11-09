// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;
use egui::vec2;

fn storage(name: &str) -> (Storage, std::path::PathBuf) {
    let directory = std::env::temp_dir().join(format!("joy-panel-layout-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    (Storage::in_directory(&directory), directory)
}

fn area(width: f32) -> Rect {
    Rect::from_min_size(pos2(0.0, 40.0), vec2(width, 600.0))
}

#[test]
fn widths_clamp_for_display_without_changing_preferences() {
    let mut layout = PanelLayout::new(PanelLayoutOptions::default(), None);
    let panes = layout.panes(area(1400.0));
    assert_eq!(panes.left.unwrap().width(), 280.0);
    assert_eq!(panes.right.unwrap().width(), 260.0);
    assert_eq!(panes.center.unwrap().width(), 860.0);
    // A narrow window: the left side caps at 36% and keeps its minimum; the right keeps its minimum.
    let narrow = layout.panes(area(600.0));
    assert!((narrow.left.unwrap().width() - 216.0).abs() < 0.01);
    assert!((narrow.right.unwrap().width() - 220.0).abs() < 0.01);
    assert_eq!(layout.widths(), (280.0, 260.0), "preferences are untouched");
}

#[test]
fn narrow_hosted_panes_stay_inside_the_host_and_restore_saved_widths() {
    let mut layout = PanelLayout::new(PanelLayoutOptions::default(), None);
    layout.set_widths(400.0, 300.0);
    for width in [320.0, 120.0, 0.0, 800.0] {
        let host = Rect::from_min_size(pos2(91.0, 137.0), vec2(width, 420.0));
        let panes = layout.panes(host);
        let mut edge = host.left();
        for pane in [panes.left, panes.center, panes.right].into_iter().flatten() {
            assert!(pane.is_finite());
            assert!(pane.width() >= 0.0);
            assert!((pane.left() - edge).abs() < 0.01, "panes must neither overlap nor leave gaps");
            assert!(pane.right() <= host.right() + 0.01, "pane escaped the hosted tab: {pane:?}");
            assert_eq!(pane.top(), host.top());
            assert_eq!(pane.bottom(), host.bottom());
            edge = pane.right();
        }
        assert!((edge - host.right()).abs() < 0.01);
        assert_eq!(layout.widths(), (400.0, 300.0));
    }
    let wide = layout.panes(area(1600.0));
    assert_eq!(wide.left.unwrap().width(), 400.0);
    assert_eq!(wide.right.unwrap().width(), 300.0);
}

#[test]
fn hidden_panes_release_their_space_without_losing_preferences() {
    let mut layout = PanelLayout::new(
        PanelLayoutOptions {
            center_collapsible: true,
            ..Default::default()
        },
        None,
    );
    let host = area(320.0);
    layout.set_collapsed(PaneSide::Center, true);
    let panes = layout.panes(host);
    assert!(panes.center.is_none());
    assert!((panes.left.unwrap().right() - panes.right.unwrap().left()).abs() < 0.01);
    assert_eq!(panes.right.unwrap().right(), host.right());
    layout.set_collapsed(PaneSide::Left, true);
    assert_eq!(layout.panes(host).right, Some(host));
    layout.set_collapsed(PaneSide::Center, false);
    layout.set_collapsed(PaneSide::Right, true);
    assert_eq!(layout.panes(host).center, Some(host));
    assert_eq!(layout.widths(), (280.0, 260.0));
}

#[test]
fn preferences_persist_and_never_collapse_every_pane() {
    let (storage, directory) = storage("persist");
    let options = PanelLayoutOptions {
        storage_key: "joy-editor-panels-test-v2".to_owned(),
        ..Default::default()
    };
    let mut layout = PanelLayout::new(options.clone(), Some(storage.clone()));
    layout.set_widths(900.0, 300.0);
    assert!(layout.toggle(PaneSide::Left));
    assert!(!layout.set_collapsed(PaneSide::Center, true), "the center is not collapsible here");
    assert!(layout.toggle(PaneSide::Right));
    let mut reopened = PanelLayout::new(options.clone(), Some(storage.clone()));
    assert_eq!(reopened.widths(), (600.0, 300.0), "widths clamp to their limits");
    assert!(reopened.is_collapsed(PaneSide::Left) && reopened.is_collapsed(PaneSide::Right));
    assert_eq!(reopened.panes(area(1000.0)).center, Some(area(1000.0)));
    let saved = storage.load("joy-editor-panels-test-v2").unwrap().unwrap();
    assert!(saved.contains("\"leftCollapsed\":true"), "camelCase document key names: {saved}");

    let collapsible = PanelLayoutOptions {
        center_collapsible: true,
        ..options.clone()
    };
    let mut layout = PanelLayout::new(collapsible, None);
    assert!(layout.toggle(PaneSide::Left) && layout.toggle(PaneSide::Right));
    assert!(!layout.set_collapsed(PaneSide::Center, true), "the last visible pane stays");
    // Unreadable preferences fall back to defaults.
    storage.save("joy-editor-panels-test-v2", "{not json").unwrap();
    assert_eq!(PanelLayout::new(options, Some(storage)).widths(), (280.0, 260.0));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn dividers_resize_by_drag_keys_and_reset_on_double_click() {
    let context = egui::Context::default();
    let mut layout = PanelLayout::new(PanelLayoutOptions::default(), None);
    let run = |layout: &mut PanelLayout, events: Vec<egui::Event>| {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1400.0, 700.0))),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |root| {
            let panes = layout.panes(area(1400.0));
            layout.dividers(root, &panes);
        });
        output.textures_delta.clear();
    };
    let handle = pos2(280.0, 300.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(&mut layout, vec![egui::Event::PointerMoved(handle)]);
    run(&mut layout, vec![button(handle, true)]);
    run(&mut layout, vec![egui::Event::PointerMoved(handle + vec2(60.0, 0.0))]);
    run(&mut layout, vec![button(handle + vec2(60.0, 0.0), false)]);
    assert_eq!(layout.widths().0, 340.0);
    let key = |key| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Default::default(),
    };
    run(&mut layout, vec![key(Key::ArrowLeft)]);
    assert_eq!(layout.widths().0, 320.0, "a focused divider steps by 20 points");
    let moved = pos2(320.0, 300.0);
    run(&mut layout, vec![egui::Event::PointerMoved(moved)]);
    run(&mut layout, vec![button(moved, true), button(moved, false)]);
    run(&mut layout, vec![button(moved, true), button(moved, false)]);
    assert_eq!(layout.widths().0, 280.0, "double-click restores the default");
}
