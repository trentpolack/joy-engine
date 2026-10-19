// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Temporary hosting workspaces, project manifests, and a headless editor driver shared by the tests.

use std::path::{
    Path,
    PathBuf,
};
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use joy_editor::server::project_files::ProjectFiles;
use joy_editor::workspace::tool_bridge::ToolRegistry;
use joy_editor::{
    EditorOptions,
    JoyEditor,
};
use joy_engine::egui;
use joy_engine::math::Vec2;
use joy_engine::runtime::Storage;
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A temporary folder removed when dropped.
pub struct TempFolder {
    pub root: PathBuf,
}

impl TempFolder {
    /// A new empty folder named after `label`.
    pub fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "joy-editor-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temporary folder");
        Self { root: std::fs::canonicalize(&root).expect("canonical temporary folder") }
    }

    /// Write `text` at `relative`, creating folders.
    pub fn write(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        path
    }

    /// Read the text at `relative`.
    pub fn read(&self, relative: &str) -> String {
        std::fs::read_to_string(self.root.join(relative)).unwrap()
    }

    /// Whether `relative` exists.
    pub fn exists(&self, relative: &str) -> bool {
        self.root.join(relative).exists()
    }

    /// A project folder with a manifest named `name` and optional extra capability extensions.
    pub fn project(&self, id: &str, name: &str, extensions: &[&str]) -> PathBuf {
        self.write(&format!("projects/{id}/project.joyproject"), &manifest(name, extensions));
        std::fs::create_dir_all(self.root.join(format!("projects/{id}/assets"))).unwrap();
        self.root.join("projects").join(id)
    }
}

impl Drop for TempFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A valid manifest; `extensions` add a `notes` capability.
pub fn manifest(name: &str, extensions: &[&str]) -> String {
    let mut capabilities = vec![joy_engine::serde_json::json!({
        "id": "authored-levels", "label": "Authored levels", "extensions": [".joylevel"],
        "assetRoot": "assets", "renderer": "scene-primitives", "features": {"physics": true}
    })];
    if !extensions.is_empty() {
        capabilities.push(joy_engine::serde_json::json!({
            "id": "notes", "label": "Notes", "extensions": extensions, "assetRoot": "assets", "renderer": "text", "features": {}
        }));
    }
    joy_engine::serde_json::json!({
        "schemaVersion": 1, "name": name, "preview": {"root": "dist", "entry": "index.html"}, "capabilities": capabilities
    })
    .to_string()
}

/// A small valid level with one box.
pub fn level_text(name: &str) -> String {
    joy_engine::serde_json::to_string_pretty(&joy_engine::serde_json::json!({
        "version": 1, "name": name, "entities": [{
            "id": "box", "name": "Box", "type": "prop", "tags": [],
            "transform": {"position": [0, 0.5, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1]},
            "components": {"render": {"shape": "box", "color": [0.9, 0.3, 0.1]}, "custom": {"nested": [1, {"label": "retain me"}]}}
        }]
    }))
    .unwrap()
        + "\n"
}

/// An editor over `workspace` with session storage in `storage` (or none).
pub fn editor(workspace: &Path, storage: Option<&Path>, tools: ToolRegistry, project: Option<&str>) -> JoyEditor {
    JoyEditor::new(EditorOptions {
        store: Box::new(ProjectFiles::new(workspace)),
        storage: storage.map(Storage::in_directory),
        tools,
        project: project.map(str::to_owned),
        startup_message: None,
    })
}

/// Logical size of headless UI tests: the TypeScript captures' laptop size.
pub const WIDTH: u32 = 1440;
/// Height of headless UI tests.
pub const HEIGHT: u32 = 900;

