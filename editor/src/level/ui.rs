// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The level editor's egui presentation: the authoring toolbar, the scene outliner, the viewport's input and overlays
//! (tools, grid, snap, the move gizmo), and the inspector with transforms, the schema-driven component tree, and
//! Advanced Source.
//!
//! The whole editor is one egui area, laid out by the authoring kit's `PanelLayout` (outliner, viewport, inspector,
//! with persisted widths and dividers); viewport gestures come from the kit's `viewport_input`, and gizmo handles are
//! widgets drawn over the viewport so a press on a handle drags it instead of orbiting.

use std::collections::HashMap;

use joy_engine::development::editor_shell::{
    StatusTone,
    chip_button,
    chip_menu_button,
    overflow_button,
    quiet_button,
    quiet_menu_button,
    rule,
    status_label,
};
use joy_engine::development::{
    PaneLimits,
    PanelLayout,
    PanelLayoutOptions,
    ScriptEditor,
    ScriptSyntax,
    ViewportShortcut,
    rerolled_seed,
    viewport_input,
};
use joy_engine::egui::{
    self,
    Align,
    Color32,
    Id,
    Key,
    KeyboardShortcut,
    Modifiers,
    Order,
    Rect,
    RichText,
    ScrollArea,
    Sense,
    Stroke,
    TextEdit,
    Ui,
    pos2,
    vec2,
};
use joy_engine::math::Vec2;
use joy_engine::runtime::Storage;
use joy_engine::serde_json::{
    Map,
    Value,
};
use joy_engine::theme::{
    colors,
    layout,
    text_styles,
};

use super::constants::{
    DEFAULT_CUSTOM_COMPONENT,
    LevelTool,
};
use super::main::{
    AssetAccess,
    ComponentChoice,
    FieldKey,
    LevelEditor,
    TransformField,
    transform_bounds,
};
use super::property_editor::{
    CustomFieldKind,
    FieldControl,
    PropertyContext,
    PropertyGroup,
    PropertyNode,
    add_custom_field,
    append_list_item,
    build_property_tree,
    display_string,
    initial_field_value,
    value_at,
};
use super::viewport::{
    GizmoHandle,
    ViewRegion,
};
use crate::interaction::numeric_controls::{
    NumberBounds,
    display_number,
    format_number,
    json_number,
    parse_number,
    step_number,
};
use crate::interaction::viewport_input::{
    drag_camera,
    orbit_shortcut,
    zoom_camera,
};
use crate::interaction::widgets::{
    self,
    ControlMap,
    button,
    caption,
    dialog_frame,
    draft_field,
    heading,
    primary_button,
    region,
};

/// Grid snap choices in meters offered by the toolbar's Snap menu; zero turns snapping off.
const SNAP_CHOICES: [f32; 6] = [0.0, 0.25, 0.5, 1.0, 2.0, 5.0];
/// Width of the viewport's View settings popover.
const VIEW_SETTINGS_WIDTH: f32 = 250.0;
/// Height of one outliner row.
const OUTLINER_ROW_HEIGHT: f32 = 40.0;
/// Height of one inspector row of controls.
const ROW_HEIGHT: f32 = 30.0;
/// Width of a seed field's reroll button.
const SEED_REROLL_WIDTH: f32 = 30.0;
/// Upper reroll bound for a seed without a declared maximum: the nonnegative 31-bit range.
const SEED_FALLBACK_MAXIMUM: f64 = 2_147_483_647.0;
/// Storage key of the level editor's panel widths; stable so saved layouts keep loading.
const PANEL_STORAGE_KEY: &str = "joy-editor-panels-level-v2";

/// An inspector edit collected while drawing and applied afterwards.
#[derive(Clone, Debug, PartialEq)]
enum PropertyAction {
    /// Set the value at a component path.
    Set(Vec<String>, Value),
    /// Remove the property (or component) at a path.
    Remove(Vec<String>),
    /// Open a referenced asset's source.
    Open(String),
}

/// A level dialog awaiting a name.
#[derive(Clone, Debug, PartialEq, Eq)]
enum LevelDialog {
    /// Save As Object: the path beneath `assets/`.
    SaveObject(String),
    /// A custom component's name.
    CustomComponent(String),
}

/// Owns level editor UI state that is not part of the document.
pub struct LevelUi {
    panels: PanelLayout,
    dialog: Option<LevelDialog>,
    missing_choice: HashMap<String, String>,
    custom_fields: HashMap<String, (String, CustomFieldKind)>,
    source_editor: ScriptEditor,
    // The viewport's View settings popover (grid, camera reset, panels).
    view_settings_open: bool,
}

impl LevelUi {
    /// Fresh UI state; panel widths persist in `storage`.
    pub fn new(storage: Option<Storage>) -> Self {
        let options = PanelLayoutOptions {
            storage_key: PANEL_STORAGE_KEY.to_owned(),
            left: PaneLimits {
                default_width: 180.0,
                minimum: 160.0,
                maximum: 600.0,
            },
            right: PaneLimits {
                default_width: 300.0,
                minimum: 220.0,
                maximum: 480.0,
            },
            labels: ["Outliner", "Viewport", "Inspector"],
            ..Default::default()
        };
        Self {
            panels: PanelLayout::new(options, storage),
            dialog: None,
            missing_choice: HashMap::new(),
            custom_fields: HashMap::new(),
            source_editor: ScriptEditor::new("level-components-source", ScriptSyntax::json()),
            view_settings_open: false,
        }
    }

    /// Whether the viewport temporarily fills the editor (viewport-only Maximize).
    pub fn maximized(&self) -> bool {
        self.panels.maximized()
    }

