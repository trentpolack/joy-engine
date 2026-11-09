// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Resizable editor panes: a left panel, a center (usually the preview), and a
//! right panel inside any rectangle, so the same layout works in a standalone window and inside a Joy Editor tab area.
//!
//! Widths are preferences, never document data: they clamp to the available space when displayed without rewriting
//! the saved preference, persist as JSON through [`Storage`] under the tool's key, and a storage failure only means
//! the layout is not remembered. Dividers resize by dragging,
//! by arrow keys once focused (20 points per press), and reset to their default width on double-click. Panels can be
//! collapsed (never all at once), and layouts that allow it can maximize their center pane. Maximizing never hides a
//! tool's header or the host's chrome.
//!
//! Draw in two steps so dividers sit above pane content for hit testing:
//!
//! ```ignore
//! let panes = layout.panes(area);
//! if let Some(rect) = panes.left { ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| script_panel(ui)); }
//! // ... center and right ...
//! layout.dividers(ui, &panes);
//! ```

use egui::{
    Color32,
    CursorIcon,
    Id,
    Key,
    Rect,
    Sense,
    Stroke,
    Ui,
    pos2,
};
use serde::{
    Deserialize,
    Serialize,
};

use crate::pane_geometry::PaneGeometry;
use joy_storage::Storage;
use joy_ui::theme::{
    colors,
    layout,
};

/// Divider hit width in points.
pub const DIVIDER_WIDTH: f32 = 10.0;

/// Width change per arrow-key press on a focused divider.
pub const DIVIDER_KEY_STEP: f32 = 20.0;

/// Width limits for one side panel, in logical points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneLimits {
    /// Width before any preference is saved, and after a double-click reset.
    pub default_width: f32,
    /// Narrowest width.
    pub minimum: f32,
    /// Widest preference.
    pub maximum: f32,
}

/// What maximizing does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MaximizeMode {
    /// Show only the center pane.
    #[default]
    Viewport,
    /// The layout does not maximize: the visibility controls offer no Maximize, and panels are hidden one at a time.
    None,
}

/// One of the three panes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaneSide {
    /// The left panel (scripts, outliner).
    Left,
    /// The center pane (preview, viewport).
    Center,
    /// The right panel (inspector, parameters).
    Right,
}

impl PaneSide {
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Center => 1,
            Self::Right => 2,
        }
    }
}

/// Layout configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelLayoutOptions {
    /// Storage key for the persisted preferences, such as `joy-editor-panels-particles-v2`.
    pub storage_key: String,
    /// Left panel limits (default: 280 wide, 160 to 600).
    pub left: PaneLimits,
    /// Right panel limits (default: 260 wide, 220 to 480).
    pub right: PaneLimits,
    /// The left panel never takes more than this fraction of the layout width (default 0.36).
    pub left_fraction: f32,
    /// Width kept for the center when clamping the right panel (default 280).
    pub center_minimum: f32,
    /// Whether the center can be hidden; off by default.
    pub center_collapsible: bool,
    /// Labels for the visibility toggles: left, center, right.
    pub labels: [&'static str; 3],
    /// What maximizing does.
    pub maximize_mode: MaximizeMode,
}

impl Default for PanelLayoutOptions {
    fn default() -> Self {
        Self {
            storage_key: "joy-editor-panels-v2".to_owned(),
            left: PaneLimits {
                default_width: 280.0,
                minimum: 160.0,
                maximum: 600.0,
            },
            right: PaneLimits {
                default_width: 260.0,
                minimum: 220.0,
                maximum: 480.0,
            },
            left_fraction: 0.36,
            center_minimum: 280.0,
            center_collapsible: false,
            labels: ["Script", "Preview", "Inspector"],
            maximize_mode: MaximizeMode::Viewport,
        }
    }
}

/// The panes' rectangles for one frame; collapsed panes are `None`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PanelRects {
    /// Left panel.
    pub left: Option<Rect>,
    /// Center pane.
    pub center: Option<Rect>,
    /// Right panel.
    pub right: Option<Rect>,
}

/// Persisted preferences: camelCase JSON, kept stable so saved layouts keep loading.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavedLayout {
    left: Option<f32>,
    right: Option<f32>,
    #[serde(default)]
    left_collapsed: bool,
    #[serde(default)]
    center_collapsed: bool,
    #[serde(default)]
    right_collapsed: bool,
}

/// Three resizable panes with persisted widths.
#[derive(Clone, Debug)]
pub struct PanelLayout {
    geometry: PaneGeometry,
    options: PanelLayoutOptions,
    storage: Option<Storage>,
    left_width: f32,
    right_width: f32,
    collapsed: [bool; 3],
    maximized: bool,
    /// Width when a divider drag began.
    drag_start: Option<f32>,
}