/// A headless shell for UI tests, or `None` (with a note) without a GPU adapter.
pub fn ui_shell() -> Option<HeadlessShell> {
    let options = HeadlessOptions { width: WIDTH, height: HEIGHT, ..Default::default() };
    match HeadlessShell::new(options) {
        Ok(shell) => Some(shell),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

/// The egui key for a DOM `KeyboardEvent.code` such as `KeyS`, `Digit0`, `Enter`, or `F2`.
pub fn key(code: &str) -> egui::Key {
    let name = code.strip_prefix("Key").or_else(|| code.strip_prefix("Digit")).unwrap_or(code);
    egui::Key::from_name(name).unwrap_or_else(|| panic!("no egui key for {code}"))
}

/// Drives a [`JoyEditor`] in a headless shell through named controls, with synthetic egui input.
pub struct Driver {
    pub shell: HeadlessShell,
    pub editor: JoyEditor,
    events: Vec<egui::Event>,
    pointer: egui::Pos2,
}

impl Driver {
    /// Start `editor` and lay out two frames so controls have positions.
    pub fn start(mut shell: HeadlessShell, mut editor: JoyEditor) -> Self {
        shell.start(&mut editor).expect("editor starts");
        let mut driver = Self { shell, editor, events: Vec::new(), pointer: egui::Pos2::ZERO };
        driver.frames(2);
        driver
    }

    /// Run `count` frames with the UI phase; queued events go to the first.
    pub fn frames(&mut self, count: u32) {
        for _ in 0..count {
            let input = egui::RawInput { events: std::mem::take(&mut self.events), ..Default::default() };
            self.shell.step_with_ui(&mut self.editor, input).expect("editor frame");
        }
    }

    /// The UI context the shell runs, for focus and memory checks.
    pub fn ui(&mut self) -> &egui::Context {
        self.shell.ui_context()
    }

    /// The center of a named control; panics with the available names when it is missing.
    pub fn center(&self, name: &str) -> Vec2 {
        let rect = self
            .editor
            .control(name)
            .unwrap_or_else(|| panic!("no control named {name}; available: {:?}", self.editor.view().controls.names()));
        assert!(rect.width() > 0.0 && rect.height() > 0.0, "control {name} is scrolled out of view: {rect:?}");
        Vec2::new(rect.center().x, rect.center().y)
    }

    /// Whether a named control was drawn in the last frame.
    pub fn has(&self, name: &str) -> bool {
        self.editor.control(name).is_some()
    }

    /// Move the pointer and run a frame.
    pub fn pointer_move(&mut self, point: Vec2) {
        self.pointer = egui::pos2(point.x, point.y);
        self.events.push(egui::Event::PointerMoved(self.pointer));
        self.frames(1);
    }

    fn button(&mut self, button: egui::PointerButton, pressed: bool, modifiers: egui::Modifiers) {
        self.events.push(egui::Event::PointerButton { pos: self.pointer, button, pressed, modifiers });
    }

    /// Click a named control, then run two frames.
    pub fn click(&mut self, name: &str) {
        let point = self.center(name);
        self.click_at(point);
    }

    /// Click a window point, then run two frames.
    pub fn click_at(&mut self, point: Vec2) {
        self.pointer_move(point);
        self.button(egui::PointerButton::Primary, true, egui::Modifiers::NONE);
        self.frames(1);
        self.button(egui::PointerButton::Primary, false, egui::Modifiers::NONE);
        self.frames(2);
    }

    /// Secondary-click a named control.
    pub fn secondary_click(&mut self, name: &str) {
        let point = self.center(name);
        self.pointer_move(point);
        self.button(egui::PointerButton::Secondary, true, egui::Modifiers::NONE);
        self.frames(1);
        self.button(egui::PointerButton::Secondary, false, egui::Modifiers::NONE);
        self.frames(2);
    }

    /// Drag from one point to another with DOM `button` (0 primary, 1 middle, 2 secondary) over several frames.
    pub fn drag(&mut self, from: Vec2, to: Vec2, button: u16) {
        let button = match button {
            0 => egui::PointerButton::Primary,
            1 => egui::PointerButton::Middle,
            _ => egui::PointerButton::Secondary,
        };
        self.pointer_move(from);
        self.button(button, true, egui::Modifiers::NONE);
        self.frames(1);
        for step in 1..=8 {
            self.pointer_move(from + ((to - from) * (step as f32 / 8.0)));
        }
        self.button(button, false, egui::Modifiers::NONE);
        self.frames(2);
    }

    /// Scroll the wheel by `delta` points (egui's sign: negative y scrolls toward the author) at `point`.
    pub fn scroll(&mut self, point: Vec2, delta: Vec2) {
        self.pointer_move(point);
        self.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(delta.x, delta.y),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        });
        self.frames(2);
    }

    fn key_event(&mut self, code: &str, modifiers: egui::Modifiers) {
        let key = key(code);
        for pressed in [true, false] {
            self.events.push(egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers });
        }
    }

    /// Focus a named text field, select its contents, type `text`, then press Enter.
    pub fn fill(&mut self, name: &str, text: &str) {
        self.type_into(name, text);
        self.press("Enter", egui::Modifiers::NONE);
    }

    /// Type into a named field (replacing its contents) without committing.
    pub fn type_into(&mut self, name: &str, text: &str) {
        self.click(name);
        self.key_event("KeyA", egui::Modifiers::COMMAND);
        self.frames(1);
        if text.is_empty() {
            self.key_event("Backspace", egui::Modifiers::NONE);
        } else {
            self.events.push(egui::Event::Text(text.to_owned()));
        }
        self.frames(2);
    }

    /// Press a key (DOM code) with modifiers, then run two frames.
    pub fn press(&mut self, code: &str, modifiers: egui::Modifiers) {
        self.key_event(code, modifiers);
        self.frames(2);
    }

    /// Save a PNG of the current frame when `JOY_EDITOR_CAPTURE_DIR` is set.
    pub fn capture(&self, name: &str) {
        if let Ok(directory) = std::env::var("JOY_EDITOR_CAPTURE_DIR") {
            std::fs::create_dir_all(&directory).expect("capture directory");
            self.shell.save_png(&Path::new(&directory).join(format!("{name}.png"))).expect("png written");
        }
    }

    /// The workspace status line.
    pub fn status(&self) -> String {
        self.editor.workspace().status().to_owned()
    }
}