    /// Draw the level editor's panes inside `rect` (the body beneath the shell's toolbar), reading and creating project
    /// content through `access`. The shell draws [`toolbar_actions`](Self::toolbar_actions) and
    /// [`toolbar_trailing`](Self::toolbar_trailing) in its toolbar and [`status_context`](Self::status_context) in its
    /// status bar.
    pub fn show(
        &mut self,
        context: &egui::Context,
        rect: Rect,
        level: &mut LevelEditor,
        access: &mut dyn AssetAccess,
        controls: &mut ControlMap,
    ) {
        // Transparent: the viewport pane shows the 3D render beneath; every other pane paints its own background.
        region(context, "level-editor", rect, Color32::TRANSPARENT, Order::Middle, |ui| {
            let panes = self.panels.panes(rect);
            if let Some(center) = panes.center {
                controls.insert("viewport", center);
                level.viewport.region = ViewRegion {
                    origin: Vec2::new(center.min.x, center.min.y),
                    size: Vec2::new(center.width(), center.height()),
                };
                joy_engine::runtime::ui_layout::show_region(ui, "viewport-pane", center, |ui| {
                    self.viewport(ui, center, level, controls);
                    self.viewport_overlay(ui, center, level, controls);
                });
            }
            if let Some(left) = panes.left {
                controls.insert("outliner", left);
                joy_engine::runtime::ui_layout::show_region(ui, "outliner-pane", left, |ui| {
                    self.outliner(ui, left, level, controls);
                });
            }
            if let Some(right) = panes.right {
                controls.insert("inspector", right);
                joy_engine::runtime::ui_layout::show_region(ui, "inspector-pane", right, |ui| {
                    self.inspector(ui, right, level, access, controls);
                });
            }
            self.panels.dividers(ui, &panes);
        });
        self.dialogs(context, level, access, controls);
        self.shortcuts(context, level);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Toolbar and status
    // -----------------------------------------------------------------------------------------------------------------

    /// The level's authoring actions in the shell toolbar: add primitives, place reusable objects, the Select and Move
    /// tools, and grid snap. Object documents edit one object, so they offer only the tools.
    pub fn toolbar_actions(
        &mut self,
        ui: &mut Ui,
        level: &mut LevelEditor,
        access: &mut dyn AssetAccess,
        controls: &mut ControlMap,
    ) {
        if !level.object_mode() {
            let primitive = chip_menu_button(ui, "+ Primitive");
            controls.mark("add-primitive", primitive.clone());
            egui::Popup::menu(&primitive).show(|ui| {
                ui.set_min_width(160.0);
                for (name, label) in
                    [("add-box", "Box"), ("add-sphere", "Sphere"), ("add-light", "Light"), ("add-empty", "Empty")]
                {
                    let item = ui.button(label);
                    if controls.mark(name, item).clicked() {
                        match name {
                            "add-box" => level.add_primitive(false),
                            "add-sphere" => level.add_primitive(true),
                            "add-light" => level.add_light(),
                            _ => level.add_empty(),
                        }
                    }
                }
            });
            self.place_asset_menu(ui, level, access, controls);
            rule(ui);
        }

        let tool = level.viewport.tool;
        let select = quiet_toggle(ui, "Select", tool == LevelTool::Select);
        if controls.mark("select-mode", select).clicked() {
            level.set_tool(LevelTool::Select);
        }
        let move_button = quiet_toggle(ui, "Move", tool == LevelTool::Move)
            .on_hover_text("Drag a gizmo handle or click the ground to move the selection");
        if controls.mark("move-mode", move_button).clicked() {
            level.set_tool(LevelTool::Move);
        }
        let snap_label = if level.viewport.snap > 0.0 {
            format!("Snap {} m", format_number(f64::from(level.viewport.snap)))
        } else {
            "Snap off".to_owned()
        };
        let snap = quiet_menu_button(ui, &snap_label).on_hover_text("Grid snap for moves and placement");
        controls.mark("snap", snap.clone());
        egui::Popup::menu(&snap).show(|ui| {
            for choice in SNAP_CHOICES {
                let label =
                    if choice > 0.0 { format!("{} m", format_number(f64::from(choice))) } else { "Off".to_owned() };
                let item = ui.selectable_label(level.viewport.snap == choice, label);
                if controls.mark(format!("snap:{choice}"), item).clicked() {
                    level.viewport.snap = choice;
                }
            }
        });
    }

    /// The toolbar's right edge for a level: Focus viewport and the overflow menu (history, duplicate, delete, and
    /// Save As Object).
    pub fn toolbar_trailing(&mut self, ui: &mut Ui, level: &mut LevelEditor, controls: &mut ControlMap) {
        let more = overflow_button(ui).on_hover_text("More actions");
        controls.mark("more-actions", more.clone());
        egui::Popup::menu(&more).show(|ui| {
            ui.set_min_width(190.0);
            let undo =
                ui.add_enabled(level.document().can_undo(), egui::Button::new("Undo").shortcut_text("Ctrl/Cmd Z"));
            if controls.mark("undo", undo).clicked() {
                level.history(false);
            }
            let redo = ui
                .add_enabled(level.document().can_redo(), egui::Button::new("Redo").shortcut_text("Ctrl/Cmd Shift Z"));
            if controls.mark("redo", redo).clicked() {
                level.history(true);
            }
            if !level.object_mode() {
                ui.separator();
                let has_selection = level.selected().is_some();
                let duplicate = ui.add_enabled(has_selection, egui::Button::new("Duplicate"));
                if controls.mark("duplicate", duplicate).clicked() {
                    level.duplicate();
                }
                let save_object = ui.add_enabled(has_selection, egui::Button::new("Save as Object…"));
                if controls.mark("save-object", save_object).clicked() {
                    self.dialog = Some(LevelDialog::SaveObject(level.suggested_object_path()));
                }
                let delete =
                    ui.add_enabled(has_selection, egui::Button::new(RichText::new("Delete").color(colors::danger())));
                if controls.mark("delete", delete).clicked() {
                    level.remove_selected();
                }
            }
        });
        let label = if self.panels.maximized() { "Restore panels" } else { "Focus viewport" };
        let focus = quiet_button(ui, label).on_hover_text("Show only the viewport");
        if controls.mark("focus-viewport", focus).clicked() {
            let maximized = self.panels.maximized();
            self.panels.set_maximized(!maximized);
        }
        status_label(ui, "Preview: Current", StatusTone::Good);
    }

    /// The status bar's right side for a level: grid spacing and drawn instances.
    pub fn status_context(&self, level: &LevelEditor) -> String {
        let spacing = level
            .viewport
            .grid_spacing()
            .map_or("Grid off".to_owned(), |spacing| format!("Grid {} m", format_number(spacing)));
        format!("{spacing} · {} instances", level.viewport.instance_count())
    }

    /// Place Asset…: instance a reusable object from the project at the scene origin.
    fn place_asset_menu(
        &mut self,
        ui: &mut Ui,
        level: &mut LevelEditor,
        access: &mut dyn AssetAccess,
        controls: &mut ControlMap,
    ) {
        let objects: Vec<String> =
            access.catalog().iter().filter(|asset| asset.kind == "joyobject").map(|asset| asset.path.clone()).collect();
        let place = quiet_button(ui, "Place Asset…").on_hover_text("Place a reusable object from this project");
        controls.mark("place-asset", place.clone());
        egui::Popup::menu(&place).show(|ui| {
            ui.set_min_width(220.0);
            if objects.is_empty() {
                ui.label(
                    RichText::new("No reusable objects yet. Save a selection as an object first.")
                        .color(colors::muted()),
                );
            }
            for path in &objects {
                let label = path.strip_prefix("assets/").unwrap_or(path);
                let item = ui.button(label);
                if controls.mark(format!("place:{path}"), item).clicked() {
                    level.instance_choice = Some(path.clone());
                    if let Err(error) = level.add_instance(path, access) {
                        level.set_status(error);
                    }
                }
            }
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Outliner
    // -----------------------------------------------------------------------------------------------------------------

    fn outliner(&mut self, ui: &mut Ui, rect: Rect, level: &mut LevelEditor, controls: &mut ControlMap) {
        ui.painter().rect_filled(rect, 0.0, colors::panel());
        widgets::edge(ui, rect.right_top(), rect.right_bottom());
        let total = level.document().level().entities.len();
        let filtered = level.filtered_entities().len();

        // Heading row: "Scene" and the entity count, on a hairline like the shell's panel headings.
        let heading_rect = Rect::from_min_size(rect.min, vec2(rect.width(), PANEL_HEADING_HEIGHT));
        widgets::edge(ui, heading_rect.left_bottom(), heading_rect.right_bottom());
        let count =
            if level.filter.trim().is_empty() { entity_count_label(total) } else { format!("{filtered} of {total}") };
        controls.insert(format!("entity-count:{count}"), Rect::NOTHING);
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(heading_rect.shrink2(vec2(16.0, 0.0)))
                .layout(egui::Layout::left_to_right(Align::Center)),
            |ui| {
                panel_title(ui, "Scene");
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let label = ui.label(RichText::new(&count).size(layout::TYPE_MD).color(colors::muted()));
                    controls.mark("entity-count", label);
                });
            },
        );

        let body = Rect::from_min_max(pos2(rect.min.x, heading_rect.max.y), rect.max).shrink2(vec2(14.0, 14.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(Align::LEFT)), |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let filter = TextEdit::singleline(&mut level.filter)
                .id(Id::new("scene-filter"))
                .hint_text("Filter scene…")
                .font(egui::FontId::proportional(layout::TYPE_MD + 1.0))
                .margin(vec2(10.0, 8.0))
                .desired_width(f32::INFINITY)
                .show(ui)
                .response
                .response;
            controls.mark("scene-filter", filter);
            ui.add_space(4.0);
            let height = (ui.available_height() - 70.0).max(60.0);
            ScrollArea::vertical().id_salt("entities").auto_shrink([false, true]).max_height(height).show(ui, |ui| {
                let rows: Vec<(String, String, String)> = level
                    .filtered_entities()
                    .into_iter()
                    .map(|entity| (entity.id.clone(), entity.name.clone(), level.resolve(entity).kind))
                    .collect();
                let selected = level.document().selected_id().map(str::to_owned);
                let mut choose = None;
                for (id, name, kind) in rows {
                    let active = selected.as_deref() == Some(id.as_str());
                    let (row, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), OUTLINER_ROW_HEIGHT), Sense::click());
                    let painter = ui.painter();
                    if active {
                        painter.rect_filled(row, 0.0, colors::surface());
                        painter.rect_filled(
                            Rect::from_min_size(row.min, vec2(3.0, row.height())),
                            0.0,
                            colors::accent(),
                        );
                    } else if response.hovered() {
                        painter.rect_filled(row, 0.0, colors::surface().gamma_multiply(0.6));
                    }
                    let marker = Rect::from_center_size(pos2(row.min.x + 20.0, row.center().y), vec2(8.0, 8.0));
                    painter.add(egui::Shape::convex_polygon(
                        vec![marker.center_top(), marker.right_center(), marker.center_bottom(), marker.left_center()],
                        colors::muted().gamma_multiply(0.7),
                        Stroke::NONE,
                    ));
                    // The type sits at the right edge when the name leaves room for it; the name truncates otherwise.
                    let label = if name.is_empty() { kind.clone() } else { name.clone() };
                    let kind_galley = painter.layout_no_wrap(
                        kind.clone(),
                        egui::FontId::proportional(layout::TYPE_SM),
                        colors::muted(),
                    );
                    let name_left = row.min.x + 36.0;
                    let kind_space = if name.is_empty() { 0.0 } else { kind_galley.size().x + 16.0 };
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        label,
                        egui::FontId::proportional(layout::TYPE_MD + 1.0),
                        colors::text(),
                    );
                    job.wrap = egui::text::TextWrapping::truncate_at_width(
                        (row.max.x - 10.0 - kind_space - name_left).max(24.0),
                    );
                    let name_galley = painter.layout_job(job);
                    let name_width = name_galley.size().x;
                    painter.galley(
                        pos2(name_left, row.center().y - (name_galley.size().y / 2.0)),
                        name_galley,
                        colors::text(),
                    );
                    if !name.is_empty() && name_left + name_width + 16.0 + kind_galley.size().x <= row.max.x - 10.0 {
                        let kind_position = pos2(
                            row.max.x - 10.0 - kind_galley.size().x,
                            row.center().y - (kind_galley.size().y / 2.0),
                        );
                        painter.galley(kind_position, kind_galley, colors::muted());
                    }
                    controls.mark(format!("entity:{id}"), response.clone());
                    if response.clicked() {
                        choose = Some(id);
                    }
                }
                if let Some(id) = choose {
                    level.select(Some(&id));
                }
            });
            ui.add_space(8.0);
            let hint = if total == 0 {
                "Scene settings can exist before any visible objects are placed."
            } else {
                "Select an entity to edit its transform and components."
            };
            ui.label(RichText::new(hint).size(layout::TYPE_MD).color(colors::muted()));
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Viewport
    // -----------------------------------------------------------------------------------------------------------------

