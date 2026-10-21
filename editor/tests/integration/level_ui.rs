// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The level editor driven through its real UI: toolbar edits, viewport selection, ground moves and gizmo drags with
//! snapping, inspector typing, invalid input that blocks saving, undo and redo, camera controls, and a rendered
//! viewport with the editor grid.

use joy_editor::level::constants::LevelTool;
use joy_editor::workspace::main::TabContent;
use joy_editor::workspace::tool_bridge::ToolRegistry;
use joy_engine::egui;
use joy_engine::gameplay::parse_level;
use joy_engine::math::Vec2;

use crate::support::{
    Driver,
    TempFolder,
    editor,
    ui_shell,
};

fn level_workspace() -> TempFolder {
    let workspace = TempFolder::new("level-ui");
    workspace.project("demo", "Demo", &[]);
    workspace.write(
        "projects/demo/assets/track.joylevel",
        "{\n  \"version\": 1,\n  \"name\": \"Track\",\n  \"entities\": []\n}\n",
    );
    workspace
}

fn level(driver: &mut Driver) -> &mut joy_editor::level::LevelEditor {
    match &mut driver.editor.workspace_mut().active_tab_mut().unwrap().content {
        TabContent::Level(level) => level,
        _ => panic!("level tab"),
    }
}

#[test]
fn authoring_selection_moves_inspector_edits_history_and_saves_through_the_ui() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = level_workspace();
    let file = "projects/demo/assets/track.joylevel";
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));
    driver.click("row:assets/track.joylevel");
    let viewport = driver.editor.control("viewport").unwrap();
    assert!(viewport.width() > 400.0 && viewport.height() > 300.0, "the viewport has a usable authoring area");
    let outliner = driver.editor.control("level-name").unwrap();
    let inspector_heading = driver.editor.control("viewport").unwrap().max.x;
    assert!(
        outliner.max.x <= viewport.min.x + 1.0 && inspector_heading < 1440.0,
        "outliner and inspector flank the viewport"
    );

    driver.click("add-box");
    assert_eq!(level(&mut driver).document().level().entities.len(), 1);
    assert!(driver.has("entity-count:1 ENTITIES"));
    driver.fill("EntityName", "Authored obstacle");
    driver.fill("position-0", "3");
    driver.fill("scale-0", "3");
    let entity = level(&mut driver).selected().unwrap().clone();
    assert_eq!(entity.name, "Authored obstacle");
    assert_eq!((entity.transform.position.x, entity.transform.scale.x), (3.0, 3.0));

    driver.click("duplicate");
    assert_eq!(level(&mut driver).document().level().entities.len(), 2);
    driver.click("undo");
    assert_eq!(level(&mut driver).document().level().entities.len(), 1);
    driver.click("redo");
    assert_eq!(level(&mut driver).document().level().entities.len(), 2);

    // Frame the first entity, then a viewport click at the center recovers the same selection.
    let first = level(&mut driver).document().level().entities[0].id.clone();
    driver.click(&format!("entity:{first}"));
    driver.click("focus");
    level(&mut driver).select(None);
    driver.frames(2);
    driver.click_at(driver.center("viewport"));
    assert_eq!(level(&mut driver).selected().map(|entity| entity.name.clone()).as_deref(), Some("Authored obstacle"));
    driver.capture("level-selected");

    // MOVE: a ground click moves horizontally to a snapped point and keeps the height.
    driver.click("move-mode");
    assert_eq!(level(&mut driver).viewport.tool, LevelTool::Move);
    let viewport = driver.editor.control("viewport").unwrap();
    driver.click_at(Vec2::new(viewport.min.x + (viewport.width() * 0.35), viewport.min.y + (viewport.height() * 0.65)));
    let moved = level(&mut driver).selected().unwrap().transform.position;
    assert!(moved.x.fract() == 0.0 && moved.z.fract() == 0.0 && moved.x.abs() < 100.0, "{moved}");
    assert_eq!(moved.y, 0.5);

    // The gizmo's X handle drags along X only, snapped, as one undoable transaction.
    driver.frames(1);
    let tip = driver.center("gizmo:X");
    let origin = moved;
    let undo_before = level(&mut driver).document().can_undo();
    let start = tip + Vec2::new(-6.0, 0.0);
    driver.drag(start, start + Vec2::new(160.0, 0.0), 0);
    let dragged = level(&mut driver).selected().unwrap().transform.position;
    assert!(dragged.x != origin.x && dragged.x.fract() == 0.0, "{origin} -> {dragged}");
    assert_eq!((dragged.y, dragged.z), (origin.y, origin.z));
    assert!(undo_before);
    driver.click("undo");
    assert_eq!(level(&mut driver).selected().unwrap().transform.position, origin);
    driver.capture("level-gizmo");

    // A focused field commits on save; invalid input blocks saving and selection changes.
    driver.click("select-mode");
    driver.type_into("position-2", "2");
    driver.press("KeyS", egui::Modifiers::COMMAND);
    let saved = parse_level(&workspace.read(file)).unwrap();
    assert_eq!(saved.entities.len(), 2);
    assert_eq!(saved.entities[0].transform.position.z, 2.0);
    assert_eq!(driver.status(), "Saved assets/track.joylevel");

    driver.type_into("scale-1", "not a number");
    driver.press("Tab", egui::Modifiers::NONE);
    driver.press("KeyS", egui::Modifiers::COMMAND);
    assert!(driver.status().contains("Enter a finite number."), "{}", driver.status());
    assert_eq!(parse_level(&workspace.read(file)).unwrap(), saved, "invalid input never reaches the file");
    let second = level(&mut driver).document().level().entities[1].id.clone();
    driver.click(&format!("entity:{second}"));
    assert_eq!(
        level(&mut driver).document().selected_id(),
        Some(first.as_str()),
        "invalid input blocks selection changes"
    );
    driver.fill("scale-1", "2");
    driver.press("KeyS", egui::Modifiers::COMMAND);
    assert_eq!(parse_level(&workspace.read(file)).unwrap().entities[0].transform.scale.y, 2.0);
}

