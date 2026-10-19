// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The real tool on a real device, driven through its interface with synthetic input (`HeadlessShell::step_with_ui`):
//! the default study builds and renders through the engine, examples open, parameters edit with undo, panels hide and
//! maximize, and exports write. Needs a GPU adapter (llvmpipe or SwiftShader is enough); without one each test prints
//! a note and returns. Set `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

use form_lab::FormLab;
use joy_engine::development::ToolHosting;
use joy_engine::egui;
use joy_engine::runtime::Storage;
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};

/// Capture width.
pub const WIDTH: u32 = 1280;
/// Capture height.
pub const HEIGHT: u32 = 800;

/// A headless shell with FORM LAB's renderer settings, or `None` (with a note) without an adapter.
pub fn shell() -> Option<HeadlessShell> {
    let options = HeadlessOptions {
        width: WIDTH,
        height: HEIGHT,
        renderer: form_lab::shell_options().renderer,
        ..Default::default()
    };
    match HeadlessShell::new(options) {
        Ok(shell) => Some(shell),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

/// A standalone FORM LAB whose recovery and layout live in a fresh temporary folder.
pub fn lab(name: &str) -> FormLab {
    let directory = std::env::temp_dir().join(format!("form-lab-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    FormLab::with_storage(ToolHosting::Standalone, Some(Storage::in_directory(directory)))
}

/// UI input carrying `events`.
pub fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput { events, ..Default::default() }
}

/// Run UI frames until builds settle (at most `limit` frames), finishing outstanding builds between frames.
pub fn settle(shell: &mut HeadlessShell, lab: &mut FormLab, limit: usize) {
    for _ in 0..limit {
        lab.finish_build();
        shell.step_with_ui(lab, input(Vec::new())).expect("UI frame");
        if !lab.session().compiling() && !lab.session().rebuild_scheduled() {
            break;
        }
    }
    shell.step_with_ui(lab, input(Vec::new())).expect("settled frame");
}

/// Save the latest frame as `name` when captures are requested, and return its pixels.
pub fn capture(shell: &HeadlessShell, name: &str) -> Vec<u8> {
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        shell.save_png(&std::path::Path::new(&directory).join(format!("{name}.png"))).expect("png written");
    }
    shell.capture().expect("readback")
}

/// The RGB of a pixel.
pub fn pixel(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let offset = ((y * WIDTH + x) * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

#[test]
fn the_default_study_builds_and_renders_through_the_engine() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = lab("default");
    shell.start(&mut lab).expect("FORM LAB starts");
    settle(&mut shell, &mut lab, 30);
    let pixels = capture(&shell, "form-lab-default");
    let session = lab.session();
    assert!(session.data().is_some(), "{:?}", session.status());
    assert_eq!(session.name, "Wave study");
    assert!(lab.viewport().has_environment(), "the studio environment is prepared");
    let pane = lab.viewport().pane().expect("the preview has a pane");
    // The wave surface fills the middle of the preview with warm tones.
    let center = pane.min + (pane.size * 0.5);
    let middle = pixel(&pixels, center.x as u32, center.y as u32);
    assert!(middle[0] > middle[2], "the tinted surface is warm at the preview center: {middle:?}");
}

/// Press and release the primary button at `position`.
pub fn click(shell: &mut HeadlessShell, lab: &mut FormLab, position: egui::Pos2) {
    let button = |pressed| egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    shell.step_with_ui(lab, input(vec![egui::Event::PointerMoved(position)])).unwrap();
    shell.step_with_ui(lab, input(vec![button(true)])).unwrap();
    shell.step_with_ui(lab, input(vec![button(false)])).unwrap();
}

/// Drag with `button` from `from` to `to` in ten steps.
pub fn drag(
    shell: &mut HeadlessShell,
    lab: &mut FormLab,
    from: egui::Pos2,
    to: egui::Pos2,
    button: egui::PointerButton,
) {
    let event = |pos, pressed| egui::Event::PointerButton { pos, button, pressed, modifiers: egui::Modifiers::NONE };
    shell.step_with_ui(lab, input(vec![egui::Event::PointerMoved(from)])).unwrap();
    shell.step_with_ui(lab, input(vec![event(from, true)])).unwrap();
    for step in 1..=10 {
        let position = from + ((to - from) * (step as f32 / 10.0));
        shell.step_with_ui(lab, input(vec![egui::Event::PointerMoved(position)])).unwrap();
    }
    shell.step_with_ui(lab, input(vec![event(to, false)])).unwrap();
}

/// Press a key with modifiers.
pub fn key(shell: &mut HeadlessShell, lab: &mut FormLab, key: egui::Key, modifiers: egui::Modifiers) {
    let event = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
    shell.step_with_ui(lab, input(vec![event(true)])).unwrap();
    shell.step_with_ui(lab, input(vec![event(false)])).unwrap();
}

/// Type text into the focused field.
pub fn type_text(shell: &mut HeadlessShell, lab: &mut FormLab, text: &str) {
    shell.step_with_ui(lab, input(vec![egui::Event::Text(text.to_owned())])).unwrap();
}

/// The FORM LAB fixture with variants, document maps, and a masked material.
fn variant_study() -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/variant-study.formlab"),
    )
    .expect("fixture")
}

#[test]
fn examples_and_documents_render_their_materials_through_the_engine() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = lab("examples");
    shell.start(&mut lab).expect("FORM LAB starts");
    settle(&mut shell, &mut lab, 30);
    let wave = capture(&shell, "form-lab-wave-study");

    lab.open_example("assets/examples/material-studio.form").unwrap();
    assert!(!lab.confirmation_pending(), "an unedited study is replaced without asking");
    settle(&mut shell, &mut lab, 40);
    let studio = capture(&shell, "form-lab-material-studio");
    assert_eq!(lab.session().name, "Material studio");
    assert!(lab.session().data().unwrap().materials.len() >= 6, "five finishes plus Default");
    assert!(difference(&wave, &studio) > 2.0, "the preview shows the new study");

    lab.open_example("assets/examples/vessel-workshop.form").unwrap();
    settle(&mut shell, &mut lab, 40);
    capture(&shell, "form-lab-vessel-workshop");
    assert_eq!(lab.session().name, "Vessel workshop");

    lab.open_file("variant-study.formlab", &variant_study());
    settle(&mut shell, &mut lab, 40);
    let variants = capture(&shell, "form-lab-variant-study");
    let session = lab.session();
    assert_eq!(session.variants().len(), 2);
    assert_eq!(session.materials().len(), 3, "Default, Checker, and Lattice");
    assert!(lab.gpu_error().is_none(), "document maps decode: {:?}", lab.gpu_error());
    let submeshes = lab.viewport().scene().iter().next().unwrap().1.asset.submeshes().to_vec();
    assert!(submeshes.iter().any(|submesh| submesh.textures.is_some() && submesh.masked), "the lattice is masked");

    // Applying a take replaces overrides and rebuilds; the palette stays shared.
    lab.session_mut().apply_variant(1).unwrap();
    settle(&mut shell, &mut lab, 40);
    let copper = capture(&shell, "form-lab-variant-study-copper");
    assert!(lab.session().overrides().contains_key("finish"));
    assert_eq!(lab.session().materials().len(), 3);
    assert!(difference(&variants, &copper) > 0.3, "the take changes the preview");
}