    fn viewport(&mut self, ui: &mut Ui, rect: Rect, level: &mut LevelEditor, controls: &mut ControlMap) {
        let actions = viewport_input(ui, rect, Id::new("level-viewport-input"));
        controls.mark("viewport-input", actions.response.clone());
        if let Some((gesture, delta)) = actions.drag {
            drag_camera(&mut level.viewport.camera, gesture, delta.x, delta.y);
        }
        if actions.zoom != 0.0 {
            zoom_camera(&mut level.viewport.camera, actions.zoom);
        }
        for shortcut in actions.shortcuts {
            match shortcut {
                ViewportShortcut::Frame => {
                    let selected = level.document().selected_id().map(str::to_owned);
                    level.viewport.focus(selected.as_deref());
                }
                ViewportShortcut::Reset => level.viewport.reset_camera(),
                other => orbit_shortcut(&mut level.viewport.camera, other),
            }
        }
        if let Some(position) = actions.click.map(|point| Vec2::new(point.x, point.y)) {
            if level.viewport.tool == LevelTool::Move && level.selected().is_some() {
                if let Some(point) = level.viewport.pick_ground(position) {
                    level.move_to_ground(point);
                }
            } else {
                let picked = level.viewport.pick(position);
                level.select(picked.as_deref());
            }
        }
        if actions.response.has_focus() {
            let outline = Stroke::new(1.0, colors::accent().gamma_multiply(0.6));
            ui.painter().rect_stroke(rect.shrink(1.0), 0.0, outline, egui::StrokeKind::Inside);
        }
        self.gizmo(ui, level, controls);
    }