impl PanelLayout {
    /// A layout with saved preferences from `storage` when present and readable; unreadable preferences are ignored.
    pub fn new(options: PanelLayoutOptions, storage: Option<Storage>) -> Self {
        let mut layout = Self {
            geometry: PaneGeometry::new(),
            left_width: options.left.default_width,
            right_width: options.right.default_width,
            options,
            storage,
            collapsed: [false; 3],
            maximized: false,
            drag_start: None,
        };
        let saved = layout
            .storage
            .as_ref()
            .and_then(|storage| storage.load(&layout.options.storage_key).ok().flatten())
            .and_then(|text| serde_json::from_str::<SavedLayout>(&text).ok());
        if let Some(saved) = saved {
            if let (Some(left), Some(right)) =
                (saved.left.filter(|v| v.is_finite()), saved.right.filter(|v| v.is_finite()))
            {
                layout.left_width = left.clamp(layout.options.left.minimum, layout.options.left.maximum);
                layout.right_width = right.clamp(layout.options.right.minimum, layout.options.right.maximum);
            }
            layout.collapsed = [
                saved.left_collapsed,
                layout.options.center_collapsible && saved.center_collapsed,
                saved.right_collapsed,
            ];
            if layout.collapsed.iter().all(|collapsed| *collapsed) {
                layout.collapsed[1] = false;
            }
        }
        layout
    }

    /// The preferred widths (left, right), before clamping to the available space.
    pub fn widths(&self) -> (f32, f32) {
        (self.left_width, self.right_width)
    }

    /// Set both preferred widths (presets) and persist them.
    pub fn set_widths(&mut self, left: f32, right: f32) {
        self.left_width = left.clamp(self.options.left.minimum, self.options.left.maximum);
        self.right_width = right.clamp(self.options.right.minimum, self.options.right.maximum);
        self.persist();
    }

    /// Whether `side` is hidden.
    pub fn is_collapsed(&self, side: PaneSide) -> bool {
        self.collapsed[side.index()]
    }

    /// Show or hide `side`. The last visible pane cannot be hidden, and the center only when collapsible. Returns
    /// whether the state changed.
    pub fn set_collapsed(&mut self, side: PaneSide, collapsed: bool) -> bool {
        if side == PaneSide::Center && !self.options.center_collapsible && collapsed {
            return false;
        }
        let visible = self.collapsed.iter().filter(|collapsed| !**collapsed).count();
        if collapsed && !self.collapsed[side.index()] && visible == 1 {
            return false;
        }
        if self.collapsed[side.index()] == collapsed {
            return false;
        }
        self.collapsed[side.index()] = collapsed;
        self.persist();
        true
    }

    /// Toggle `side`'s visibility.
    pub fn toggle(&mut self, side: PaneSide) -> bool {
        self.set_collapsed(side, !self.is_collapsed(side))
    }

    /// Whether the layout is maximized.
    pub fn maximized(&self) -> bool {
        self.maximized
    }

    /// What maximizing does for this layout.
    pub fn maximize_mode(&self) -> MaximizeMode {
        self.options.maximize_mode
    }

    /// Maximize or restore (presentation only; widths and collapsed panes are kept). Layouts with
    /// [`MaximizeMode::None`] stay restored.
    pub fn set_maximized(&mut self, maximized: bool) {
        self.maximized = maximized && (self.options.maximize_mode != MaximizeMode::None);
    }

    /// The pane rectangles inside `area` for this frame. Widths clamp to the space available without changing the
    /// saved preferences. Retains Taffy layout caches; does not mutate document or preference state.
    pub fn panes(&mut self, area: Rect) -> PanelRects {
        let width = area.width().max(0.0);
        let left = self.left_width.min(width * self.options.left_fraction).max(self.options.left.minimum);
        // A hidden left pane must not reserve space when sizing the right pane.
        let left_space = if self.collapsed[0] { 0.0 } else { left };
        let right =
            self.right_width.min(width - left_space - self.options.center_minimum).max(self.options.right.minimum);
        let hidden = if self.maximized && self.options.maximize_mode == MaximizeMode::Viewport {
            [true, false, true]
        } else {
            self.collapsed
        };
        let [left, center, right] = self.geometry.compute(area, [left, right], hidden);
        PanelRects {
            left,
            center,
            right,
        }
    }

    /// Draw the dividers between `panes` and apply resize gestures; call after drawing pane content.
    pub fn dividers(&mut self, ui: &mut Ui, panes: &PanelRects) {
        // Escape restores a maximized layout unless a text field or editor has the keyboard.
        if self.maximized && !ui.ctx().text_edit_focused() && ui.input(|input| input.key_pressed(Key::Escape)) {
            self.maximized = false;
        }
        let Some(center) = panes.center else {
            return;
        };
        if let Some(left) = panes.left {
            self.divider(ui, PaneSide::Left, left.right(), center, left.width());
        }
        if let Some(right) = panes.right {
            self.divider(ui, PaneSide::Right, right.left(), center, right.width());
        }
    }