#[test]
fn parameters_edit_through_the_inspector_with_property_undo() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = lab("parameters");
    shell.start(&mut lab).expect("FORM LAB starts");
    settle(&mut shell, &mut lab, 30);
    let before = lab.session().data().unwrap().positions.clone();

    // Type an exact height into the field at the right of the HEIGHT control.
    let height = lab.parameter_rect("height").expect("HEIGHT is drawn");
    let field = egui::pos2(height.right() - 30.0, height.center().y);
    click(&mut shell, &mut lab, field);
    key(&mut shell, &mut lab, egui::Key::A, egui::Modifiers::COMMAND);
    type_text(&mut shell, &mut lab, "4.2");
    key(&mut shell, &mut lab, egui::Key::Enter, egui::Modifiers::NONE);
    settle(&mut shell, &mut lab, 30);
    capture(&shell, "form-lab-parameter-edited");
    assert_eq!(lab.session().overrides().get("height").and_then(|value| value.as_f64()), Some(4.2));
    assert_ne!(lab.session().data().unwrap().positions, before, "the edit rebuilt the surface");
    assert!(lab.session().can_undo_properties());

    // Undo property restores the script default without touching the source.
    let source = lab.session().source.clone();
    let undo = lab.parameter_rect("undo").expect("Undo property is drawn");
    click(&mut shell, &mut lab, undo.center());
    settle(&mut shell, &mut lab, 30);
    assert!(lab.session().overrides().get("height").is_none(), "{:?}", lab.session().overrides());
    assert_eq!(lab.session().source, source);
    assert!(lab.session().can_redo_properties());

    // Search filters groups by title.
    let search = lab.parameter_rect("search").unwrap();
    click(&mut shell, &mut lab, search.center());
    type_text(&mut shell, &mut lab, "freq");
    shell.step_with_ui(&mut lab, input(Vec::new())).unwrap();
    assert!(lab.parameter_rect("frequency").is_some());
    assert!(lab.parameter_rect("height").is_none(), "HEIGHT is filtered out");
}