    /// Draw the move gizmo and let its handles take drags before the viewport does.
    fn gizmo(&mut self, ui: &mut Ui, level: &mut LevelEditor, controls: &mut ControlMap) {
        let lines = level.viewport.gizmo_lines();
        for line in &lines {
            let start = pos2(line.start.x, line.start.y);
            let end = pos2(line.end.x, line.end.y);
            let color = match line.handle {
                GizmoHandle::X => Color32::from_rgb(0xe8, 0x5a, 0x4f),
                GizmoHandle::Y => Color32::from_rgb(0x8f, 0xd1, 0x6a),
                GizmoHandle::Z => Color32::from_rgb(0x5a, 0x9b, 0xe8),
                GizmoHandle::Plane => colors::accent(),
            };
            let mut grips = Vec::new();
            if line.handle == GizmoHandle::Plane {
                let square = Rect::from_two_pos(start, end);
                ui.painter().rect_filled(square, 0.0, color.gamma_multiply(0.35));
                ui.painter().rect_stroke(square, 0.0, Stroke::new(1.5, color), egui::StrokeKind::Inside);
                grips.push(square);
            } else {
                ui.painter().line_segment([start, end], Stroke::new(3.0, color));
                ui.painter().circle_filled(end, 5.0, color);
                grips.push(Rect::from_center_size(end, vec2(18.0, 18.0)));
                grips.push(Rect::from_center_size(start.lerp(end, 0.6), vec2(12.0, 12.0)));
            }
            controls.insert(format!("gizmo:{:?}", line.handle), grips[0]);
            for (index, grip) in grips.into_iter().enumerate() {
                let response = ui.interact(grip, Id::new(("gizmo", line.handle as u8, index)), Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
                }
                let pointer = response.interact_pointer_pos().map(|point| Vec2::new(point.x, point.y));
                if response.drag_started()
                    && let Some(point) = pointer
                {
                    level.viewport.begin_gizmo(line.handle, point);
                }
                if response.dragged()
                    && let Some(point) = pointer
                {
                    level.viewport.drag_gizmo(point);
                }
                if response.drag_stopped()
                    && let Some((entity, position)) = level.viewport.end_gizmo()
                {
                    level.move_entity(&entity, position);
                }
            }
        }
    }

