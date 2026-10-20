// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The workspace's egui presentation: the header with
//! the project picker and Save File, the Content Browser (search, type filter, refresh, New, Import, rows with rename
//! and delete, Project Details, Documentation), document tabs with conflict and return bars, repair views, dialogs,
//! and the status footer. The active tab's editor fills the remaining region: the level editor draws there through
//! [`LevelUi`]; a hosted tool receives the region as its tab area.

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
    StrokeKind,
    TextEdit,
    Ui,
    pos2,
    vec2,
};
use std::collections::HashMap;

use joy_engine::development::{
    FileDialogEvent,
    FileDialogs,
    FileFilter,
    OpenRequest,
    ScriptEditor,
    ScriptSyntax,
};
use joy_engine::runtime::Storage;
use joy_engine::runtime::hud_text::{
    self,
    Style,
};

use super::asset_types::{
    DEFAULT_NEW_ASSET_KIND,
    NEW_ASSET_KINDS,
    new_asset_hint,
};
use super::constants::{
    CONTENT_BROWSER_MAXIMUM_WIDTH,
    CONTENT_BROWSER_MINIMUM_WIDTH,
    DIVIDER_KEY_STEP,
};
use super::main::{
    Confirmation,
    Outcome,
    SourceView,
    TabContent,
    Workspace,
};
use crate::interaction::widgets::{
    self,
    ControlMap,
    Icon,
    badge,
    button,
    caption,
    danger_button,
    dialog_frame,
    icon_button,
    palette,
    primary_button,
    region,
    toggle_button,
};
use crate::level::ui::LevelUi;

/// Header height in logical points.
const HEADER_HEIGHT: f32 = 52.0;
/// Footer height.
const FOOTER_HEIGHT: f32 = 28.0;
/// Tab strip height.
const TABS_HEIGHT: f32 = 36.0;
/// Return and conflict bar height.
const BAR_HEIGHT: f32 = 34.0;

/// Asset type filters: value and label.
const FILTERS: [(&str, &str); 4] =
    [("form", "FORM"), ("joyfx", "Effects"), ("joylevel", "Levels"), ("joyobject", "Objects")];

/// A modal dialog.
#[derive(Clone, Debug, PartialEq)]
enum Dialog {
    /// Confirm discarding work.
    Confirm(Confirmation),
    /// Rename an asset; the text is the new name beneath `assets/`.
    Rename { path: String, text: String },
    /// Open a project folder by path.
    OpenFolder { text: String },
    /// The workspace guide.
    Documentation,
}

/// The inline New form.
#[derive(Clone, Debug, PartialEq, Eq)]
struct NewAssetForm {
    kind: String,
    name: String,
    focus: bool,
}

/// Commands the editor shell performs after the UI pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewCommands {
    /// The author confirmed quitting with unsaved edits.
    pub quit: bool,
}

/// Owns workspace UI state: dialogs, the New form, popovers, focus requests, and the level editor's UI.
pub struct WorkspaceView {
    /// Named control rectangles from the latest frame.
    pub controls: ControlMap,
    /// The level editor's UI state.
    pub level: LevelUi,
    /// The region the active tab's editor fills, in logical points.
    pub document_rect: Rect,
    /// Import's file picker; tests deliver files through it.
    pub file_dialogs: FileDialogs,
    source_editors: HashMap<String, ScriptEditor>,
    dialog: Option<Dialog>,
    new_asset: Option<NewAssetForm>,
    filters_open: bool,
    asset_menu: Option<(String, egui::Pos2)>,
    focus_search: bool,
    project_details_open: bool,
    commands: ViewCommands,
}

impl WorkspaceView {
    /// Fresh UI state; editor panel widths persist in `storage`.
    pub fn new(storage: Option<Storage>) -> Self {
        Self {
            controls: ControlMap::default(),
            level: LevelUi::new(storage),
            document_rect: Rect::NOTHING,
            file_dialogs: FileDialogs::new(),
            source_editors: HashMap::new(),
            dialog: None,
            new_asset: None,
            filters_open: false,
            asset_menu: None,
            focus_search: false,
            project_details_open: false,
            commands: ViewCommands::default(),
        }
    }

    /// Ask the author to confirm quitting with unsaved edits.
    pub fn confirm_quit(&mut self) {
        self.dialog = Some(Dialog::Confirm(Confirmation::Quit));
    }

    /// Whether a modal dialog is open.
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    /// Commands collected during the last pass.
    pub fn take_commands(&mut self) -> ViewCommands {
        std::mem::take(&mut self.commands)
    }