#[test]
fn components_and_camera_controls_work_through_the_ui() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = level_workspace();
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));
    driver.click("row:assets/track.joylevel");
    driver.click("add-light");
    assert!(driver.has("group:light"), "{:?}", driver.editor.view().controls.names());
    // Component fields follow the transform and may start below the inspector's fold.
    let inspector = driver.center("inspector");
    driver.scroll(inspector, Vec2::new(0.0, -2000.0));
    driver.fill("property:light.range", "0");
    assert!(level(&mut driver).selected_issues().iter().any(|issue| issue.component == "light"));
    assert!(driver.editor.view().controls.names().iter().any(|name| name.starts_with("issue:light:")));
    // The light group sits below the transform; scroll the inspector to reach its remove button.
    driver.scroll(inspector, Vec2::new(0.0, -2000.0));
    driver.click("remove:light");
    assert!(!level(&mut driver).selected().unwrap().components.contains_key("light"));
    driver.click("undo");
    assert!(level(&mut driver).selected().unwrap().components.contains_key("light"));

    // Camera: wheel zoom, F frame, 0 reset, Shift drag pan, drag orbit.
    let center = driver.center("viewport");
    let distance = level(&mut driver).viewport.camera.distance;
    driver.scroll(center, Vec2::new(0.0, -120.0));
    driver.frames(2);
    assert!(level(&mut driver).viewport.camera.distance > distance, "scrolling down zooms out");
    let yaw = level(&mut driver).viewport.camera.yaw;
    driver.drag(center, center + Vec2::new(80.0, 0.0), 0);
    assert!(level(&mut driver).viewport.camera.yaw < yaw, "dragging orbits");
    let target = level(&mut driver).viewport.camera.target;
    driver.drag(center, center + Vec2::new(0.0, 60.0), 2);
    assert_ne!(level(&mut driver).viewport.camera.target, target, "secondary drag pans");
    driver.click_at(center + Vec2::new(150.0, 120.0));
    driver.press("Digit0", egui::Modifiers::NONE);
    assert_eq!(level(&mut driver).viewport.camera.distance, 26.0, "0 resets the camera");
    driver.press("KeyF", egui::Modifiers::NONE);
    assert!(level(&mut driver).viewport.camera.distance >= 6.0);
    driver.click("grid-toggle");
    assert!(!level(&mut driver).viewport.show_grid);
}

#[test]
fn the_viewport_renders_primitives_and_the_editor_grid() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = level_workspace();
    workspace.write("projects/demo/assets/track.joylevel", &crate::support::level_text("Track"));
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));
    driver.click("row:assets/track.joylevel");
    driver.frames(3);
    assert_eq!(level(&mut driver).viewport.instance_count(), 1);
    assert!(level(&mut driver).viewport.grid_spacing().is_some_and(|spacing| spacing > 0.0));
    let pixels = driver.shell.capture().unwrap();
    let viewport = driver.editor.control("viewport").unwrap();
    let sample = |x: f32, y: f32| {
        let offset = (((y as u32) * crate::support::WIDTH) + (x as u32)) as usize * 4;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    };
    let background = sample(viewport.min.x + 30.0, viewport.min.y + 60.0);
    let center = sample(viewport.center().x, viewport.center().y);
    assert_ne!(background, center, "the framed box draws over the clear color");
    driver.capture("level-viewport");
}

/// Screenshots of VEHICLE PLAYGROUND's level, run when `JOY_EDITOR_JOYGAMES` names a JoyGames checkout.
#[test]
fn vehicle_playground_capture() {
    let Ok(checkout) = std::env::var("JOY_EDITOR_JOYGAMES") else {
        return;
    };
    let Some(shell) = joy_engine::HeadlessShell::new(joy_engine::HeadlessOptions {
        width: 1600,
        height: 1000,
        ..Default::default()
    })
    .ok() else {
        return;
    };
    let storage = TempFolder::new("playground-storage");
    let mut driver = Driver::start(
        shell,
        editor(std::path::Path::new(&checkout), Some(&storage.root), ToolRegistry::new(), Some("vehicle-playground")),
    );
    driver.capture("rust-workspace");
    driver.click("row:assets/playground.joylevel");
    driver.frames(10);
    driver.capture("rust-level");
    // The kart sits far down the 56-entity outliner; filter to reach it, then show the whole scene again.
    driver.type_into("scene-filter", "Coral");
    driver.click("entity:kart-coral");
    driver.type_into("scene-filter", "");
    driver.frames(4);
    driver.capture("rust-level-selected");
    driver.click("move-mode");
    driver.frames(2);
    driver.capture("rust-level-gizmo");
    driver.click("workspace-help-open");
    driver.frames(20);
    driver.capture("rust-documentation");
}
