// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Headless harness: the real tool on a software or hardware adapter, driven through the UI phase with synthetic input.

use joy_engine::egui::{
    self,
    Event,
    Key,
    Modifiers,
    PointerButton,
    Pos2,
};
use joy_engine::{
    HeadlessOptions,
    HeadlessShell,
};
use particle_lab::ParticleLabTool;

/// Capture width, matching the TypeScript screenshots.
pub const WIDTH: u32 = 1280;
/// Capture height.
pub const HEIGHT: u32 = 800;

/// A started tool on a headless shell, or `None` (with a note) without an adapter.
pub struct Harness {
    pub shell: HeadlessShell,
    pub tool: ParticleLabTool,
    /// Accessible widgets from the last frame: name and bounds.
    widgets: Vec<(String, egui::Rect)>,
}

impl Harness {
    pub fn new() -> Option<Self> {
        let options = HeadlessOptions { width: WIDTH, height: HEIGHT, ..Default::default() };
        let mut shell = match HeadlessShell::new(options) {
            Ok(shell) => shell,
            Err(error) => {
                eprintln!("skipping GPU test: {error}");
                return None;
            }
        };
        let mut tool = ParticleLabTool::with_storage(None);
        shell.start(&mut tool).expect("tool starts");
        shell.ui_context().enable_accesskit();
        let mut harness = Self { shell, tool, widgets: Vec::new() };
        harness.idle(3);
        Some(harness)
    }

    /// Run one frame with `events`.
    pub fn frame(&mut self, events: Vec<Event>) {
        let input = egui::RawInput { events, ..Default::default() };
        let output = self.shell.step_with_ui(&mut self.tool, input).expect("frame runs");
        if let Some(update) = output.accesskit_update {
            self.widgets = update
                .nodes
                .iter()
                .filter_map(|(_, node)| {
                    let label = node.label()?.to_owned();
                    let bounds = node.bounds()?;
                    let rect = egui::Rect::from_min_max(
                        egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                        egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                    );
                    Some((label, rect))
                })
                .collect();
        }
    }

    /// The center of the first widget whose accessible name is exactly `name`.
    pub fn find(&self, name: &str) -> Option<Pos2> {
        self.widgets.iter().find(|(label, rect)| label == name && rect.width() > 0.0).map(|(_, rect)| rect.center())
    }

    /// Click the widget named `name`.
    pub fn click_named(&mut self, name: &str) {
        let position = self.find(name).unwrap_or_else(|| panic!("no widget named {name:?}; have {:?}", self.names()));
        self.click(position);
    }

    /// Accessible names in the last frame, for failure messages.
    pub fn names(&self) -> Vec<&str> {
        self.widgets.iter().map(|(label, _)| label.as_str()).collect()
    }

    /// Replace the text in the field named `name` and confirm with Tab (the TypeScript test's `fill` then `Tab`).
    pub fn fill(&mut self, name: &str, text: &str) {
        self.click_named(name);
        self.idle(1);
        self.type_text(text);
        self.key(Key::Tab, Modifiers::NONE);
        self.idle(1);
    }

    /// Run `count` frames without input.
    pub fn idle(&mut self, count: usize) {
        for _ in 0..count {
            self.frame(Vec::new());
        }
    }

    /// Click at `position` (move, press, release over three frames).
    pub fn click(&mut self, position: Pos2) {
        self.frame(vec![Event::PointerMoved(position)]);
        self.frame(vec![button(position, true)]);
        self.frame(vec![button(position, false)]);
    }

    /// Type `text` into the focused widget.
    pub fn type_text(&mut self, text: &str) {
        self.frame(vec![Event::Text(text.to_owned())]);
    }

    /// Press `key` with `modifiers`.
    pub fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
    }

    /// Save the latest frame as `name` when `JOY_RENDER_CAPTURE_DIR` is set.
    pub fn capture(&self, name: &str) {
        if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
            std::fs::create_dir_all(&directory).expect("capture directory");
            self.shell.save_png(&std::path::Path::new(&directory).join(format!("{name}.png"))).expect("png written");
        }
    }

    /// The latest frame's RGB at `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let pixels = self.shell.capture().expect("readback");
        let offset = ((y * WIDTH + x) * 4) as usize;
        [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
    }
}

pub fn button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos: position, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}