    /// Draw the workspace chrome and the active editor's panels for one frame.
    pub fn show(&mut self, context: &egui::Context, workspace: &mut Workspace) {
        self.controls.clear();
        self.poll_dialogs(workspace);
        self.shortcuts(context, workspace);

        let screen = context.content_rect();
        let mut layout = workspace.layout();
        let header = Rect::from_min_max(screen.min, pos2(screen.max.x, screen.min.y + HEADER_HEIGHT));
        let footer = Rect::from_min_max(pos2(screen.min.x, screen.max.y - FOOTER_HEIGHT), screen.max);
        let browser_visible = !layout.content_browser_collapsed && !workspace.maximized;
        let browser_width = if browser_visible {
            layout
                .content_browser_width
                .clamp(CONTENT_BROWSER_MINIMUM_WIDTH, CONTENT_BROWSER_MAXIMUM_WIDTH)
                .min((screen.width() * 0.3).max(CONTENT_BROWSER_MINIMUM_WIDTH))
        } else {
            0.0
        };
        let browser =
            Rect::from_min_max(pos2(screen.min.x, header.max.y), pos2(screen.min.x + browser_width, footer.min.y));
        let document = Rect::from_min_max(pos2(browser.max.x, header.max.y), pos2(screen.max.x, footer.min.y));

        self.header(context, header, workspace, browser_visible);
        if browser_visible {
            self.content_browser(context, browser, workspace);
            let handle = Rect::from_center_size(browser.right_center(), vec2(6.0, browser.height()));
            let (delta, finished) =
                region(context, "joy-editor-browser-divider", handle, Color32::TRANSPARENT, Order::Middle, |ui| {
                    widgets::divider(ui, Id::new("assets-divider"), handle, DIVIDER_KEY_STEP)
                });
            self.controls.insert("assets-divider", handle);
            if delta != 0.0 {
                layout.content_browser_width =
                    (browser_width + delta).clamp(CONTENT_BROWSER_MINIMUM_WIDTH, CONTENT_BROWSER_MAXIMUM_WIDTH);
            }
            if finished || delta != 0.0 {
                workspace.set_layout(layout);
            }
        }
        self.document_area(context, document, workspace);
        self.footer(context, footer, workspace);
        self.popovers(context, workspace);
        self.dialogs(context, workspace);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Shortcuts
    // -----------------------------------------------------------------------------------------------------------------

    fn shortcuts(&mut self, context: &egui::Context, workspace: &mut Workspace) {
        if self.dialog.is_some() {
            return;
        }
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        let search = KeyboardShortcut::new(Modifiers::COMMAND, Key::P);
        if context.input_mut(|input| input.consume_shortcut(&save)) {
            workspace.save_active();
        }
        if context.input_mut(|input| input.consume_shortcut(&search)) {
            self.reveal_search(workspace);
        }
        if context.input(|input| input.key_pressed(Key::Escape)) {
            if self.filters_open || self.asset_menu.is_some() {
                self.filters_open = false;
                self.asset_menu = None;
            } else if workspace.maximized && !context.egui_wants_keyboard_input() {
                workspace.maximized = false;
            }
        }
    }

    /// Show and focus the search field (Cmd/Ctrl+P), from anywhere including a tool.
    pub fn reveal_search(&mut self, workspace: &mut Workspace) {
        workspace.maximized = false;
        let mut layout = workspace.layout();
        layout.content_browser_collapsed = false;
        workspace.set_layout(layout);
        self.focus_search = true;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Header and footer
    // -----------------------------------------------------------------------------------------------------------------

    fn header(&mut self, context: &egui::Context, rect: Rect, workspace: &mut Workspace, browser_visible: bool) {
        region(context, "joy-editor-header", rect, palette::HEADER, Order::Middle, |ui| {
            widgets::edge(ui, rect.left_bottom(), rect.right_bottom());
            let inner = rect.shrink2(vec2(16.0, 10.0));
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(Align::Center)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    // Brand.
                    let (mark, _) = ui.allocate_exact_size(vec2(28.0, 30.0), Sense::hover());
                    ui.painter().rect_filled(mark, 0.0, palette::ACCENT);
                    let letter = hud_text::single(ui.painter(), "J", Style::new(22.0, palette::ON_ACCENT).heavy());
                    letter.paint_in(ui.painter(), mark, Align::Center, Align::Center);
                    let brand = hud_text::layout(
                        ui.painter(),
                        &[
                            ("JOY ", Style::new(15.0, palette::TEXT).spacing(0.8)),
                            ("EDITOR", Style::new(15.0, palette::MUTED).spacing(0.8)),
                        ],
                        f32::INFINITY,
                        Align::LEFT,
                    );
                    let (brand_rect, _) = ui.allocate_exact_size(brand.size(), Sense::hover());
                    brand.paint(ui.painter(), brand_rect.min);
                    let (line, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
                    ui.painter().rect_filled(line, 0.0, palette::LINE);

                    // Project picker.
                    caption(ui, "PROJECT", palette::MUTED);
                    let current = workspace.project_summary().map(|project| project.name.clone()).unwrap_or_default();
                    let mut chosen = None;
                    let combo = egui::ComboBox::from_id_salt("project")
                        .selected_text(RichText::new(current).size(12.0))
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for project in workspace.projects() {
                                let selected = project.id == workspace.project();
                                if ui.selectable_label(selected, &project.name).clicked() && !selected {
                                    chosen = Some(project.id.clone());
                                }
                            }
                        });
                    self.controls.mark("project", combo.response);
                    if let Some(id) = chosen {
                        let outcome = workspace.select_project(&id, false);
                        self.handle(outcome);
                    }
                    if cfg!(not(target_arch = "wasm32")) {
                        let open = button(ui, "Open Folder");
                        if self.controls.mark("open-folder", open).clicked() {
                            self.dialog = Some(Dialog::OpenFolder { text: String::new() });
                        }
                    }

                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        let tab = workspace.active_tab();
                        let dirty = tab.is_some_and(|tab| tab.dirty());
                        let saving = tab.is_some_and(|tab| tab.document.saving);
                        let conflict = tab.is_some_and(|tab| tab.document.conflict.is_some());
                        let state = match tab {
                            None => {
                                if workspace.writable() {
                                    "LOCAL FILES"
                                } else {
                                    "READ-ONLY EXAMPLES"
                                }
                            }
                            Some(_) if saving => "SAVING…",
                            Some(_) if conflict => "DISK CONFLICT",
                            Some(_) if dirty => "UNSAVED CHANGES",
                            Some(_) if !workspace.writable() => "BUNDLED EXAMPLE",
                            Some(_) => "SAVED TO PROJECT",
                        };
                        let enabled = dirty && !saving && !conflict && workspace.writable();
                        let save = ui.add_enabled_ui(enabled, |ui| primary_button(ui, "Save File  ⌘ S")).inner;
                        if self.controls.mark("save", save).clicked() {
                            workspace.save_active();
                        }
                        if !workspace.writable() && workspace.active_tab().is_some() {
                            let download = button(ui, "Download");
                            if self.controls.mark("download", download).clicked() {
                                download_active(workspace);
                            }
                        }
                        let label = ui.label(RichText::new(state).size(9.0).color(palette::MUTED));
                        self.controls.mark("document-state", label);
                        let toggle = toggle_button(ui, "Content Browser", browser_visible);
                        if self.controls.mark("assets-toggle", toggle).clicked() {
                            workspace.maximized = false;
                            let mut layout = workspace.layout();
                            layout.content_browser_collapsed = browser_visible;
                            workspace.set_layout(layout);
                        }
                    });
                },
            );
        });
    }

    fn footer(&mut self, context: &egui::Context, rect: Rect, workspace: &Workspace) {
        region(context, "joy-editor-footer", rect, palette::HEADER, Order::Middle, |ui| {
            widgets::edge(ui, rect.left_top(), rect.right_top());
            let inner = rect.shrink2(vec2(14.0, 4.0));
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(Align::Center)),
                |ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.5, palette::SUCCESS);
                    let status = ui.add(
                        egui::Label::new(RichText::new(workspace.status()).size(11.0).color(palette::TEXT)).truncate(),
                    );
                    self.controls.mark("status", status);
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new("JOY ENGINE").size(10.0).color(palette::MUTED));
                        let count = ui.label(
                            RichText::new(format!("{} ASSETS", workspace.files().len()))
                                .size(10.0)
                                .color(palette::MUTED),
                        );
                        self.controls.mark("file-count", count);
                    });
                },
            );
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Content Browser
    // -----------------------------------------------------------------------------------------------------------------

    fn content_browser(&mut self, context: &egui::Context, rect: Rect, workspace: &mut Workspace) {
        region(context, "joy-editor-browser", rect, palette::PANEL, Order::Middle, |ui| {
            let inner = rect.shrink2(vec2(12.0, 10.0));
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(Align::LEFT)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.horizontal(|ui| {
                        caption(ui, "CONTENT BROWSER", palette::MUTED);
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            let refresh = icon_button(ui, Icon::Refresh, 26.0, palette::MUTED, false)
                                .on_hover_text("Refresh content");
                            if self.controls.mark("refresh", refresh).clicked() {
                                workspace.refresh_files();
                                workspace.set_status("Content refreshed.");
                            }
                        });
                    });

                    // Search and type filter.
                    ui.horizontal(|ui| {
                        let filter_width = 28.0;
                        let width = (ui.available_width() - filter_width - 8.0).max(60.0);
                        let search = ui
                            .scope(|ui| {
                                ui.visuals_mut().extreme_bg_color = palette::FIELD;
                                TextEdit::singleline(&mut workspace.search)
                                    .id(Id::new("search"))
                                    .hint_text("Find an asset…")
                                    .desired_width(width)
                                    .margin(vec2(6.0, 5.0))
                                    .show(ui)
                                    .response
                                    .response
                            })
                            .inner;
                        if std::mem::take(&mut self.focus_search) {
                            search.request_focus();
                        }
                        self.controls.mark("search", search);
                        let restricted = !workspace.filters().is_empty();
                        let filter = icon_button(ui, Icon::Filter, filter_width, palette::MUTED, restricted)
                            .on_hover_text(if restricted {
                                format!("Filter assets, {} types selected", workspace.filters().len())
                            } else {
                                "Filter assets".to_owned()
                            });
                        if restricted {
                            let count = hud_text::single(
                                ui.painter(),
                                &workspace.filters().len().to_string(),
                                Style::new(9.0, palette::ACCENT).bold(),
                            );
                            count.paint(ui.painter(), filter.rect.right_top() + vec2(-8.0, 1.0));
                        }
                        if self.controls.mark("filter-toggle", filter.clone()).clicked() {
                            self.filters_open = !self.filters_open;
                        }
                    });

                    // New and Import.
                    ui.horizontal(|ui| {
                        let half = ((ui.available_width() - 8.0) / 2.0).max(40.0);
                        let writable = workspace.writable();
                        let new = ui
                            .add_enabled_ui(writable, |ui| {
                                ui.add_sized(
                                    [half, 28.0],
                                    egui::Button::new(RichText::new("+ New").size(12.0).color(palette::ACCENT))
                                        .fill(palette::PANEL)
                                        .stroke(Stroke::new(1.0, palette::ACCENT))
                                        .corner_radius(0.0),
                                )
                            })
                            .inner;
                        if self.controls.mark("new-asset", new).clicked() {
                            self.new_asset = Some(NewAssetForm {
                                kind: DEFAULT_NEW_ASSET_KIND.to_owned(),
                                name: String::new(),
                                focus: true,
                            });
                        }
                        let import = ui
                            .add_enabled_ui(writable, |ui| {
                                ui.add_sized(
                                    [half, 28.0],
                                    egui::Button::new(RichText::new("Import").size(12.0).color(palette::ACCENT))
                                        .fill(palette::PANEL)
                                        .stroke(Stroke::new(1.0, palette::ACCENT))
                                        .corner_radius(0.0),
                                )
                            })
                            .inner;
                        if self.controls.mark("import-asset", import).clicked() {
                            self.begin_import(workspace);
                        }
                    });
                    if self.new_asset.is_some() {
                        self.new_asset_form(ui, workspace);
                    }
                    ui.label(
                        RichText::new("Import .form, .formlab, .joyfx, .joylevel or .joyobject files.")
                            .size(10.0)
                            .color(palette::MUTED),
                    );

                    // Rows, then Project Details and Documentation at the bottom.
                    let bottom_height = if self.project_details_open { 150.0 } else { 70.0 };
                    let tree_height = (ui.available_height() - bottom_height).max(60.0);
                    caption(ui, "ASSETS", palette::MUTED);
                    ScrollArea::vertical().id_salt("files").max_height(tree_height).auto_shrink([false, false]).show(
                        ui,
                        |ui| {
                            self.file_rows(ui, workspace);
                        },
                    );
                    ui.add_space((ui.available_height() - bottom_height + 8.0).max(0.0));
                    widgets::edge(ui, pos2(inner.left(), ui.cursor().top()), pos2(inner.right(), ui.cursor().top()));
                    let details =
                        egui::CollapsingHeader::new(RichText::new("Project Details").size(12.0).color(palette::ACCENT))
                            .id_salt("project-details")
                            .default_open(false)
                            .show(ui, |ui| {
                                caption(ui, "LOCAL PROJECT", palette::MUTED);
                                if let Some(project) = workspace.project_summary() {
                                    ui.label(RichText::new(&project.name).size(13.0).strong());
                                    ui.label(
                                        RichText::new(&project.location).size(9.0).monospace().color(palette::MUTED),
                                    );
                                    for capability in &project.manifest.capabilities {
                                        let features: Vec<&str> = capability
                                            .features
                                            .iter()
                                            .filter(|(_, enabled)| **enabled)
                                            .map(|(name, _)| name.as_str())
                                            .collect();
                                        ui.label(RichText::new(&capability.label).size(11.0).strong());
                                        ui.label(
                                            RichText::new(capability.extensions.join(", "))
                                                .size(10.0)
                                                .color(palette::MUTED),
                                        );
                                        let features = if features.is_empty() {
                                            "No optional renderer features".to_owned()
                                        } else {
                                            features.join(" · ")
                                        };
                                        ui.label(RichText::new(features).size(9.0).color(palette::MUTED));
                                    }
                                }
                            });
                    self.project_details_open = details.fully_open() || details.body_response.is_some();
                    self.controls.mark("project-details", details.header_response);
                    let documentation = ui.add_sized(
                        [ui.available_width(), 28.0],
                        egui::Button::new(RichText::new("Documentation ↗").size(12.0))
                            .fill(palette::SURFACE)
                            .stroke(Stroke::new(1.0, palette::LINE))
                            .corner_radius(0.0),
                    );
                    if self.controls.mark("workspace-help-open", documentation).clicked() {
                        self.dialog = Some(Dialog::Documentation);
                    }
                },
            );
        });
    }

    fn new_asset_form(&mut self, ui: &mut Ui, workspace: &mut Workspace) {
        let Some(form) = self.new_asset.as_mut() else {
            return;
        };
        let mut create = false;
        let mut cancel = false;
        egui::Frame::new().fill(palette::SURFACE).inner_margin(8.0).show(ui, |ui| {
            caption(ui, "TYPE", palette::MUTED);
            let label = NEW_ASSET_KINDS.iter().find(|(kind, _)| *kind == form.kind).map_or("", |(_, label)| *label);
            let combo = egui::ComboBox::from_id_salt("new-asset-type")
                .selected_text(label)
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    for (kind, label) in NEW_ASSET_KINDS {
                        ui.selectable_value(&mut form.kind, kind.to_owned(), label);
                    }
                });
            self.controls.mark("new-asset-type", combo.response);
            caption(ui, "NAME", palette::MUTED);
            let name = ui
                .scope(|ui| {
                    ui.visuals_mut().extreme_bg_color = palette::FIELD;
                    TextEdit::singleline(&mut form.name)
                        .id(Id::new("new-asset-name"))
                        .hint_text("asset-name")
                        .desired_width(f32::INFINITY)
                        .show(ui)
                        .response
                        .response
                })
                .inner;
            if std::mem::take(&mut form.focus) {
                name.request_focus();
            }
            if name.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                create = true;
            }
            self.controls.mark("new-asset-name", name);
            ui.label(RichText::new(new_asset_hint(&form.kind)).size(10.0).color(palette::MUTED));
            ui.horizontal(|ui| {
                let submit = primary_button(ui, "Create");
                if self.controls.mark("new-asset-create", submit).clicked() {
                    create = true;
                }
                if self.controls.mark("cancel-new-asset", button(ui, "Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if create && let Some(form) = self.new_asset.clone() {
            if workspace.create_asset(&form.name, &form.kind).is_ok() {
                self.new_asset = None;
            }
        } else if cancel {
            self.new_asset = None;
        }
    }

    fn file_rows(&mut self, ui: &mut Ui, workspace: &mut Workspace) {
        let files: Vec<_> = workspace.filtered_files().into_iter().cloned().collect();
        let mut folder = String::new();
        let mut open = None;
        let mut rename = None;
        let mut delete = None;
        for file in &files {
            if file.folder() != folder {
                folder = file.folder().to_owned();
                ui.add_space(4.0);
                ui.label(RichText::new(folder.to_uppercase()).size(10.0).color(palette::MUTED));
            }
            let selected = workspace.selected_asset() == Some(file.path.as_str());
            let width = ui.available_width();
            let (rect, response) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
            let response = response.on_hover_text(&file.path);
            let hovered = response.hovered() || response.has_focus();
            let painter = ui.painter();
            if selected || hovered {
                painter.rect_filled(rect, 0.0, if selected { palette::SELECTED } else { palette::SURFACE });
            }
            if selected {
                painter.rect_filled(Rect::from_min_size(rect.min, vec2(2.0, rect.height())), 0.0, palette::ACCENT);
            }
            if response.has_focus() {
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0, palette::ACCENT), StrokeKind::Inside);
            }
            let (mark, fill, color) = match file.kind.as_str() {
                "joyfx" => ("FX", Color32::from_rgb(0x34, 0x30, 0x47), Color32::from_rgb(0xc8, 0xb7, 0xeb)),
                "joylevel" => ("L", Color32::from_rgb(0x24, 0x37, 0x4b), Color32::from_rgb(0x8a, 0xa9, 0xc6)),
                "joyobject" => ("O", Color32::from_rgb(0x24, 0x37, 0x4b), Color32::from_rgb(0x8a, 0xa9, 0xc6)),
                _ => ("F", Color32::from_rgb(0x24, 0x37, 0x4b), Color32::from_rgb(0x8a, 0xa9, 0xc6)),
            };
            badge(painter, Rect::from_min_size(rect.min + vec2(10.0, 6.0), vec2(20.0, 20.0)), mark, fill, color);
            let name = hud_text::single(painter, file.file_name(), Style::new(12.0, palette::TEXT));
            name.paint(painter, pos2(rect.min.x + 38.0, rect.center().y - (name.size().y / 2.0)));
            self.controls.mark(format!("row:{}", file.path), response.clone());
            if response.clicked() {
                workspace.select_asset(&file.path);
                open = Some(file.path.clone());
                response.request_focus();
            }
            if response.secondary_clicked() {
                self.asset_menu =
                    Some((file.path.clone(), response.interact_pointer_pos().unwrap_or(rect.left_bottom())));
            }
            if response.has_focus() && ui.input(|input| input.key_pressed(Key::F2)) {
                rename = Some(file.path.clone());
            }
            if hovered || selected {
                // Row actions sit at the right edge: the actions menu and delete.
                let trash = Rect::from_min_size(pos2(rect.right() - 30.0, rect.top() + 4.0), vec2(24.0, 24.0));
                let more = Rect::from_min_size(pos2(rect.right() - 58.0, rect.top() + 4.0), vec2(24.0, 24.0));
                let more_response =
                    ui.put(more, |ui: &mut Ui| icon_button(ui, Icon::More, 24.0, palette::MUTED, false));
                if self.controls.mark(format!("more:{}", file.path), more_response.clone()).clicked() {
                    self.asset_menu = Some((file.path.clone(), more_response.rect.left_bottom()));
                }
                let trash_response = ui
                    .put(trash, |ui: &mut Ui| icon_button(ui, Icon::Trash, 24.0, palette::DANGER, false))
                    .on_hover_text("Delete asset");
                if self.controls.mark(format!("delete:{}", file.path), trash_response).clicked() {
                    delete = Some(file.path.clone());
                }
            }
        }
        if files.is_empty() {
            ui.label(RichText::new("No matching assets.").size(11.0).color(palette::MUTED));
        }
        if let Some(path) = open {
            let _ = workspace.open(&path);
        }
        if let Some(path) = rename {
            self.begin_rename(workspace, &path);
        }
        if let Some(path) = delete {
            let outcome = workspace.delete_asset(&path, false);
            self.handle(outcome);
        }
    }

    fn begin_rename(&mut self, workspace: &mut Workspace, path: &str) {
        self.asset_menu = None;
        if workspace.check_modifiable(path, "renaming").is_ok() {
            let text = path.strip_prefix("assets/").unwrap_or(path).to_owned();
            self.dialog = Some(Dialog::Rename { path: path.to_owned(), text });
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Documents
    // -----------------------------------------------------------------------------------------------------------------

    fn document_area(&mut self, context: &egui::Context, rect: Rect, workspace: &mut Workspace) {
        let tabs_rect = Rect::from_min_size(rect.min, vec2(rect.width(), TABS_HEIGHT));
        self.tab_strip(context, tabs_rect, workspace);
        let mut top = tabs_rect.max.y;

        let conflict = workspace.active_tab().is_some_and(|tab| tab.document.conflict.is_some());
        if workspace.return_path().is_some() || conflict {
            let bar = Rect::from_min_max(pos2(rect.min.x, top), pos2(rect.max.x, top + BAR_HEIGHT));
            self.document_bar(context, bar, workspace, conflict);
            top = bar.max.y;
        }
        let editor = Rect::from_min_max(pos2(rect.min.x, top), rect.max);
        self.document_rect = editor;

        let path = workspace.active_path().map(str::to_owned);
        let Some(path) = path else {
            self.empty_state(context, editor, workspace);
            return;
        };
        let is_level = matches!(workspace.tab(&path).map(|tab| &tab.content), Some(TabContent::Level(_)));
        let is_source = matches!(workspace.tab(&path).map(|tab| &tab.content), Some(TabContent::Source(_)));
        if is_level {
            let mut created = false;
            if let Some((level, mut access)) = workspace.active_level() {
                self.level.show(context, editor, level, &mut access, &mut self.controls);
                created = !access.created.is_empty();
            }
            if created {
                workspace.refresh_files();
            }
            workspace.route_level_requests();
        } else if is_source {
            self.source_view(context, editor, workspace, &path);
        }
    }

    fn tab_strip(&mut self, context: &egui::Context, rect: Rect, workspace: &mut Workspace) {
        region(context, "joy-editor-tabs", rect, palette::HEADER, Order::Middle, |ui| {
            widgets::edge(ui, rect.left_bottom(), rect.right_bottom());
            let mut activate = None;
            let mut close = None;
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let project = workspace.project().to_owned();
                    for tab in workspace.tabs() {
                        let path = tab.document.path.clone();
                        let active = workspace.active_path() == Some(path.as_str());
                        let marker = if tab.document.conflict.is_some() {
                            "! "
                        } else if tab.dirty() {
                            "● "
                        } else {
                            ""
                        };
                        let label = hud_text::single(
                            ui.painter(),
                            &format!("{marker}{}", tab.file_name()),
                            Style::new(12.0, palette::TEXT),
                        );
                        let width = label.size().x + 52.0;
                        let (tab_rect, response) = ui.allocate_exact_size(vec2(width, rect.height()), Sense::click());
                        let response = response.on_hover_text(format!("{project} / {path}"));
                        let painter = ui.painter();
                        painter.rect_filled(tab_rect, 0.0, if active { palette::SURFACE } else { palette::PANEL });
                        if active {
                            painter.rect_filled(
                                Rect::from_min_size(tab_rect.min, vec2(tab_rect.width(), 2.0)),
                                0.0,
                                palette::ACCENT,
                            );
                        }
                        painter.line_segment(
                            [tab_rect.right_top(), tab_rect.right_bottom()],
                            Stroke::new(1.0, palette::LINE),
                        );
                        label.paint(painter, pos2(tab_rect.min.x + 12.0, tab_rect.center().y - (label.size().y / 2.0)));
                        let close_rect = Rect::from_center_size(
                            pos2(tab_rect.right() - 18.0, tab_rect.center().y),
                            vec2(20.0, 20.0),
                        );
                        let close_response = ui
                            .interact(close_rect, Id::new(("close-tab", &path)), Sense::click())
                            .on_hover_text(format!("Close {path}"));
                        widgets::paint_icon(
                            ui.painter(),
                            close_rect.shrink(5.0),
                            Icon::Close,
                            if close_response.hovered() { palette::ACCENT } else { palette::MUTED },
                        );
                        self.controls.mark(format!("tab:{path}"), response.clone());
                        self.controls.mark(format!("close:{path}"), close_response.clone());
                        if close_response.clicked() {
                            close = Some(path.clone());
                        } else if response.clicked() {
                            activate = Some(path.clone());
                        }
                    }
                    let tool_tab = matches!(workspace.active_tab().map(|tab| &tab.content), Some(TabContent::Tool(_)));
                    if tool_tab {
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(8.0);
                            let label = if workspace.maximized { "Restore" } else { "Maximize" };
                            let maximize = toggle_button(ui, label, workspace.maximized);
                            if self.controls.mark("lab-maximize", maximize).clicked() {
                                workspace.maximized = !workspace.maximized;
                            }
                        });
                    }
                },
            );
            if let Some(path) = close {
                let outcome = workspace.close(&path, false);
                self.handle(outcome);
            } else if let Some(path) = activate {
                workspace.activate(&path);
            }
        });
    }

    fn document_bar(&mut self, context: &egui::Context, rect: Rect, workspace: &mut Workspace, conflict: bool) {
        region(context, "joy-editor-document-bar", rect, palette::PANEL, Order::Middle, |ui| {
            widgets::edge(ui, rect.left_bottom(), rect.right_bottom());
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(rect.shrink2(vec2(12.0, 4.0)))
                    .layout(egui::Layout::left_to_right(Align::Center)),
                |ui| {
                    if let Some(owner) = workspace.return_path().map(str::to_owned) {
                        let name = owner.rsplit('/').next().unwrap_or(&owner).to_owned();
                        let back = button(ui, &format!("← {name}"));
                        if self.controls.mark("return-owner", back).clicked() {
                            workspace.return_to_owner();
                        }
                    }
                    if conflict {
                        ui.label(
                            RichText::new("This file changed on disk. Your edits are still here.")
                                .size(12.0)
                                .color(palette::DANGER),
                        );
                        let reload = danger_button(ui, "Use Disk Version");
                        if self.controls.mark("reload-disk", reload).clicked() {
                            let outcome = workspace.reload_disk(false);
                            self.handle(outcome);
                        }
                        let keep = button(ui, "Keep My Edits");
                        if self.controls.mark("keep-local", keep).clicked() {
                            workspace.keep_local();
                        }
                    }
                },
            );
        });
    }

    fn empty_state(&mut self, context: &egui::Context, rect: Rect, workspace: &mut Workspace) {
        region(context, "joy-editor-empty", rect, palette::CANVAS, Order::Middle, |ui| {
            let center = rect.center();
            let mark = Rect::from_center_size(center - vec2(0.0, 110.0), vec2(48.0, 52.0));
            ui.painter().rect_filled(mark, 0.0, palette::ACCENT);
            hud_text::single(ui.painter(), "J", Style::new(38.0, palette::ON_ACCENT).heavy()).paint_in(
                ui.painter(),
                mark,
                Align::Center,
                Align::Center,
            );
            let title =
                hud_text::single(ui.painter(), "A place for your next idea.", Style::new(26.0, palette::TEXT).bold());
            title.paint_in(
                ui.painter(),
                Rect::from_center_size(center - vec2(0.0, 50.0), vec2(rect.width(), 40.0)),
                Align::Center,
                Align::Center,
            );
            let body = hud_text::wrapped(
                ui.painter(),
                "Open a FORM, particle or level asset from the project.\nEdit it here. See it in the game.",
                Style::new(13.0, palette::MUTED).line_height(1.6),
                rect.width() - 40.0,
                Align::Center,
            );
            body.paint_in(
                ui.painter(),
                Rect::from_center_size(center, vec2(rect.width(), 50.0)),
                Align::Center,
                Align::Center,
            );
            let button_rect = Rect::from_center_size(center + vec2(0.0, 60.0), vec2(200.0, 32.0));
            let open = ui.put(button_rect, |ui: &mut Ui| primary_button(ui, "Open Project Content"));
            if self.controls.mark("open-first", open).clicked()
                && let Some(first) = workspace.files().first().map(|file| file.path.clone())
            {
                let _ = workspace.open(&first);
            }
            let note = hud_text::single(ui.painter(), "Same files. One workspace.", Style::new(11.0, palette::MUTED));
            note.paint_in(
                ui.painter(),
                Rect::from_center_size(center + vec2(0.0, 100.0), vec2(rect.width(), 20.0)),
                Align::Center,
                Align::Center,
            );
        });
    }

    fn source_view(&mut self, context: &egui::Context, rect: Rect, workspace: &mut Workspace, path: &str) {
        region(context, "joy-editor-source", rect, palette::CANVAS, Order::Middle, |ui| {
            let Some(tab) = workspace.tab_mut(path) else {
                return;
            };
            let TabContent::Source(SourceView { text, message, .. }) = &mut tab.content else {
                return;
            };
            let inner = rect.shrink(14.0);
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(Align::LEFT)),
                |ui| {
                    let heading = match message {
                        Some(message) => {
                            format!("Preview unavailable: {message} Edit the source below and save to retry.")
                        }
                        None => "Source view. Save File writes this text to the project.".to_owned(),
                    };
                    ui.label(RichText::new(heading).size(12.0).color(if message.is_some() {
                        palette::DANGER
                    } else {
                        palette::MUTED
                    }));
                    let editor = self.source_editors.entry(path.to_owned()).or_insert_with(|| source_editor(path));
                    let response = editor.show(ui, text);
                    if response.changed {
                        tab.document.edit(text.clone());
                    }
                    let response = response.response;
                    self.controls.mark(format!("repair:{path}"), response);
                },
            );
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Popovers and dialogs
    // -----------------------------------------------------------------------------------------------------------------

    fn popovers(&mut self, context: &egui::Context, workspace: &mut Workspace) {
        if self.filters_open
            && let Some(anchor) = self.controls.get("filter-toggle")
        {
            let area = egui::Area::new(Id::new("asset-filters"))
                .order(Order::Foreground)
                .fixed_pos(anchor.left_bottom() + vec2(-130.0, 6.0))
                .show(context, |ui| {
                    dialog_frame().inner_margin(8.0).show(ui, |ui| {
                        ui.set_min_width(150.0);
                        let all = ui.add_sized(
                            [150.0, 26.0],
                            egui::Button::new("All types").fill(palette::SURFACE).corner_radius(0.0),
                        );
                        if self.controls.mark("filter-all", all).clicked() {
                            workspace.clear_filters();
                        }
                        for (kind, label) in FILTERS {
                            let mut checked = workspace.filters().contains(kind);
                            let checkbox = ui.checkbox(&mut checked, label);
                            if self.controls.mark(format!("filter:{kind}"), checkbox.clone()).changed() {
                                workspace.toggle_filter(kind);
                            }
                        }
                    });
                });
            let clicked_outside = context.input(|input| input.pointer.any_pressed())
                && !area
                    .response
                    .rect
                    .contains(context.input(|input| input.pointer.interact_pos().unwrap_or_default()))
                && !anchor.contains(context.input(|input| input.pointer.interact_pos().unwrap_or_default()));
            if clicked_outside {
                self.filters_open = false;
            }
        }
        if let Some((path, anchor)) = self.asset_menu.clone() {
            let area =
                egui::Area::new(Id::new("asset-menu")).order(Order::Foreground).fixed_pos(anchor).show(context, |ui| {
                    dialog_frame().inner_margin(6.0).show(ui, |ui| {
                        let rename = ui.add_sized(
                            [140.0, 28.0],
                            egui::Button::new("Rename   F2").fill(palette::SURFACE).corner_radius(0.0),
                        );
                        if self.controls.mark("rename-asset", rename).clicked() {
                            self.begin_rename(workspace, &path);
                        }
                    });
                });
            let pointer = context.input(|input| input.pointer.interact_pos().unwrap_or_default());
            if context.input(|input| input.pointer.any_pressed())
                && !area.response.rect.contains(pointer)
                && self.asset_menu.as_ref().is_some_and(|(_, at)| *at == anchor)
            {
                // A press outside closes the menu; the press that opened it lands on the row, not here.
                let opened_here = self.controls.get(&format!("row:{path}")).is_some_and(|row| row.contains(pointer))
                    || self.controls.get(&format!("more:{path}")).is_some_and(|more| more.contains(pointer));
                if !opened_here {
                    self.asset_menu = None;
                }
            }
        }
    }

    fn dialogs(&mut self, context: &egui::Context, workspace: &mut Workspace) {
        let Some(mut dialog) = self.dialog.clone() else {
            return;
        };
        // `next` replaces the dialog: `Some(None)` closes it, `Some(Some(…))` asks a follow-up question.
        let mut next: Option<Option<Dialog>> = None;
        let modal =
            egui::Modal::new(Id::new("joy-editor-dialog")).frame(dialog_frame()).show(
                context,
                |ui| match &mut dialog {
                    Dialog::Confirm(confirmation) => {
                        ui.set_max_width(420.0);
                        ui.label(RichText::new(confirmation.message()).size(14.0));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let confirm = danger_button(ui, confirmation.action());
                            if self.controls.mark("dialog-confirm", confirm).clicked() {
                                next = Some(None);
                                let outcome = match confirmation.clone() {
                                    Confirmation::SwitchProject(id) => workspace.select_project(&id, true),
                                    Confirmation::CloseTab(path) => workspace.close(&path, true),
                                    Confirmation::DeleteAsset(path) => workspace.delete_asset(&path, true),
                                    Confirmation::ReloadDisk(_) => workspace.reload_disk(true),
                                    Confirmation::Quit => {
                                        self.commands.quit = true;
                                        Outcome::Done
                                    }
                                };
                                if let Outcome::Confirm(question) = outcome {
                                    next = Some(Some(Dialog::Confirm(question)));
                                }
                            }
                            if self.controls.mark("dialog-cancel", button(ui, "Cancel")).clicked() {
                                next = Some(None);
                            }
                        });
                    }
                    Dialog::Rename { path, text } => {
                        ui.set_max_width(420.0);
                        ui.label(RichText::new(format!("Rename {path} to:")).size(13.0));
                        let field = TextEdit::singleline(text)
                            .id(Id::new("rename-field"))
                            .desired_width(380.0)
                            .show(ui)
                            .response
                            .response;
                        if !field.has_focus() && !field.lost_focus() && ui.memory(|memory| memory.focused().is_none()) {
                            field.request_focus();
                        }
                        let submitted = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                        self.controls.mark("rename-field", field);
                        ui.horizontal(|ui| {
                            let rename = primary_button(ui, "Rename");
                            if (self.controls.mark("dialog-confirm", rename).clicked() || submitted)
                                && workspace.rename_asset(path, text).is_ok()
                            {
                                next = Some(None);
                            }
                            if self.controls.mark("dialog-cancel", button(ui, "Cancel")).clicked() {
                                next = Some(None);
                            }
                        });
                    }
                    Dialog::OpenFolder { text } => {
                        ui.set_max_width(460.0);
                        ui.label(
                            RichText::new("Open a Joy project folder (one with a project.joyproject manifest):")
                                .size(13.0),
                        );
                        let field = TextEdit::singleline(text)
                            .id(Id::new("folder-field"))
                            .hint_text("/path/to/project")
                            .desired_width(420.0)
                            .show(ui)
                            .response
                            .response;
                        let submitted = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                        self.controls.mark("folder-field", field);
                        ui.horizontal(|ui| {
                            let open = primary_button(ui, "Open");
                            if self.controls.mark("dialog-confirm", open).clicked() || submitted {
                                match workspace.open_folder(text.trim()) {
                                    Outcome::Done => next = Some(None),
                                    Outcome::Confirm(question) => next = Some(Some(Dialog::Confirm(question))),
                                    Outcome::Blocked => {}
                                }
                            }
                            if self.controls.mark("dialog-cancel", button(ui, "Cancel")).clicked() {
                                next = Some(None);
                            }
                        });
                    }
                    Dialog::Documentation => {
                        ui.set_max_width(640.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Joy Editor Documentation").size(22.0).color(palette::ACCENT));
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                if self.controls.mark("workspace-help-close", button(ui, "Close")).clicked() {
                                    next = Some(None);
                                }
                            });
                        });
                        ScrollArea::vertical().max_height(560.0).show(ui, |ui| {
                            for (title, body) in super::documentation::SECTIONS {
                                ui.separator();
                                ui.label(RichText::new(title).size(15.0).color(palette::ACCENT));
                                for paragraph in body {
                                    ui.label(RichText::new(*paragraph).size(13.0));
                                    ui.add_space(4.0);
                                }
                            }
                        });
                    }
                },
            );
        if next.is_none() && modal.should_close() {
            next = Some(None);
        }
        self.dialog = match next {
            Some(replacement) => replacement,
            None => Some(dialog),
        };
    }

    /// Ask for a file to import through the platform file dialog.
    fn begin_import(&mut self, workspace: &mut Workspace) {
        let filter = FileFilter::new("Joy assets", &["form", "formlab", "joyfx", "joylevel", "joyobject"]);
        let request = OpenRequest::text("Import asset", filter).maximum_bytes(crate::server::MAXIMUM_ASSET_BYTES);
        if let Err(error) = self.file_dialogs.open(request) {
            workspace.set_status(error.to_string());
        }
    }

    /// Import a file the picker produced; existing project files are never replaced.
    fn poll_dialogs(&mut self, workspace: &mut Workspace) {
        while let Some(event) = self.file_dialogs.poll() {
            match event {
                FileDialogEvent::Opened(file) => match file.contents.text() {
                    Some(text) => {
                        let _ = workspace.import_asset(&file.name, text);
                    }
                    None => workspace.set_status(format!("{} is not a text asset.", file.name)),
                },
                FileDialogEvent::Failed(message) => workspace.set_status(message),
                FileDialogEvent::Cancelled | FileDialogEvent::Saved { .. } => {}
            }
        }
    }

    /// Show the question for a confirmation outcome.
    fn handle(&mut self, outcome: Outcome) {
        if let Outcome::Confirm(confirmation) = outcome {
            self.dialog = Some(Dialog::Confirm(confirmation));
        }
    }
}

/// A script editor for a source or repair view, highlighted for the file's language.
fn source_editor(path: &str) -> ScriptEditor {
    let id = ("joy-editor-source", path.to_owned());
    match crate::server::extension_of(path).as_str() {
        ".form" => ScriptEditor::new(id, ScriptSyntax::form()).with_catalog(joy_engine::form::form_language_catalog()),
        ".formlab" | ".joyfx" | ".joylevel" | ".joyobject" | ".joyproject" | ".json" => {
            ScriptEditor::new(id, ScriptSyntax::json())
        }
        _ => ScriptEditor::new(id, ScriptSyntax::plain()),
    }
}

/// Hand the active document's current text to the browser (or the working directory natively).
fn download_active(workspace: &mut Workspace) {
    let Some(tab) = workspace.active_tab() else {
        return;
    };
    let name = tab.file_name().to_owned();
    let text = tab.document.text.clone();
    let message = match joy_engine::runtime::save_text_file(&name, &text) {
        Ok(location) => format!("Downloaded {location}"),
        Err(error) => error.to_string(),
    };
    workspace.set_status(message);
}