    fn viewport_overlay(&mut self, ui: &mut Ui, rect: Rect, level: &mut LevelEditor, controls: &mut ControlMap) {
        // Top chips: Frame on the left, View settings on the right.
        let bar = rect.shrink2(vec2(20.0, 20.0));
        let bar = Rect::from_min_size(bar.min, vec2(bar.width().max(0.0), 36.0));
        let mut settings_anchor = None;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let frame = chip_button(ui, "Frame  F", false).on_hover_text("Frame the selection, or the whole scene");
                if controls.mark("focus", frame).clicked() {
                    let selected = level.document().selected_id().map(str::to_owned);
                    level.viewport.focus(selected.as_deref());
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let settings = chip_button(ui, "View settings", self.view_settings_open);
                    if controls.mark("view-settings", settings.clone()).clicked() {
                        self.view_settings_open = !self.view_settings_open;
                    }
                    settings_anchor = Some(settings.rect);
                });
            },
        );
        if self.view_settings_open
            && let Some(anchor) = settings_anchor
        {
            self.view_settings(ui, rect, anchor, level, controls);
        }

        // Bottom hints: camera gestures on the left, the world axes on the right.
        let corner =
            Rect::from_min_size(pos2(rect.min.x + 20.0, rect.max.y - 34.0), vec2((rect.width() - 40.0).max(0.0), 24.0));
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(corner).layout(egui::Layout::left_to_right(Align::Center)),
            |ui| {
                let hint = egui::FontId::monospace(layout::TYPE_SM + 1.0);
                ui.label(
                    RichText::new("Drag orbit · Shift drag pan · Scroll zoom")
                        .font(hint.clone())
                        .color(colors::muted()),
                );
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    for axis in ["Z ↙", "X →", "Y ↑"] {
                        ui.label(RichText::new(axis).font(hint.clone()).color(colors::slider_edge()));
                    }
                });
            },
        );
    }

    /// The View settings popover: editor grid, camera reset, and which panels show.
    fn view_settings(
        &mut self,
        ui: &mut Ui,
        viewport: Rect,
        anchor: Rect,
        level: &mut LevelEditor,
        controls: &mut ControlMap,
    ) {
        let top_left = pos2(anchor.max.x - VIEW_SETTINGS_WIDTH, anchor.max.y + 8.0);
        let area =
            Rect::from_min_size(top_left, vec2(VIEW_SETTINGS_WIDTH, (viewport.max.y - top_left.y - 50.0).max(0.0)));
        ui.scope_builder(egui::UiBuilder::new().max_rect(area).layout(egui::Layout::top_down(Align::LEFT)), |ui| {
            let frame = egui::Frame::new()
                .fill(colors::header())
                .stroke(Stroke::new(1.0, colors::line()))
                .corner_radius(layout::RADIUS_SM)
                .inner_margin(12.0);
            let shown = frame.show(ui, |ui| {
                ui.set_width(VIEW_SETTINGS_WIDTH - 26.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                section_label(ui, "VIEWPORT");
                let grid = ui
                    .checkbox(&mut level.viewport.show_grid, RichText::new("Editor grid").size(layout::TYPE_MD))
                    .on_hover_text(
                        "Editor-only lines; minor spacing follows 1/2/5 meters, major lines mark five divisions",
                    );
                controls.mark("grid-toggle", grid);
                let reset = ui.button("Reset camera  0").on_hover_text("Reset camera (0)");
                if controls.mark("reset-camera", reset).clicked() {
                    level.viewport.reset_camera();
                }
                ui.add_space(4.0);
                section_label(ui, "PANELS");
                ui.horizontal_wrapped(|ui| self.panels.visibility_controls(ui));
            });
            controls.mark("view-settings-panel", shown.response);
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Inspector
    // -----------------------------------------------------------------------------------------------------------------

    fn inspector(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        level: &mut LevelEditor,
        access: &mut dyn AssetAccess,
        controls: &mut ControlMap,
    ) {
        ui.painter().rect_filled(rect, 0.0, colors::panel());
        widgets::edge(ui, rect.left_top(), rect.left_bottom());
        let heading_rect = Rect::from_min_size(rect.min, vec2(rect.width(), PANEL_HEADING_HEIGHT));
        widgets::edge(ui, heading_rect.left_bottom(), heading_rect.right_bottom());
        let title = if level.selected().is_some() { "Inspector" } else { "Level settings" };
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(heading_rect.shrink2(vec2(16.0, 0.0)))
                .layout(egui::Layout::left_to_right(Align::Center)),
            |ui| panel_title(ui, title),
        );
        let body = Rect::from_min_max(pos2(rect.min.x, heading_rect.max.y), rect.max).shrink2(vec2(14.0, 12.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(Align::LEFT)), |ui| {
            ScrollArea::vertical().id_salt("inspector").auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                let Some(selected) = level.selected().cloned() else {
                    self.level_settings(ui, level, controls);
                    return;
                };
                let resolved = level.resolve(&selected);
                let width = ui.available_width();
                for (key, label, value, hint) in [
                    (FieldKey::EntityName, "NAME", resolved.name.clone(), ""),
                    (FieldKey::EntityType, "TYPE", resolved.kind.clone(), ""),
                    (FieldKey::EntityTags, "TAGS", resolved.tags.join(", "), "comma, separated"),
                ] {
                    caption(ui, label, colors::muted());
                    let draft = level.drafts().get(&key).cloned();
                    let name = format!("{key:?}");
                    let (response, event) =
                        draft_field(ui, Id::new(("inspector", &name)), &value, draft.as_ref(), width, false, hint);
                    controls.mark(name, response);
                    apply_text_event(level, key, event);
                }

                if let Some(template) = &selected.template {
                    ui.add_space(4.0);
                    egui::Frame::new().fill(colors::surface()).inner_margin(8.0).show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        caption(ui, "LINKED OBJECT", colors::accent());
                        ui.label(RichText::new(template).monospace().size(layout::TYPE_XS));
                        let groups = selected.overrides.as_ref().map_or(0, |overrides| overrides.len());
                        ui.label(
                            RichText::new(format!("{groups} override groups"))
                                .size(layout::TYPE_XS)
                                .color(colors::muted()),
                        );
                        ui.horizontal_wrapped(|ui| {
                            if controls.mark("edit-object", button(ui, "EDIT SOURCE ↗")).clicked() {
                                level.open_source(template);
                            }
                            if controls.mark("reset-overrides", button(ui, "RESET OVERRIDES")).clicked() {
                                level.reset_overrides();
                            }
                            if controls.mark("detach-object", button(ui, "DETACH")).clicked() {
                                level.detach();
                            }
                        });
                    });
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    heading(ui, "TRANSFORM");
                    caption(ui, "WORLD SPACE", colors::muted());
                });
                for field in TransformField::ALL {
                    caption(ui, field.label(), colors::muted());
                    ui.horizontal(|ui| {
                        let channel_width = ((width / 3.0) - 6.0).max(40.0);
                        for (axis, axis_label) in ["X", "Y", "Z"].iter().enumerate() {
                            ui.label(RichText::new(*axis_label).size(layout::TYPE_XS).color(colors::muted()));
                            let key = FieldKey::Transform(field, axis);
                            let current = level.transform_value(field, axis).map(f64::from).unwrap_or(0.0);
                            let draft = level.drafts().get(&key).cloned();
                            let id = Id::new(("transform", field, axis));
                            let (response, event) = draft_field(
                                ui,
                                id,
                                &display_number(current),
                                draft.as_ref(),
                                channel_width - 14.0,
                                true,
                                "",
                            );
                            controls.mark(format!("{}-{axis}", transform_name(field)), response);
                            apply_number_event(level, key, event, transform_bounds(field), current);
                        }
                    });
                }

                ui.add_space(6.0);
                heading(ui, "COMPONENTS");
                self.component_menu(ui, level, access, controls);
                self.properties(ui, level, access, controls);

                ui.add_space(8.0);
                let source = egui::CollapsingHeader::new(RichText::new("ADVANCED SOURCE").size(layout::TYPE_SM))
                    .id_salt("advanced-source")
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Optional component source and repair view.")
                                .size(layout::TYPE_XS)
                                .color(colors::muted()),
                        );
                        let editor = ui
                            .allocate_ui(vec2(ui.available_width(), 220.0), |ui| {
                                self.source_editor.show(ui, &mut level.components_source)
                            })
                            .inner;
                        controls.mark("components", editor.response);
                        if controls.mark("apply-components", button(ui, "APPLY SOURCE")).clicked() {
                            match level.apply_components() {
                                Ok(()) => level.set_status("Components applied."),
                                Err(error) => level.set_status(error),
                            }
                        }
                    });
                controls.mark("advanced-source", source.header_response);
            });
        });
    }

    /// The inspector with nothing selected: the level document, its scene, and viewport-only previews.
    fn level_settings(&mut self, ui: &mut Ui, level: &mut LevelEditor, controls: &mut ControlMap) {
        section_label(ui, "DOCUMENT");
        ui.label(RichText::new("Level name").size(layout::TYPE_MD + 1.0).color(colors::text()));
        let name = level.document().level().name.clone();
        let key = FieldKey::LevelName;
        let draft = level.drafts().get(&key).cloned();
        let (response, event) =
            draft_field(ui, Id::new("level-name"), &name, draft.as_ref(), ui.available_width(), false, "");
        controls.mark("level-name", response);
        apply_text_event(level, key, event);
        ui.label(RichText::new("Y-up · World units in meters").size(layout::TYPE_MD).color(colors::muted()));
        ui.add_space(10.0);
        ui.separator();

        section_label(ui, "SCENE");
        let entities = level.document().level().entities.len();
        ui.label(
            RichText::new(format!(
                "{}. Authored scene settings are saved in this level.",
                entity_count_label(entities)
            ))
            .size(layout::TYPE_MD)
            .color(colors::muted()),
        );
        ui.add_space(10.0);
        ui.separator();

        section_label(ui, "VIEWPORT ONLY");
        let effects = ui.scope(|ui| level.viewport.effects.ui(ui));
        controls.mark("effects", effects.response);
        ui.label(
            RichText::new("Preview enablement does not change the saved scene.")
                .size(layout::TYPE_MD)
                .color(colors::muted()),
        );
    }

    fn component_menu(
        &mut self,
        ui: &mut Ui,
        level: &mut LevelEditor,
        access: &mut dyn AssetAccess,
        controls: &mut ControlMap,
    ) {
        let label = match &level.component_choice {
            ComponentChoice::Custom => "Custom Properties".to_owned(),
            ComponentChoice::Preset(id) => level
                .schemas()
                .list()
                .flat_map(|schema| schema.presets.iter())
                .find(|preset| preset.id == *id)
                .map_or_else(|| id.clone(), |preset| preset.label.clone()),
        };
        ui.horizontal(|ui| {
            let mut choice = level.component_choice.clone();
            let width = (ui.available_width() - 70.0).max(80.0);
            let combo =
                egui::ComboBox::from_id_salt("component-kind").selected_text(label).width(width).show_ui(ui, |ui| {
                    let mut category = String::new();
                    for schema in level.schemas().list() {
                        if schema.category != category {
                            category = schema.category.clone();
                            ui.label(RichText::new(&category).size(layout::TYPE_XS).color(colors::muted()));
                        }
                        for preset in &schema.presets {
                            let selected = choice == ComponentChoice::Preset(preset.id.clone());
                            let option = ui
                                .selectable_label(selected, format!("  {}", preset.label))
                                .on_hover_text(&schema.description);
                            if option.clicked() {
                                choice = ComponentChoice::Preset(preset.id.clone());
                            }
                        }
                    }
                    ui.label(RichText::new("Custom").size(layout::TYPE_XS).color(colors::muted()));
                    if ui.selectable_label(choice == ComponentChoice::Custom, "  Custom Properties").clicked() {
                        choice = ComponentChoice::Custom;
                    }
                });
            controls.mark("component-kind", combo.response);
            level.component_choice = choice;
            if controls.mark("add-component", primary_button(ui, "+ ADD")).clicked() {
                if level.component_choice == ComponentChoice::Custom {
                    self.dialog = Some(LevelDialog::CustomComponent(DEFAULT_CUSTOM_COMPONENT.to_owned()));
                } else {
                    let choice = level.component_choice.clone();
                    let catalog = access.catalog().to_vec();
                    level.add_component(&choice, None, &catalog);
                }
            }
        });
    }

    fn properties(
        &mut self,
        ui: &mut Ui,
        level: &mut LevelEditor,
        access: &mut dyn AssetAccess,
        controls: &mut ControlMap,
    ) {
        let components = level.display_components();
        let issues = level.selected_issues();
        let metadata = level.parameter_metadata().clone();
        let catalog = access.catalog().to_vec();
        let nodes = {
            let context = PropertyContext {
                schemas: level.schemas(),
                issues: &issues,
                metadata: &metadata,
                assets: &catalog,
            };
            build_property_tree(&components, &context)
        };
        let mut actions = Vec::new();
        for node in &nodes {
            self.node(ui, node, level, &components, &mut actions, controls);
        }
        for action in actions {
            let result = match action {
                PropertyAction::Set(path, value) => level.change_property(&path, Some(value)),
                PropertyAction::Remove(path) => level.change_property(&path, None),
                PropertyAction::Open(path) => {
                    level.open_source(&path);
                    Ok(())
                }
            };
            if let Err(error) = result {
                level.set_status(error);
            }
        }
    }

    fn node(
        &mut self,
        ui: &mut Ui,
        node: &PropertyNode,
        level: &mut LevelEditor,
        components: &Map<String, Value>,
        actions: &mut Vec<PropertyAction>,
        controls: &mut ControlMap,
    ) {
        match node {
            PropertyNode::Note(text) => {
                ui.label(RichText::new(text).size(layout::TYPE_XS).color(colors::muted()));
            }
            PropertyNode::Group(group) => self.group(ui, group, level, components, actions, controls),
            PropertyNode::Field(field) => {
                let key = field.path.join(".");
                ui.scope(|ui| {
                    if field.inactive.is_some() {
                        ui.multiply_opacity(0.45);
                    }
                    let label_color = if field.issue.is_some() { colors::danger() } else { colors::muted() };
                    let tooltip =
                        field.issue.clone().or_else(|| field.inactive.clone()).or_else(|| field.description.clone());
                    let label =
                        ui.label(RichText::new(field.label.to_uppercase()).size(layout::TYPE_XS).color(label_color));
                    if let Some(tooltip) = tooltip {
                        label.on_hover_text(tooltip);
                    }
                    let removed = removable_row(ui, controls, &key, |ui, controls| {
                        let width = ui.available_width();
                        match &field.control {
                            FieldControl::Boolean => {
                                let mut checked = field.value.as_bool().unwrap_or(false);
                                let checkbox = ui.checkbox(&mut checked, "");
                                if controls.mark(format!("property:{key}"), checkbox).changed() {
                                    actions.push(PropertyAction::Set(field.path.clone(), Value::Bool(checked)));
                                }
                            }
                            FieldControl::Choice(choices) => {
                                let current = display_string(&field.value);
                                let mut chosen = current.clone();
                                let combo = egui::ComboBox::from_id_salt(("choice", &key))
                                    .selected_text(&current)
                                    .width(width)
                                    .show_ui(ui, |ui| {
                                        for choice in choices {
                                            ui.selectable_value(&mut chosen, choice.clone(), choice);
                                        }
                                    });
                                controls.mark(format!("property:{key}"), combo.response);
                                if chosen != current {
                                    actions.push(PropertyAction::Set(field.path.clone(), Value::from(chosen)));
                                }
                            }
                            FieldControl::Asset(options) => {
                                let current = field.value.as_str().unwrap_or_default().to_owned();
                                let label = options
                                    .iter()
                                    .find(|(value, _)| *value == current)
                                    .map_or(current.clone(), |(_, label)| label.clone());
                                let mut chosen = current.clone();
                                let combo = egui::ComboBox::from_id_salt(("asset", &key))
                                    .selected_text(label)
                                    .width((width - 120.0).max(60.0))
                                    .show_ui(ui, |ui| {
                                        for (value, label) in options {
                                            ui.selectable_value(&mut chosen, value.clone(), label);
                                        }
                                    });
                                controls.mark(format!("property:{key}"), combo.response);
                                if chosen != current {
                                    actions.push(PropertyAction::Set(field.path.clone(), Value::from(chosen)));
                                }
                                let edit =
                                    ui.add_enabled_ui(!current.is_empty(), |ui| button(ui, "EDIT SOURCE ↗")).inner;
                                if controls.mark(format!("edit-source:{key}"), edit).clicked() {
                                    actions.push(PropertyAction::Open(current.clone()));
                                }
                            }
                            FieldControl::Indexed(options) => {
                                let last = options.len().saturating_sub(1);
                                let current = (field.value.as_f64().unwrap_or(0.0).round().max(0.0) as usize).min(last);
                                let mut chosen = current;
                                let combo = egui::ComboBox::from_id_salt(("indexed", &key))
                                    .selected_text(options.get(current).cloned().unwrap_or_default())
                                    .width(width)
                                    .show_ui(ui, |ui| {
                                        for (index, option) in options.iter().enumerate() {
                                            ui.selectable_value(&mut chosen, index, option);
                                        }
                                    });
                                controls.mark(format!("property:{key}"), combo.response);
                                if chosen != current {
                                    actions.push(PropertyAction::Set(field.path.clone(), Value::from(chosen)));
                                }
                            }
                            FieldControl::Seed(bounds) => {
                                let draft_key = FieldKey::Property(field.path.clone());
                                let draft = level.drafts().get(&draft_key).cloned();
                                let current = field.value.as_f64().unwrap_or(0.0);
                                let reroll_width = SEED_REROLL_WIDTH + ui.spacing().item_spacing.x;
                                let (response, event) = draft_field(
                                    ui,
                                    Id::new(("property", &key)),
                                    &format_number(current),
                                    draft.as_ref(),
                                    (width - reroll_width).max(60.0),
                                    true,
                                    "",
                                );
                                controls.mark(format!("property:{key}"), response);
                                apply_number_event(level, draft_key, event, *bounds, current);
                                let reroll = ui
                                    .add_sized(
                                        [SEED_REROLL_WIDTH, ui.spacing().interact_size.y],
                                        egui::Button::new("⟳"),
                                    )
                                    .on_hover_text("Reroll the seed");
                                if controls.mark(format!("reroll:{key}"), reroll).clicked() {
                                    let minimum = bounds.min.unwrap_or(0.0);
                                    let maximum = bounds.max.unwrap_or(SEED_FALLBACK_MAXIMUM);
                                    let next = rerolled_seed(current, minimum, maximum);
                                    actions.push(PropertyAction::Set(field.path.clone(), json_number(next)));
                                }
                            }
                            FieldControl::Number(bounds) => {
                                let draft_key = FieldKey::Property(field.path.clone());
                                let draft = level.drafts().get(&draft_key).cloned();
                                let current = field.value.as_f64().unwrap_or(0.0);
                                let (response, event) = draft_field(
                                    ui,
                                    Id::new(("property", &key)),
                                    &format_number(current),
                                    draft.as_ref(),
                                    width,
                                    true,
                                    "",
                                );
                                controls.mark(format!("property:{key}"), response);
                                apply_number_event(level, draft_key, event, *bounds, current);
                            }
                            FieldControl::Text {
                                placeholder,
                            } => {
                                let draft_key = FieldKey::Property(field.path.clone());
                                let draft = level.drafts().get(&draft_key).cloned();
                                let current = field.value.as_str().unwrap_or_default().to_owned();
                                let hint = placeholder.as_deref().unwrap_or("");
                                let (response, event) = draft_field(
                                    ui,
                                    Id::new(("property", &key)),
                                    &current,
                                    draft.as_ref(),
                                    width,
                                    false,
                                    hint,
                                );
                                controls.mark(format!("property:{key}"), response);
                                apply_text_event(level, draft_key, event);
                            }
                        }
                    });
                    if removed {
                        actions.push(PropertyAction::Remove(field.path.clone()));
                    }
                });
            }
            PropertyNode::Vector(vector) => {
                let key = vector.path.join(".");
                ui.scope(|ui| {
                    if vector.inactive.is_some() {
                        ui.multiply_opacity(0.45);
                    }
                    let color = if vector.issue.is_some() { colors::danger() } else { colors::muted() };
                    let label = ui.label(RichText::new(&vector.label).size(layout::TYPE_XS).color(color));
                    if let Some(tooltip) =
                        vector.issue.clone().or_else(|| vector.inactive.clone()).or_else(|| vector.description.clone())
                    {
                        label.on_hover_text(tooltip);
                    }
                    let removed = removable_row(ui, controls, &key, |ui, controls| {
                        let count = vector.values.len().max(1) as f32;
                        let channel_width = ((ui.available_width() / count) - 20.0).max(30.0);
                        for (index, channel) in vector.channels.iter().enumerate() {
                            ui.label(RichText::new(*channel).size(layout::TYPE_XS).color(colors::muted()));
                            let draft_key = FieldKey::PropertyChannel(vector.path.clone(), index);
                            let draft = level.drafts().get(&draft_key).cloned();
                            let value = vector.values[index];
                            let id = Id::new(("vector", &key, index));
                            let (response, event) =
                                draft_field(ui, id, &format_number(value), draft.as_ref(), channel_width, true, "");
                            controls.mark(format!("property:{key}.{index}"), response);
                            apply_number_event(level, draft_key, event, vector.bounds, value);
                        }
                    });
                    if removed {
                        actions.push(PropertyAction::Remove(vector.path.clone()));
                    }
                    if vector.color {
                        let swatch = joy_engine::development::lab_values::display_color(&vector.values);
                        let (rect, _) =
                            ui.allocate_exact_size(vec2(ui.available_width().min(120.0), 6.0), Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, swatch);
                    }
                });
            }
        }
    }

    fn group(
        &mut self,
        ui: &mut Ui,
        group: &PropertyGroup,
        level: &mut LevelEditor,
        components: &Map<String, Value>,
        actions: &mut Vec<PropertyAction>,
        controls: &mut ControlMap,
    ) {
        let key = group.path.join(".");
        let mut title = RichText::new(group.label.to_uppercase()).size(layout::TYPE_SM).strong();
        if !group.issues.is_empty() {
            title = title.color(colors::danger());
        }
        let fill = if group.component { colors::header() } else { Color32::TRANSPARENT };
        let stroke = Stroke::new(if group.component { 1.0 } else { 0.0 }, colors::line());
        let frame = egui::Frame::new().fill(fill).stroke(stroke).inner_margin(if group.component { 8.0 } else { 2.0 });
        frame.show(ui, |ui| {
            let header =
                egui::CollapsingHeader::new(title).id_salt(("group", &key)).default_open(group.open).show(ui, |ui| {
                    if let Some(type_name) = &group.type_name {
                        ui.label(RichText::new(type_name).size(layout::TYPE_XS).color(colors::muted()));
                    }
                    if let Some(description) = &group.description {
                        ui.label(RichText::new(description).size(layout::TYPE_XS).color(colors::muted()));
                    }
                    for issue in &group.issues {
                        let label =
                            ui.label(RichText::new(format!("• {issue}")).size(layout::TYPE_XS).color(colors::danger()));
                        controls.mark(format!("issue:{key}:{issue}"), label);
                    }
                    for child in &group.children {
                        self.node(ui, child, level, components, actions, controls);
                    }
                    if !group.missing_fields.is_empty() {
                        ui.horizontal(|ui| {
                            let choice = self
                                .missing_choice
                                .entry(key.clone())
                                .or_insert_with(|| group.missing_fields[0].0.clone());
                            if !group.missing_fields.iter().any(|(name, _)| name == choice) {
                                *choice = group.missing_fields[0].0.clone();
                            }
                            let label = group
                                .missing_fields
                                .iter()
                                .find(|(name, _)| name == choice)
                                .map_or(choice.clone(), |(_, label)| label.clone());
                            let width = (ui.available_width() - 90.0).max(60.0);
                            let combo = egui::ComboBox::from_id_salt(("missing", &key))
                                .selected_text(label)
                                .width(width)
                                .show_ui(ui, |ui| {
                                    for (name, label) in &group.missing_fields {
                                        ui.selectable_value(choice, name.clone(), label);
                                    }
                                });
                            controls.mark(format!("missing:{key}"), combo.response);
                            let chosen = choice.clone();
                            if controls.mark(format!("add-field:{key}"), button(ui, "+ FIELD")).clicked() {
                                let rest: Vec<&str> = group.path[1..]
                                    .iter()
                                    .map(String::as_str)
                                    .chain(std::iter::once(chosen.as_str()))
                                    .collect();
                                if let Some(schema) = level.schemas().field(&group.path[0], &rest) {
                                    let mut path = group.path.clone();
                                    path.push(chosen.clone());
                                    actions.push(PropertyAction::Set(path, initial_field_value(schema)));
                                }
                            }
                        });
                    }
                    let current =
                        value_at(&Value::Object(components.clone()), &group.path).cloned().unwrap_or(Value::Null);
                    let removed = removable_row(ui, controls, &key, |ui, controls| {
                        if group.list {
                            if controls.mark(format!("add-item:{key}"), button(ui, "+ ITEM")).clicked()
                                && let Some(next) = append_list_item(&current)
                            {
                                actions.push(PropertyAction::Set(group.path.clone(), next));
                            }
                        } else {
                            let entry = self
                                .custom_fields
                                .entry(key.clone())
                                .or_insert_with(|| (String::new(), CustomFieldKind::Number));
                            let name = TextEdit::singleline(&mut entry.0)
                                .id(Id::new(("custom-field", &key)))
                                .hint_text("Field")
                                .desired_width((ui.available_width() - 150.0).max(60.0))
                                .show(ui)
                                .response
                                .response;
                            controls.mark(format!("custom-name:{key}"), name);
                            let combo = egui::ComboBox::from_id_salt(("custom-kind", &key))
                                .selected_text(entry.1.label())
                                .width(70.0)
                                .show_ui(ui, |ui| {
                                    for kind in CustomFieldKind::ALL {
                                        ui.selectable_value(&mut entry.1, kind, kind.label());
                                    }
                                });
                            controls.mark(format!("custom-kind:{key}"), combo.response);
                            if controls.mark(format!("add-custom:{key}"), button(ui, "+ ADD")).clicked() {
                                match add_custom_field(&current, &entry.0, entry.1) {
                                    Ok(next) => {
                                        actions.push(PropertyAction::Set(group.path.clone(), next));
                                        entry.0.clear();
                                    }
                                    Err(error) => level.set_status(error),
                                }
                            }
                        }
                    });
                    if removed {
                        actions.push(PropertyAction::Remove(group.path.clone()));
                    }
                });
            controls.mark(format!("group:{key}"), header.header_response);
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Dialogs and shortcuts
    // -----------------------------------------------------------------------------------------------------------------

    fn dialogs(
        &mut self,
        context: &egui::Context,
        level: &mut LevelEditor,
        access: &mut dyn AssetAccess,
        controls: &mut ControlMap,
    ) {
        let Some(mut dialog) = self.dialog.clone() else {
            return;
        };
        let mut next: Option<Option<LevelDialog>> = None;
        let modal = egui::Modal::new(Id::new("level-dialog")).frame(dialog_frame()).show(context, |ui| {
            ui.set_max_width(420.0);
            let (prompt, text) = match &mut dialog {
                LevelDialog::SaveObject(text) => ("Reusable object path beneath assets/:", text),
                LevelDialog::CustomComponent(text) => ("Component name:", text),
            };
            ui.label(RichText::new(prompt).size(layout::TYPE_MD));
            let field = TextEdit::singleline(text)
                .id(Id::new("level-dialog-field"))
                .desired_width(380.0)
                .show(ui)
                .response
                .response;
            if ui.memory(|memory| memory.focused().is_none()) {
                field.request_focus();
            }
            let submitted = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
            controls.mark("level-dialog-field", field);
            ui.horizontal(|ui| {
                if controls.mark("level-dialog-confirm", primary_button(ui, "OK")).clicked() || submitted {
                    next = Some(None);
                    match &dialog {
                        LevelDialog::SaveObject(path) => {
                            if let Err(error) = level.save_object(path, access) {
                                level.set_status(error);
                            }
                        }
                        LevelDialog::CustomComponent(name) => {
                            let catalog = access.catalog().to_vec();
                            level.add_component(&ComponentChoice::Custom, Some(name), &catalog);
                        }
                    }
                }
                if controls.mark("level-dialog-cancel", button(ui, "Cancel")).clicked() {
                    next = Some(None);
                }
            });
        });
        if next.is_none() && modal.should_close() {
            next = Some(None);
        }
        self.dialog = match next {
            Some(replacement) => replacement,
            None => Some(dialog),
        };
    }

    fn shortcuts(&mut self, context: &egui::Context, level: &mut LevelEditor) {
        if self.dialog.is_some() || context.egui_wants_keyboard_input() {
            return;
        }
        let undo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        let redo = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        if context.input_mut(|input| input.consume_shortcut(&redo)) {
            level.history(true);
        } else if context.input_mut(|input| input.consume_shortcut(&undo)) {
            level.history(false);
        }
        let delete = context.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Delete) || input.consume_key(Modifiers::NONE, Key::Backspace)
        });
        if delete && !level.object_mode() {
            level.remove_selected();
        }
        if context.input(|input| input.key_pressed(Key::Escape)) && level.viewport.dragging() {
            level.viewport.cancel_gizmo();
        }
    }
}