    /// The visibility toggles (one per pane, the center only when collapsible) and, unless the layout's mode is
    /// [`MaximizeMode::None`], the maximize button, reading left to right in either layout direction (in a right-to-left
    /// layout they are added in reverse).
    pub fn visibility_controls(&mut self, ui: &mut Ui) {
        let visible = self.collapsed.iter().filter(|collapsed| !**collapsed).count();
        let mut items: Vec<Option<PaneSide>> = [PaneSide::Left, PaneSide::Center, PaneSide::Right]
            .into_iter()
            .filter(|side| *side != PaneSide::Center || self.options.center_collapsible)
            .map(Some)
            .collect();
        if self.options.maximize_mode != MaximizeMode::None {
            items.push(None);
        }
        if ui.layout().main_dir() == egui::Direction::RightToLeft {
            items.reverse();
        }
        for item in items {
            match item {
                Some(side) => {
                    let shown = !self.is_collapsed(side);
                    let label = self.options.labels[side.index()];
                    let response = ui.add_enabled(!(shown && visible == 1), egui::Button::new(label)).on_hover_text(
                        format!("{} {} panel", if shown { "Hide" } else { "Show" }, label.to_lowercase()),
                    );
                    if response.clicked() {
                        self.toggle(side);
                    }
                }
                None => {
                    let label = if self.maximized { "Restore" } else { "Maximize" };
                    if ui.button(label).clicked() {
                        self.maximized = !self.maximized;
                    }
                }
            }
        }
    }

    /// One divider at `x`, beside the center pane, for a side panel currently `displayed` points wide.
    fn divider(&mut self, ui: &mut Ui, side: PaneSide, x: f32, center: Rect, displayed: f32) {
        let id = Id::new(("joy-panel-divider", &self.options.storage_key, side));
        let rect = Rect::from_min_max(
            pos2(x - (DIVIDER_WIDTH * 0.5), center.top()),
            pos2(x + (DIVIDER_WIDTH * 0.5), center.bottom()),
        );
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let limits = if side == PaneSide::Left { self.options.left } else { self.options.right };
        let current = if side == PaneSide::Left { self.left_width } else { self.right_width };
        let mut next = None;
        let mut finished = false;

        if response.drag_started() {
            self.drag_start = Some(current);
            response.request_focus();
        }
        if response.dragged()
            && let (Some(start), Some(origin), Some(pointer)) =
                (self.drag_start, ui.input(|input| input.pointer.press_origin()), response.interact_pointer_pos())
        {
            let delta = pointer.x - origin.x;
            next = Some(if side == PaneSide::Left { start + delta } else { start - delta });
        }
        if response.drag_stopped() {
            self.drag_start = None;
            finished = true;
        }
        if response.double_clicked() {
            next = Some(limits.default_width);
            finished = true;
        }
        if response.has_focus() {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter {
                        horizontal_arrows: true,
                        ..Default::default()
                    },
                )
            });
            let (left_presses, right_presses) =
                ui.input(|input| (input.num_presses(Key::ArrowLeft), input.num_presses(Key::ArrowRight)));
            let steps = right_presses as f32 - left_presses as f32;
            if steps != 0.0 {
                // Keyboard steps start from the displayed width (which clamping to the available space may have
                // narrowed), not the stored preference.
                let delta = steps * DIVIDER_KEY_STEP;
                next = Some(if side == PaneSide::Left { displayed + delta } else { displayed - delta });
                finished = true;
            }
        }
        if let Some(width) = next {
            let width = width.clamp(limits.minimum, limits.maximum);
            if side == PaneSide::Left {
                self.left_width = width;
            } else {
                self.right_width = width;
            }
        }
        if finished {
            self.persist();
        }

        let active = response.dragged() || response.has_focus();
        let color = if active {
            colors::accent()
        } else if response.hovered() {
            colors::accent_hover()
        } else {
            Color32::TRANSPARENT
        };
        if color != Color32::TRANSPARENT {
            ui.painter()
                .line_segment([pos2(x, rect.top()), pos2(x, rect.bottom())], Stroke::new(layout::BORDER_FOCUS, color));
        }
        let label = if side == PaneSide::Left { "Resize editing panel" } else { "Resize inspector" };
        response
            .on_hover_cursor(CursorIcon::ResizeHorizontal)
            .on_hover_text(format!("{label} · drag or use arrow keys · double-click to reset"));
    }

    /// Save preferences; failures are ignored because layout preferences must never interrupt authoring.
    fn persist(&self) {
        let Some(storage) = &self.storage else {
            return;
        };
        let saved = SavedLayout {
            left: Some(self.left_width),
            right: Some(self.right_width),
            left_collapsed: self.collapsed[0],
            center_collapsed: self.collapsed[1],
            right_collapsed: self.collapsed[2],
        };
        if let Ok(text) = serde_json::to_string(&saved)
            && let Err(error) = storage.save(&self.options.storage_key, &text)
        {
            log::warn!("Panel layout preferences were not saved: {error}");
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "panel_layout.test.rs"]
mod tests;