#[test]
fn panels_hide_restore_and_maximize_from_the_workbench() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = lab("panels");
    shell.start(&mut lab).expect("FORM LAB starts");
    settle(&mut shell, &mut lab, 20);
    assert!(lab.panes().left.is_some() && lab.panes().right.is_some());

    // View → Script hides the script panel.
    let view = lab.control_rect("view").expect("View is drawn");
    click(&mut shell, &mut lab, view.center());
    let script_toggle = egui::pos2(view.left() + 30.0, view.bottom() + 20.0);
    click(&mut shell, &mut lab, script_toggle);
    shell.step_with_ui(&mut lab, input(Vec::new())).unwrap();
    capture(&shell, "form-lab-script-hidden");
    assert!(lab.panes().left.is_none(), "the script panel is hidden");
    let preview = lab.viewport().pane().unwrap();
    assert!(preview.min.x < 5.0, "the preview takes the freed width: {preview:?}");

    // At least one panel always stays visible.
    use joy_engine::development::PaneSide;
    lab.layout_mut().set_collapsed(PaneSide::Right, true);
    assert!(!lab.layout_mut().set_collapsed(PaneSide::Center, true), "the last visible panel stays");
    lab.layout_mut().set_collapsed(PaneSide::Left, false);
    lab.layout_mut().set_collapsed(PaneSide::Right, false);

    // Maximize hides the tool's chrome and keeps the panes.
    shell.step_with_ui(&mut lab, input(Vec::new())).unwrap();
    let maximize = lab.control_rect("maximize").expect("Maximize is drawn");
    click(&mut shell, &mut lab, maximize.center());
    shell.step_with_ui(&mut lab, input(Vec::new())).unwrap();
    capture(&shell, "form-lab-maximized");
    assert!(lab.layout().maximized());
    assert!(lab.panes().left.unwrap().top() < 1.0, "the header and project bar are hidden");
    key(&mut shell, &mut lab, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!lab.layout().maximized(), "Escape restores");
}

#[test]
fn the_preview_orbits_zooms_and_inspects_with_pointer_input() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = lab("viewport");
    shell.start(&mut lab).expect("FORM LAB starts");
    settle(&mut shell, &mut lab, 30);
    let pane = lab.viewport().pane().unwrap();
    let center = egui::pos2(pane.min.x + pane.size.x * 0.5, pane.min.y + pane.size.y * 0.5);

    let yaw = lab.viewport().camera.yaw;
    drag(&mut shell, &mut lab, center, center + egui::vec2(120.0, 0.0), egui::PointerButton::Primary);
    assert!((lab.viewport().camera.yaw - (yaw - 120.0 * 0.007)).abs() < 0.05, "drag orbits");
    let target = lab.viewport().camera.target;
    drag(&mut shell, &mut lab, center, center + egui::vec2(0.0, 80.0), egui::PointerButton::Secondary);
    assert_ne!(lab.viewport().camera.target, target, "secondary drag pans");

    let distance = lab.viewport().camera.distance;
    shell
        .step_with_ui(
            &mut lab,
            input(vec![
                egui::Event::PointerMoved(center),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -100.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
            ]),
        )
        .unwrap();
    assert!(lab.viewport().camera.distance > distance, "wheel down zooms out");

    // Clicking an authored point selects it in the attribute table.
    shell.step_with_ui(&mut lab, input(Vec::new())).unwrap();
    let data = lab.session().data().unwrap().clone();
    let camera = lab.viewport().pane_camera().unwrap();
    let frame = joy_engine::rendering::Camera::camera_frame(&camera, 1.0);
    let size = joy_engine::math::Vec2::new(WIDTH as f32, HEIGHT as f32);
    let index = 2000;
    let position = joy_engine::math::Vec3::new(
        data.positions[index * 3] as f32,
        data.positions[index * 3 + 1] as f32,
        data.positions[index * 3 + 2] as f32,
    );
    let screen = joy_engine::rendering::project_to_viewport(position, frame.view_projection(), size).unwrap();
    click(&mut shell, &mut lab, egui::pos2(screen.x, screen.y));
    shell.step_with_ui(&mut lab, input(Vec::new())).unwrap();
    capture(&shell, "form-lab-inspected");
    let selected = lab.inspection().selected().expect("an element is selected");
    let location = lab.viewport().inspection.locations[selected];
    let chosen = joy_engine::rendering::project_to_viewport(
        joy_engine::math::Vec3::new(location[0] as f32, location[1] as f32, location[2] as f32),
        frame.view_projection(),
        size,
    )
    .unwrap();
    assert!(chosen.distance(screen) <= 12.0, "the nearest element within the picking radius is selected");
}