/// A fixed-height row with a remove button (`remove:<key>`) at its right edge; `add_contents` fills the rest from the
/// left. Returns whether the remove button was clicked.
fn removable_row(
    ui: &mut Ui,
    controls: &mut ControlMap,
    key: &str,
    add_contents: impl FnOnce(&mut Ui, &mut ControlMap),
) -> bool {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(vec2(width, ROW_HEIGHT), egui::Layout::right_to_left(Align::Center), |ui| {
        let remove = button(ui, "−").on_hover_text(format!("Remove {key}"));
        let removed = controls.mark(format!("remove:{key}"), remove).clicked();
        ui.with_layout(egui::Layout::left_to_right(Align::Center), |ui| add_contents(ui, controls));
        removed
    })
    .inner
}

/// The control name of a transform field (`position`, `rotation`, `scale`).
fn transform_name(field: TransformField) -> &'static str {
    match field {
        TransformField::Position => "position",
        TransformField::Rotation => "rotation",
        TransformField::Scale => "scale",
    }
}

/// Record or commit typed text for a text field.
fn apply_text_event(level: &mut LevelEditor, key: FieldKey, event: widgets::FieldEvent) {
    if let Some(text) = event.changed {
        level.drafts_mut().set(key.clone(), text);
    }
    if event.revert {
        level.drafts_mut().discard(&key);
    } else if event.commit {
        let _ = level.commit_draft(&key);
    }
}

/// Record, step, or commit typed text for a numeric field.
fn apply_number_event(
    level: &mut LevelEditor,
    key: FieldKey,
    event: widgets::FieldEvent,
    bounds: NumberBounds,
    current: f64,
) {
    if let Some(text) = event.changed {
        level.drafts_mut().set_number(key.clone(), text, bounds);
    }
    if let Some((direction, scale)) = event.step {
        let base =
            level.drafts().get(&key).and_then(|draft| parse_number(&draft.text, &bounds).ok()).unwrap_or(current);
        let next = step_number(base, &bounds, direction, scale);
        level.drafts_mut().set_number(key.clone(), format_number(next), bounds);
        let _ = level.commit_draft(&key);
    }
    if event.revert {
        level.drafts_mut().discard(&key);
    } else if event.commit {
        let _ = level.commit_draft(&key);
    }
}

/// Height of a pane's heading row (`Scene`, `Level settings`).
const PANEL_HEADING_HEIGHT: f32 = 56.0;

/// A pane's heading in the shell's sentence-case title face.
fn panel_title(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(egui::FontId::new(layout::TYPE_LG + 1.0, text_styles::strong_family()))
            .color(colors::text()),
    );
}

/// A small spaced capital section label (`DOCUMENT`, `VIEWPORT ONLY`).
fn section_label(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    caption(ui, text, colors::muted());
}

/// `1 entity` or `N entities`.
fn entity_count_label(count: usize) -> String {
    if count == 1 { "1 entity".to_owned() } else { format!("{count} entities") }
}

/// A sentence-case tool toggle for the toolbar: accent text while selected.
fn quiet_toggle(ui: &mut Ui, label: &str, selected: bool) -> egui::Response {
    let response = quiet_button(ui, label);
    if selected {
        let underline = Rect::from_min_max(
            pos2(response.rect.min.x + 10.0, response.rect.max.y - 2.0),
            pos2(response.rect.max.x - 10.0, response.rect.max.y),
        );
        ui.painter().rect_filled(underline, 0.0, colors::accent());
    }
    response
}