#[test]
fn failed_builds_keep_the_last_accepted_geometry_and_offer_go_to_error() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = lab("failure");
    shell.start(&mut lab).expect("FORM LAB starts");
    settle(&mut shell, &mut lab, 30);
    let accepted = lab.session().data().unwrap().clone();
    lab.session_mut().source.push_str("\npoint(missing, 0, 0)\n");
    lab.session_mut().source_changed();
    settle(&mut shell, &mut lab, 30);
    capture(&shell, "form-lab-build-failed");
    let session = lab.session();
    assert_eq!(session.preview_state(), form_lab::session::PreviewState::Failed);
    assert!(session.status().message.contains("Previous preview retained"), "{:?}", session.status());
    assert_eq!(session.data(), Some(&accepted), "exports keep the last accepted build");
    assert_eq!(session.preview_text(), "Preview failed · last accepted geometry");
    assert!(lab.control_rect("go-to-error").is_some(), "Go to error is offered");
    assert!(session.export(form_lab::session::ExportFormat::Glb).is_ok());
}

#[test]
fn browser_recovery_restores_the_study_with_replacement_protection() {
    let directory = std::env::temp_dir().join(format!("form-lab-test-recovery-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    let storage = || Some(Storage::in_directory(directory.clone()));
    let mut first = FormLab::with_storage(ToolHosting::Standalone, storage());
    first.session_mut().set_override("height", Some(joy_engine::serde_json::json!(3)));
    assert_eq!(first.session().save_state().unwrap().0, "Browser recovery updated");
    let second = FormLab::with_storage(ToolHosting::Standalone, storage());
    assert_eq!(second.session().overrides().get("height"), Some(&joy_engine::serde_json::json!(3)));
    assert!(second.session().has_edits(), "recovery may be the only copy");
    let hosted = FormLab::with_storage(ToolHosting::Hosted { area: egui::Rect::NOTHING }, storage());
    assert!(!hosted.session().recovers(), "hosted tools leave files to the editor");
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn preview_lighting_never_changes_documents_or_exports() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = lab("lighting");
    shell.start(&mut lab).expect("FORM LAB starts");
    settle(&mut shell, &mut lab, 30);
    let lit = capture(&shell, "form-lab-lighting-default");
    let document = lab.session().save_contents(form_lab::session::SaveFormat::FormLab).unwrap();
    let glb = lab.session().export(form_lab::session::ExportFormat::Glb).unwrap();
    let mut settings = lab.viewport().lighting();
    settings.direction_x = -0.8;
    settings.intensity = 8.0;
    settings.color_b = 0.2;
    settings.environment_rotation = 2.0;
    lab.viewport_mut().set_lighting(settings).unwrap();
    assert!(
        lab.viewport_mut()
            .set_lighting(form_lab::rendering::preview_lighting::PreviewLightingSettings {
                intensity: 99.0,
                ..settings
            })
            .is_err()
    );
    shell.step_with_ui(&mut lab, input(Vec::new())).unwrap();
    let relit = capture(&shell, "form-lab-lighting-changed");
    assert!(difference(&lit, &relit) > 0.5, "lighting changes the preview");
    assert_eq!(lab.session().save_contents(form_lab::session::SaveFormat::FormLab).unwrap(), document);
    assert_eq!(lab.session().export(form_lab::session::ExportFormat::Glb).unwrap(), glb);
}

/// Mean absolute per-channel difference between two captures.
pub fn difference(left: &[u8], right: &[u8]) -> f32 {
    let total: u64 = left.iter().zip(right).map(|(a, b)| u64::from(a.abs_diff(*b))).sum();
    total as f32 / left.len() as f32
}

#[test]
fn the_studio_light_casts_directional_shadows() {
    let Some(mut shell) = shell() else {
        return;
    };
    let mut lab = lab("shadows");
    shell.start(&mut lab).expect("FORM LAB starts");
    lab.open_example("assets/examples/material-studio.form").unwrap();
    settle(&mut shell, &mut lab, 40);
    let shadowed = capture(&shell, "form-lab-shadows-on");
    assert!(shell.renderer().diagnostics().shadows_active, "the studio light casts");
    let mut settings = lab.viewport().lighting();
    settings.casts_shadow = false;
    lab.viewport_mut().set_lighting(settings).unwrap();
    shell.step_with_ui(&mut lab, input(Vec::new())).unwrap();
    let unshadowed = capture(&shell, "form-lab-shadows-off");
    assert!(!shell.renderer().diagnostics().shadows_active);
    eprintln!("shadow difference {}", difference(&shadowed, &unshadowed));
    assert!(difference(&shadowed, &unshadowed) > 0.2, "shadows darken the floor");
}
