// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The PARTICLE LAB interface in egui: the
//! header with Open, Save, and the script reference; the project bar; the compose panel (Live and Apply, the emitter
//! stack, the emitter inspector, and the spawn and update script editors); the preview panel (camera actions, playback
//! toolbar, the viewport, preview options, replay window, timeline, diagnostics, and the performance monitor); the
//! effect parameter panel; and the footer, laid out with the kit's resizable panes.
//!
//! Panels are opaque; the preview region is left open for the engine's render of the scene behind the interface, and
//! its rectangle is returned so the next frame's render fills it.

use std::sync::Arc;

use joy_engine::development::{
    FileDialogEvent, FileDialogs, FileFilter, MaximizeMode, OpenRequest, PaneLimits, PanelLayout, PanelLayoutOptions,
    SaveRequest, ViewportShortcut, components, viewport_input,
};
use joy_engine::egui::{
    self, Align, Align2, CornerRadius, FontId, Layout, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2,
};
use joy_engine::glam::Vec2;
use joy_engine::particles::decode_particle_texture;
use joy_engine::runtime::hud_text::{self, Style};
use joy_engine::runtime::theme::tokens;
use joy_engine::runtime::{Storage, theme};

use crate::constants::{EmitterPreset, PANEL_LAYOUT_KEY, PerformanceMode, SPEEDS};
use crate::document::export_file_name;
use crate::editor::ParticleScriptEditor;
use crate::emitter_inspector::{EmitterInspector, InspectorAction};
use crate::examples::{EMITTER_PRESETS, EXAMPLES};
use crate::lab::{ParticleLab, PreviewState, ReplayWindow};
use crate::parameter_controls::ParameterPanel;
use crate::viewport::{CameraGesture, PREVIEW_BACKGROUNDS, PreviewRegion};

/// Header height in standalone mode, including the accent rule.
const HEADER_HEIGHT: f32 = 100.0;
/// Project bar height.
const PROJECT_BAR_HEIGHT: f32 = 54.0;
/// Footer height in standalone mode.
const FOOTER_HEIGHT: f32 = 36.0;
/// Panel heading height.
const HEADING_HEIGHT: f32 = 54.0;
/// Smallest preview viewport height.
const MINIMUM_VIEWPORT_HEIGHT: f32 = 140.0;

/// Which compose tab shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComposeTab {
    /// Emitter settings.
    Emitter,
    /// The spawn script.
    Spawn,
    /// The update script.
    Update,
}

/// The replay window menu choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayChoice {
    Full,
    Two,
    Five,
    Custom,
}

/// What a pending file dialog is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogPurpose {
    OpenEffect,
    SaveEffect,
    UploadTexture,
}

/// Interface state that is not document data.
pub struct Interface {
    layout: PanelLayout,
    spawn_editor: ParticleScriptEditor,
    update_editor: ParticleScriptEditor,
    inspector: EmitterInspector,
    parameters: ParameterPanel,
    /// The selected compose tab.
    pub tab: ComposeTab,
    /// Whether the script reference dialog is open.
    pub reference_open: bool,
    /// Whether the emitter chooser is open.
    pub chooser_open: bool,
    dialogs: FileDialogs,
    dialog_purpose: Option<DialogPurpose>,
    notice: Option<String>,
    example_index: usize,
    replay_choice: ReplayChoice,
    custom_replay: f64,
    seed_draft: f64,
    texture_preview: Option<(Arc<str>, egui::TextureHandle)>,
    region: Option<PreviewRegion>,
    hosted: bool,
    backend: String,
}

impl Interface {
    /// Interface state with panel preferences from `storage` (none when hosted).
    pub fn new(storage: Option<Storage>, hosted: bool) -> Self {
        let options = PanelLayoutOptions {
            storage_key: PANEL_LAYOUT_KEY.to_owned(),
            left: PaneLimits { default_width: 320.0, minimum: 160.0, maximum: 600.0 },
            right: PaneLimits { default_width: 260.0, minimum: 220.0, maximum: 480.0 },
            maximize_mode: MaximizeMode::Workspace,
            ..PanelLayoutOptions::default()
        };
        Self {
            layout: PanelLayout::new(options, storage),
            spawn_editor: ParticleScriptEditor::new("particle-lab-spawn"),
            update_editor: ParticleScriptEditor::new("particle-lab-update"),
            inspector: EmitterInspector::default(),
            parameters: ParameterPanel::default(),
            tab: ComposeTab::Emitter,
            reference_open: false,
            chooser_open: false,
            dialogs: FileDialogs::new(),
            dialog_purpose: None,
            notice: None,
            example_index: 0,
            replay_choice: ReplayChoice::Full,
            custom_replay: 2.0,
            seed_draft: crate::constants::DEFAULT_SEED as f64,
            texture_preview: None,
            region: None,
            hosted,
            backend: "GPU …".to_owned(),
        }
    }

    /// Set the renderer label shown in the diagnostics bar (`WEBGPU`, `WEBGL 2`, `VULKAN`, …).
    pub fn set_backend_label(&mut self, label: impl Into<String>) {
        self.backend = label.into();
    }

    /// Switch between standalone chrome and a hosted tab.
    pub fn set_hosted(&mut self, hosted: bool) {
        self.hosted = hosted;
    }

    /// The preview region from the last interface pass.
    pub fn region(&self) -> Option<PreviewRegion> {
        self.region
    }

    /// The file dialogs, for hosts and tests that deliver files.
    pub fn dialogs_mut(&mut self) -> &mut FileDialogs {
        &mut self.dialogs
    }

    /// Start a file dialog request and remember what it is for.
    fn request(&mut self, purpose: DialogPurpose, lab: &mut ParticleLab) {
        let result = match purpose {
            DialogPurpose::OpenEffect => self.dialogs.open(
                OpenRequest::text("Open particle effect", FileFilter::new("Particle effect", &["joyfx", "json"]))
                    .maximum_bytes(crate::document::MAX_ASSET_BYTES),
            ),
            DialogPurpose::UploadTexture => self.dialogs.open(
                OpenRequest::binary("Upload sprite texture", FileFilter::new("PNG image", &["png"]))
                    .maximum_bytes(crate::lab::MAXIMUM_TEXTURE_UPLOAD_BYTES),
            ),
            DialogPurpose::SaveEffect => match lab.export_text() {
                Ok(text) => self.dialogs.save(SaveRequest::text(
                    "Save particle effect",
                    export_file_name(&lab.asset().name),
                    FileFilter::new("Particle effect", &["joyfx"]),
                    text,
                )),
                Err(_) => return,
            },
        };
        match result {
            Ok(()) => self.dialog_purpose = Some(purpose),
            Err(error) => lab.show_error(error.to_string()),
        }
    }

    /// Deliver finished dialog results to the session.
    fn poll_dialogs(&mut self, lab: &mut ParticleLab) {
        let Some(event) = self.dialogs.poll() else {
            return;
        };
        let purpose = self.dialog_purpose.take();
        match (purpose, event) {
            (Some(DialogPurpose::OpenEffect), FileDialogEvent::Opened(file)) => match file.contents.text() {
                Some(text) => {
                    if lab.open_text(text).is_ok() {
                        self.notice = Some(format!("Opened {}", file.name));
                    }
                }
                None => lab.show_error(format!("{} is not text.", file.name)),
            },
            (Some(DialogPurpose::UploadTexture), FileDialogEvent::Opened(file)) => {
                let _ = lab.upload_texture(file.contents.as_bytes());
            }
            (_, FileDialogEvent::Saved { location, .. }) => {
                lab.mark_saved();
                self.notice = Some(format!("Saved {location}"));
            }
            (_, FileDialogEvent::Failed(message)) => lab.show_error(message),
            _ => {}
        }
    }

    /// Draw the whole interface inside `area` (logical points) and apply the author's actions. Returns the preview
    /// region for the next render.
    pub fn show(&mut self, context: &egui::Context, lab: &mut ParticleLab, area: Rect) -> PreviewRegion {
        self.poll_dialogs(lab);
        self.shortcuts(context, lab);
        let viewport_size = context.content_rect().size();
        let mut region = PreviewRegion {
            viewport_size: Vec2::new(viewport_size.x, viewport_size.y),
            min: Vec2::ZERO,
            size: Vec2::ZERO,
        };
        egui::Area::new(egui::Id::new("particle-lab"))
            .fixed_pos(area.min)
            .order(egui::Order::Background)
            .movable(false)
            .show(context, |ui| {
                ui.set_clip_rect(area);
                ui.set_min_size(area.size());
                let chrome = !(self.hosted
                    || (self.layout.maximized() && self.layout.maximize_mode() == MaximizeMode::Workspace));
                let mut top = area.top();
                let mut bottom = area.bottom();
                if chrome {
                    self.header(ui, lab, Rect::from_min_size(area.min, vec2(area.width(), HEADER_HEIGHT)));
                    top += HEADER_HEIGHT;
                    bottom -= FOOTER_HEIGHT;
                    footer(ui, Rect::from_min_max(pos2(area.left(), bottom), area.max));
                }
                if !self.layout.maximized() {
                    self.project_bar(
                        ui,
                        lab,
                        Rect::from_min_size(pos2(area.left(), top), vec2(area.width(), PROJECT_BAR_HEIGHT)),
                    );
                    top += PROJECT_BAR_HEIGHT;
                }
                let workspace = Rect::from_min_max(pos2(area.left(), top), pos2(area.right(), bottom));
                let panes = self.layout.panes(workspace);
                if let Some(rect) = panes.left {
                    pane(ui, rect, |ui| self.compose_panel(ui, lab));
                }
                if let Some(rect) = panes.right {
                    pane(ui, rect, |ui| self.parameter_panel(ui, lab));
                }
                if let Some(rect) = panes.center {
                    let viewport = self.preview_panel(ui, lab, rect);
                    region.min = Vec2::new(viewport.left(), viewport.top());
                    region.size = Vec2::new(viewport.width(), viewport.height());
                }
                self.layout.dividers(ui, &panes);
            });
        self.dialogs_ui(context, lab);
        self.region = Some(region);
        region
    }

    /// Document shortcuts: Ctrl/Cmd+Enter applies; Space toggles playback and R restarts outside text fields (a focused
    /// viewport, button, or slider still takes them, as in the TypeScript page).
    fn shortcuts(&mut self, context: &egui::Context, lab: &mut ParticleLab) {
        let editing = context.text_edit_focused();
        let (apply, space, restart) = context.input_mut(|input| {
            let apply = input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
            let space = !editing && input.consume_key(egui::Modifiers::NONE, egui::Key::Space);
            let restart = !editing && input.consume_key(egui::Modifiers::NONE, egui::Key::R);
            (apply, space, restart)
        });
        if apply {
            lab.apply(false);
        }
        if space {
            lab.toggle_play();
        }
        if restart {
            lab.restart();
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Chrome
    // -----------------------------------------------------------------------------------------------------------------

    fn header(&mut self, ui: &mut Ui, lab: &mut ParticleLab, rect: Rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, tokens::PANEL);
        painter.rect_filled(
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - 4.0), rect.max),
            CornerRadius::ZERO,
            tokens::ACCENT,
        );
        let title = hud_text::single(&painter, "PARTICLE LAB", Style::new(34.0, tokens::ACCENT).heavy().spacing(0.5));
        let title_rect = title.paint_in(
            &painter,
            Rect::from_min_size(rect.min + vec2(30.0, 0.0), vec2(400.0, rect.height() - 4.0)),
            Align::Min,
            Align::Center,
        );
        let version = env!("CARGO_PKG_VERSION");
        let badge = Rect::from_min_size(pos2(title_rect.right() + 10.0, title_rect.top() - 2.0), vec2(40.0, 18.0));
        painter.rect_stroke(badge, CornerRadius::ZERO, Stroke::new(1.0, tokens::ACCENT), egui::StrokeKind::Inside);
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            version,
            FontId::monospace(tokens::TYPE_XS),
            tokens::ACCENT,
        );
        let buttons =
            Rect::from_min_max(pos2(rect.right() - 300.0, rect.top()), pos2(rect.right() - 28.0, rect.bottom() - 4.0));
        ui.scope_builder(UiBuilder::new().max_rect(buttons).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().button_padding = vec2(16.0, 10.0);
            if ui.button(RichText::new("?").strong()).on_hover_text("Script Reference").clicked() {
                self.reference_open = true;
            }
            if ui
                .button(RichText::new("Save .joyfx ↗").strong())
                .on_hover_text("Save the current definition as a portable .joyfx asset")
                .clicked()
            {
                self.request(DialogPurpose::SaveEffect, lab);
            }
            if ui.button(RichText::new("Open").strong()).clicked() {
                self.request(DialogPurpose::OpenEffect, lab);
            }
        });
    }

    fn project_bar(&mut self, ui: &mut Ui, lab: &mut ParticleLab, rect: Rect) {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, tokens::PANEL);
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, tokens::BORDER));
        let inner = rect.shrink2(vec2(28.0, 10.0));
        ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)), |ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter().rect_filled(dot, CornerRadius::ZERO, tokens::ACCENT);
            let mut name = lab.asset().name.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .desired_width(200.0)
                    .char_limit(80)
                    .font(FontId::proportional(tokens::TYPE_MD)),
            );
            if response.changed() {
                lab.asset_mut().name = name;
                lab.document_changed();
            }
            ui.add_space(tokens::SPACE_MD);
            ui.label(RichText::new("PARTICLE ASSET / .JOYFX").monospace().color(tokens::MUTED).size(tokens::TYPE_XS));
            if let Some(notice) = &self.notice {
                ui.add_space(tokens::SPACE_MD);
                ui.label(RichText::new(notice).color(tokens::SUCCESS).size(tokens::TYPE_XS));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let mut selected = self.example_index;
                egui::ComboBox::from_id_salt("examples").width(250.0).selected_text(EXAMPLES[selected].label).show_ui(
                    ui,
                    |ui| {
                        for (index, example) in EXAMPLES.iter().enumerate() {
                            ui.selectable_value(&mut selected, index, example.label);
                        }
                    },
                );
                if selected != self.example_index {
                    if lab.load_example(selected).is_ok() {
                        self.example_index = selected;
                    } else {
                        lab.show_error(joy_engine::development::lab_values::REPAIR_DRAFTS_MESSAGE);
                    }
                }
                ui.label(RichText::new("EXAMPLES").monospace().color(tokens::MUTED).size(tokens::TYPE_XS));
            });
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Compose panel
    // -----------------------------------------------------------------------------------------------------------------

    fn compose_panel(&mut self, ui: &mut Ui, lab: &mut ParticleLab) {
        ui.horizontal(|ui| {
            ui.set_min_height(HEADING_HEIGHT - 20.0);
            components::panel_heading(ui, "01", "Compose");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(components::primary_button("Apply")).on_hover_text("Apply (Ctrl+Enter)").clicked() {
                    lab.apply(false);
                }
                let mut live = lab.live();
                if ui.checkbox(&mut live, "Live").changed() {
                    lab.set_live(live);
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Emitter Stack").monospace().color(tokens::MUTED).size(tokens::TYPE_XS));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if named(ui.button(RichText::new("+ Add").color(tokens::ACCENT)), "Add emitter")
                    .on_hover_text("Add emitter")
                    .clicked()
                {
                    if lab.drafts.require_valid().is_ok() {
                        self.chooser_open = true;
                    } else {
                        lab.show_error(joy_engine::development::lab_values::REPAIR_DRAFTS_MESSAGE);
                    }
                }
            });
        });
        let selected = lab.current_emitter();
        let selected_id = lab.asset().emitters[selected].id.clone();
        components::guidance(ui, format!("Selected emitter: {selected_id} · properties and scripts below"));
        let mut select = None;
        for (index, emitter) in lab.asset().emitters.iter().enumerate() {
            if emitter_row(
                ui,
                index == selected,
                emitter.enabled,
                &emitter.id,
                &format!(
                    "{} · {}",
                    if emitter.root { "ROOT" } else { "CHILD" },
                    emitter.renderer.as_str().to_uppercase()
                ),
            )
            .clicked()
            {
                select = Some(index);
            }
        }
        if let Some(index) = select
            && let Err(message) = lab.select_emitter(index)
        {
            lab.show_error(message);
        }
        ui.horizontal(|ui| {
            if ui.button("Duplicate").clicked()
                && let Err(message) = lab.duplicate_emitter()
            {
                lab.show_error(message);
            }
            if ui.add(components::danger_button("Delete")).clicked() {
                let _ = lab.delete_emitter();
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            for (tab, label) in [
                (ComposeTab::Emitter, "EMITTER"),
                (ComposeTab::Spawn, "SPAWN SCRIPT"),
                (ComposeTab::Update, "UPDATE SCRIPT"),
            ] {
                if components::toggle_button(ui, self.tab == tab, label).clicked() {
                    self.tab = tab;
                }
            }
        });

        let error_height = if lab.error().is_some() { 56.0 } else { 0.0 };
        let available = (ui.available_height() - error_height - 24.0).max(120.0);
        let index = lab.current_emitter();
        match self.tab {
            ComposeTab::Emitter => {
                let texture = self.texture_for(ui.ctx(), lab);
                egui::ScrollArea::vertical()
                    .id_salt("emitter-tab")
                    .max_height(available)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if let Some(InspectorAction::UploadTexture) = self.inspector.show(ui, lab, texture.as_ref()) {
                            self.request(DialogPurpose::UploadTexture, lab);
                        }
                    });
            }
            ComposeTab::Spawn | ComposeTab::Update => {
                let (spawn_diagnostics, update_diagnostics) = lab.script_diagnostics();
                let parameters: Vec<String> = lab.asset().parameters.keys().cloned().collect();
                let owner = (lab.generation(), index);
                let spawn = self.tab == ComposeTab::Spawn;
                let (editor, diagnostics) = if spawn {
                    (&mut self.spawn_editor, spawn_diagnostics)
                } else {
                    (&mut self.update_editor, update_diagnostics)
                };
                let mut text = if spawn {
                    lab.asset().emitters[index].spawn.clone()
                } else {
                    lab.asset().emitters[index].update.clone()
                };
                let response = editor.show(ui, &mut text, owner, parameters, diagnostics, available - 30.0);
                if response.editor.changed {
                    let emitter = &mut lab.asset_mut().emitters[index];
                    if spawn {
                        emitter.spawn = text;
                    } else {
                        emitter.update = text;
                    }
                    lab.script_changed();
                }
                if response.editor.run_requested {
                    lab.apply(false);
                }
                if response.reference_requested {
                    self.reference_open = true;
                }
            }
        }
        if let Some(message) = lab.error() {
            egui::Frame::new().fill(tokens::FIELD).stroke(Stroke::new(1.0, tokens::DANGER)).inner_margin(6).show(
                ui,
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(message).monospace().color(tokens::DANGER).size(tokens::TYPE_SM),
                        )
                        .wrap(),
                    );
                },
            );
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("SPAWN / UPDATE").monospace().color(tokens::MUTED).size(tokens::TYPE_XS));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new("Ctrl / Cmd + Enter to apply")
                            .monospace()
                            .color(tokens::MUTED)
                            .size(tokens::TYPE_XS),
                    );
                });
            });
        });
    }

    /// The selected emitter's sprite thumbnail, decoded once per texture source.
    fn texture_for(&mut self, context: &egui::Context, lab: &ParticleLab) -> Option<egui::TextureHandle> {
        let texture = lab.asset().emitters.get(lab.current_emitter())?.texture.as_ref()?;
        if let Some((source, handle)) = &self.texture_preview
            && Arc::ptr_eq(source, &texture.source)
        {
            return Some(handle.clone());
        }
        let image = decode_particle_texture(&texture.source).ok()?;
        let color =
            egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.pixels());
        let handle = context.load_texture("particle-lab-sprite", color, egui::TextureOptions::LINEAR);
        self.texture_preview = Some((texture.source.clone(), handle.clone()));
        Some(handle)
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Preview panel
    // -----------------------------------------------------------------------------------------------------------------

    /// Draw the preview panel around an open viewport; returns the viewport rectangle.
    fn preview_panel(&mut self, ui: &mut Ui, lab: &mut ParticleLab, rect: Rect) -> Rect {
        let monitor_height = match lab.performance_mode() {
            PerformanceMode::Disabled => 0.0,
            PerformanceMode::Overview => 62.0,
            PerformanceMode::Detailed => 230.0,
        };
        let top_height = HEADING_HEIGHT + 46.0;
        let bottom_height = 40.0 + 40.0 + 46.0 + 46.0 + monitor_height;
        let viewport_height = (rect.height() - top_height - bottom_height).max(MINIMUM_VIEWPORT_HEIGHT);
        let top = Rect::from_min_size(rect.min, vec2(rect.width(), top_height));
        let viewport = Rect::from_min_size(pos2(rect.left(), top.bottom()), vec2(rect.width(), viewport_height));
        let bottom = Rect::from_min_max(pos2(rect.left(), viewport.bottom()), rect.max);

        // Heading and toolbar.
        pane(ui, top, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(HEADING_HEIGHT - 22.0);
                components::panel_heading(ui, "02", "Preview");
                let (text, color) = match (lab.preview_state(), lab.error().is_some()) {
                    (_, true) => ("LIVE · FIX ERROR", tokens::DANGER),
                    (PreviewState::Pending, false) => ("PENDING CHANGES", tokens::ACCENT_HOVER),
                    (PreviewState::Live, false) => ("LIVE", tokens::SUCCESS),
                };
                components::status_badge(ui, text, color);
                // Right to left: the panel toggles, Reset, Frame, then the projection note.
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    self.layout.visibility_controls(ui);
                    if ui.button("Reset").on_hover_text("Reset camera (0)").clicked() {
                        lab.viewport.reset_camera();
                    }
                    if ui.button("Frame F").on_hover_text("Frame current particles (F)").clicked() {
                        lab.frame_current_effect(Vec2::new(viewport.width(), viewport.height()));
                    }
                    ui.label(
                        RichText::new("Y-UP / PERSPECTIVE").monospace().color(tokens::MUTED).size(tokens::TYPE_XS),
                    );
                });
            });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("↺ Restart").on_hover_text("Restart (R)").clicked() {
                    lab.restart();
                }
                let play = if lab.playing() { "⏸ Pause" } else { "▶ Play" };
                if ui.add(components::primary_button(play)).clicked() {
                    lab.toggle_play();
                }
                if ui.button("Step").on_hover_text("Step one 60 Hz tick").clicked() {
                    lab.step();
                }
                ui.label(RichText::new("Seed").color(tokens::MUTED).size(tokens::TYPE_SM));
                let mut drafts = std::mem::take(&mut lab.drafts);
                let response = joy_engine::development::NumberField::new("seed", &mut self.seed_draft)
                    .step(1.0)
                    .width(70.0)
                    .align_left()
                    .label("Seed")
                    .show(ui, &mut drafts);
                lab.drafts = drafts;
                if response.committed() && response.changed {
                    lab.set_seed(self.seed_draft.trunc() as i64);
                }
                ui.label(RichText::new("Speed").color(tokens::MUTED).size(tokens::TYPE_SM));
                let label = SPEEDS.iter().find(|(speed, _)| *speed == lab.speed).map_or("1×", |(_, label)| *label);
                egui::ComboBox::from_id_salt("speed").width(56.0).selected_text(label).show_ui(ui, |ui| {
                    for (speed, label) in SPEEDS {
                        ui.selectable_value(&mut lab.speed, speed, label);
                    }
                });
                ui.checkbox(&mut lab.looping, "Loop");
            });
        });

        // The open viewport: camera input over the engine's render.
        let actions = viewport_input(ui, viewport, egui::Id::new("particle-lab-viewport"));
        if let Some((gesture, delta)) = actions.drag {
            let gesture = if gesture == joy_engine::development::ViewportGesture::Pan {
                CameraGesture::Pan
            } else {
                CameraGesture::Orbit
            };
            lab.viewport.drag(gesture, Vec2::new(delta.x, delta.y), viewport.height());
        }
        if actions.zoom != 0.0 {
            lab.viewport.zoom(actions.zoom);
        }
        for shortcut in actions.shortcuts {
            match shortcut {
                ViewportShortcut::Frame => lab.frame_current_effect(Vec2::new(viewport.width(), viewport.height())),
                ViewportShortcut::Reset => lab.viewport.reset_camera(),
                ViewportShortcut::Left => lab.viewport.nudge(-1.0, 0.0),
                ViewportShortcut::Right => lab.viewport.nudge(1.0, 0.0),
                ViewportShortcut::Up => lab.viewport.nudge(0.0, 1.0),
                ViewportShortcut::Down => lab.viewport.nudge(0.0, -1.0),
            }
        }
        let painter = ui.painter_at(viewport);
        painter.text(
            pos2(viewport.center().x, viewport.bottom() - 16.0),
            Align2::CENTER_CENTER,
            "DRAG TO ORBIT · SHIFT + DRAG TO PAN · SCROLL TO ZOOM",
            FontId::monospace(tokens::TYPE_XS),
            tokens::MUTED.gamma_multiply(0.8),
        );
        if actions.response.has_focus() {
            painter.rect_stroke(
                viewport.shrink(1.0),
                CornerRadius::ZERO,
                Stroke::new(tokens::BORDER_FOCUS, tokens::ACCENT),
                egui::StrokeKind::Inside,
            );
        }

        // Options, replay window, timeline, diagnostics, and the monitor.
        pane(ui, bottom, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut lab.viewport.grid_visible, "Line grid").on_hover_text(
                    "Editor-only lines; minor spacing follows 1/2/5 world units, major lines mark five divisions",
                );
                ui.label(RichText::new("Editor background").size(tokens::TYPE_SM));
                let current = lab.viewport.background();
                egui::ComboBox::from_id_salt("background")
                    .width(90.0)
                    .selected_text(PREVIEW_BACKGROUNDS[current].1)
                    .show_ui(ui, |ui| {
                        for (key, label, _) in PREVIEW_BACKGROUNDS {
                            if ui.selectable_label(PREVIEW_BACKGROUNDS[current].0 == key, label).clicked() {
                                let _ = lab.viewport.set_background(key);
                            }
                        }
                    });
                ui.checkbox(&mut lab.viewport.receiver_visible, "Light receiver (editor only)");
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Replay window").size(tokens::TYPE_SM));
                let mut choice = self.replay_choice;
                let label = match choice {
                    ReplayChoice::Full => "Full effect",
                    ReplayChoice::Two => "2 seconds",
                    ReplayChoice::Five => "5 seconds",
                    ReplayChoice::Custom => "Custom",
                };
                egui::ComboBox::from_id_salt("replay").width(100.0).selected_text(label).show_ui(ui, |ui| {
                    ui.selectable_value(&mut choice, ReplayChoice::Full, "Full effect");
                    ui.selectable_value(&mut choice, ReplayChoice::Two, "2 seconds");
                    ui.selectable_value(&mut choice, ReplayChoice::Five, "5 seconds");
                    ui.selectable_value(&mut choice, ReplayChoice::Custom, "Custom");
                });
                let mut custom_committed = false;
                if choice == ReplayChoice::Custom {
                    let mut drafts = std::mem::take(&mut lab.drafts);
                    let response = joy_engine::development::NumberField::new("replay-custom", &mut self.custom_replay)
                        .range(0.01, 30.0)
                        .step(0.01)
                        .width(64.0)
                        .label("Custom replay seconds")
                        .show(ui, &mut drafts);
                    lab.drafts = drafts;
                    custom_committed = response.committed() && response.changed;
                }
                if choice != self.replay_choice || custom_committed {
                    self.replay_choice = choice;
                    let window = match choice {
                        ReplayChoice::Full => ReplayWindow::Full,
                        ReplayChoice::Two => ReplayWindow::Seconds(2.0),
                        ReplayChoice::Five => ReplayWindow::Seconds(5.0),
                        ReplayChoice::Custom => ReplayWindow::Seconds(self.custom_replay),
                    };
                    if let Err(message) = lab.set_replay_window(window) {
                        lab.show_error(message);
                    }
                }
                components::guidance(ui, "Editor only · restart on change · Loop repeats the window");
            });
            ui.separator();
            ui.horizontal(|ui| {
                let time = lab.time();
                let duration = lab.timeline_seconds();
                ui.label(RichText::new(format!("{time:.2} s")).monospace().color(tokens::MUTED));
                let mut value = time.min(duration);
                let width = (ui.available_width() - 70.0).max(60.0);
                let response =
                    joy_engine::development::lab_slider(ui, &mut value, 0.0..=duration, Some(0.01), vec2(width, 24.0))
                        .on_hover_text("Timeline");
                lab.scrubbing = response.is_pointer_button_down_on() || response.dragged();
                if response.changed() {
                    lab.seek(value);
                }
                ui.label(RichText::new(format!("{duration:.2} s")).monospace().color(tokens::MUTED));
            });
            egui::Frame::new()
                .fill(tokens::CANVAS)
                .stroke(Stroke::new(1.0, tokens::ACCENT))
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let effect = lab.effect();
                        let count = effect.map_or(0, |effect| effect.particles().len());
                        let dropped = effect.map_or(0, |effect| effect.dropped_particles());
                        ui.label(
                            RichText::new(group_thousands(count as u64)).monospace().strong().color(tokens::ACCENT),
                        );
                        ui.label(RichText::new("PARTICLES").monospace().color(tokens::TEXT).size(tokens::TYPE_SM));
                        ui.label(RichText::new(dropped.to_string()).monospace().strong().color(tokens::ACCENT));
                        ui.label(RichText::new("DROPPED").monospace().color(tokens::TEXT).size(tokens::TYPE_SM));
                        ui.label(RichText::new(&self.backend).monospace().color(tokens::ACCENT).size(tokens::TYPE_SM));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let mode = lab.performance_mode();
                            let mut selected = mode;
                            egui::ComboBox::from_id_salt("monitor").width(150.0).selected_text(mode.label()).show_ui(
                                ui,
                                |ui| {
                                    for option in PerformanceMode::ALL {
                                        ui.selectable_value(&mut selected, option, option.label());
                                    }
                                },
                            );
                            if selected != mode {
                                lab.set_performance_mode(selected);
                            }
                            ui.label(RichText::new("MONITOR").monospace().color(tokens::TEXT).size(tokens::TYPE_SM));
                        });
                    });
                });
            if lab.performance_mode() != PerformanceMode::Disabled {
                performance_panel(ui, lab);
            }
        });
        viewport
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Parameter panel
    // -----------------------------------------------------------------------------------------------------------------

    fn parameter_panel(&mut self, ui: &mut Ui, lab: &mut ParticleLab) {
        ui.horizontal(|ui| {
            ui.set_min_height(HEADING_HEIGHT - 20.0);
            components::panel_heading(ui, "03", "Effect Parameters");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if named(ui.button(RichText::new("+ Add").color(tokens::ACCENT)), "Add parameter")
                    .on_hover_text("Add parameter")
                    .clicked()
                    && let Err(message) = lab.add_parameter()
                {
                    lab.show_error(message);
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt("parameters").auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new("SHAPE THE MOTION. FIND THE FEEL.").monospace().color(tokens::ACCENT).size(tokens::TYPE_XS));
            components::guidance(ui, "Whole effect · saved in .joyfx. Save exports the current definition; LIVE marks the accepted audition.");
            ui.horizontal(|ui| {
                if ui.add_enabled(lab.can_undo(), egui::Button::new("Undo property")).clicked() {
                    lab.undo();
                }
                if ui.add_enabled(lab.can_redo(), egui::Button::new("Redo property")).clicked() {
                    lab.redo();
                }
            });
            components::guidance(ui, "Type directly · Shift + arrows fine · Alt + arrows coarse. Script undo is separate.");
            ui.add_space(tokens::SPACE_SM);
            self.parameters.show(ui, lab);
            ui.add_space(tokens::SPACE_LG);
            ui.label(RichText::new("TAKE IT WITH YOU").monospace().strong().color(tokens::TEXT).size(tokens::TYPE_SM));
            components::guidance(ui, "Save a .joyfx asset to use this effect in any Joy Engine project.");
        });
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Dialogs
    // -----------------------------------------------------------------------------------------------------------------

    fn dialogs_ui(&mut self, context: &egui::Context, lab: &mut ParticleLab) {
        if self.reference_open {
            let response = egui::Modal::new(egui::Id::new("particle-lab-reference")).show(context, |ui| {
                ui.set_max_width(640.0);
                reference_contents(ui);
                ui.add_space(tokens::SPACE_MD);
                ui.button("Close").clicked()
            });
            if response.inner || response.should_close() {
                self.reference_open = false;
            }
        }
        if self.chooser_open {
            let response = egui::Modal::new(egui::Id::new("particle-lab-chooser")).show(context, |ui| {
                ui.set_width(360.0);
                ui.heading("Add emitter");
                ui.label("Insert one emitter into this effect.");
                let mut chosen = None;
                for info in EMITTER_PRESETS {
                    let text = format!("{}\n{}", info.label, info.description);
                    if ui.add_sized([340.0, 44.0], egui::Button::new(text)).clicked() {
                        chosen = Some(info.preset);
                    }
                }
                let cancelled = ui.button("Cancel").clicked();
                (chosen, cancelled)
            });
            let (chosen, cancelled): (Option<EmitterPreset>, bool) = response.inner;
            if let Some(preset) = chosen {
                if let Err(message) = lab.add_emitter(preset) {
                    lab.show_error(message);
                }
                self.chooser_open = false;
            }
            if cancelled || response.should_close() {
                self.chooser_open = false;
            }
        }
    }
}

/// Give a widget an accessible name other than its visible text (two buttons both read `+ Add`).
fn named(response: egui::Response, name: &str) -> egui::Response {
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
    response
}

/// Draw `contents` in an opaque panel filling `rect`.
fn pane(ui: &mut Ui, rect: Rect, contents: impl FnOnce(&mut Ui)) {
    ui.painter().rect_filled(rect, CornerRadius::ZERO, tokens::PANEL);
    ui.painter().rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, tokens::BORDER), egui::StrokeKind::Inside);
    let inner = rect.shrink(tokens::SPACE_MD);
    ui.scope_builder(UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        contents(ui);
    });
}

/// The footer strip.
fn footer(ui: &mut Ui, rect: Rect) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, tokens::PANEL);
    painter.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, tokens::BORDER));
    painter.text(
        rect.left_center() + vec2(28.0, 0.0),
        Align2::LEFT_CENTER,
        "JOY-GAMES / PARTICLE LAB",
        FontId::monospace(tokens::TYPE_SM),
        tokens::MUTED,
    );
    painter.text(
        rect.right_center() - vec2(28.0, 0.0),
        Align2::RIGHT_CENTER,
        "LOCAL WORKSPACE · NO CLOUD REQUIRED",
        FontId::monospace(tokens::TYPE_SM),
        tokens::MUTED,
    );
}

/// One emitter stack row.
fn emitter_row(ui: &mut Ui, selected: bool, enabled: bool, id: &str, detail: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, id));
    let painter = ui.painter_at(rect);
    let fill = if selected {
        tokens::SURFACE
    } else if response.hovered() {
        tokens::BUTTON
    } else {
        tokens::FIELD
    };
    painter.rect_filled(rect, CornerRadius::ZERO, fill);
    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, tokens::BORDER), egui::StrokeKind::Inside);
    if selected {
        painter.rect_filled(
            Rect::from_min_size(rect.min, vec2(tokens::BORDER_ACCENT * 0.5, rect.height())),
            CornerRadius::ZERO,
            tokens::ACCENT,
        );
    }
    painter.circle_filled(
        rect.left_center() + vec2(24.0, 0.0),
        3.0,
        if enabled { tokens::ACCENT } else { tokens::BORDER },
    );
    painter.text(
        rect.left_center() + vec2(44.0, 0.0),
        Align2::LEFT_CENTER,
        id,
        FontId::proportional(tokens::TYPE_MD),
        tokens::TEXT,
    );
    painter.text(
        rect.right_center() - vec2(10.0, 0.0),
        Align2::RIGHT_CENTER,
        detail,
        FontId::monospace(tokens::TYPE_XS),
        tokens::MUTED,
    );
    response
}

/// The monitor's summary and, in detailed profiling, the per-emitter table.
fn performance_panel(ui: &mut Ui, lab: &ParticleLab) {
    let stats = lab.viewport.stats();
    let view = lab.performance();
    let fps = if lab.playing() {
        view.preview_fps.map_or("—".to_owned(), |fps| format!("{fps:.0}"))
    } else {
        "Paused".to_owned()
    };
    let lights = if stats.omitted_lights > 0 {
        format!("{} · {} omitted (preview light cap)", stats.lights, stats.omitted_lights)
    } else {
        stats.lights.to_string()
    };
    let item = |ui: &mut Ui, label: &str, value: String| {
        ui.label(RichText::new(label).monospace().color(tokens::TEXT).size(tokens::TYPE_SM));
        ui.label(RichText::new(value).monospace().strong().color(tokens::CREAM).size(tokens::TYPE_SM));
    };
    egui::Frame::new().fill(tokens::CANVAS).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            item(ui, "Range", stats.range.clone());
            item(ui, "Preview FPS", fps);
            item(ui, "Simulation CPU", format!("{:.2} ms", view.simulation_ms));
            item(ui, "Geometry CPU", format!("{:.2} ms", stats.geometry_ms));
            item(ui, "Submit CPU", format!("{:.2} ms", stats.submission_ms));
            item(ui, "Scene batches", stats.scene_batches.to_string());
            item(ui, "Triangles", group_thousands(stats.triangles as u64));
            item(ui, "Lights", lights);
        });
        if lab.performance_mode() == PerformanceMode::Detailed {
            ui.label(
                RichText::new(format!(
                    "Sort + batch CPU {:.2} ms · Vertices {} · Vertex payload {:.1} KiB (excludes uniforms and textures)",
                    stats.sort_ms,
                    group_thousands(stats.vertices as u64),
                    stats.upload_bytes as f64 / 1024.0
                ))
                .monospace()
                .size(tokens::TYPE_XS),
            );
            components::guidance(ui, "CPU timings only; GPU time unavailable. Spawn/update: ms per simulation step; geometry: last render. Totals start when detailed profiling is enabled and reset on restart, seek or loop. Detailed profiling adds per-particle timing overhead.");
            egui::ScrollArea::both().id_salt("emitter-performance").max_height(110.0).show(ui, |ui| {
                egui::Grid::new("emitter-performance-table").striped(true).show(ui, |ui| {
                    for heading in ["Emitter", "Live", "Spawn CPU", "Update CPU", "Geometry CPU", "Triangles", "Lights", "Spawned Total", "Dropped Total", "Operations Total"] {
                        ui.label(RichText::new(heading).monospace().size(tokens::TYPE_XS).color(tokens::MUTED));
                    }
                    ui.end_row();
                    for row in &view.emitters {
                        for cell in [
                            row.id.clone(),
                            row.live.to_string(),
                            format!("{:.3}", row.spawn_ms),
                            format!("{:.3}", row.update_ms),
                            format!("{:.3}", row.geometry_ms),
                            row.triangles.to_string(),
                            row.lights.to_string(),
                            row.spawned.to_string(),
                            row.dropped.to_string(),
                            group_thousands(row.operations.max(0) as u64),
                        ] {
                            ui.label(RichText::new(cell).monospace().size(tokens::TYPE_XS));
                        }
                        ui.end_row();
                    }
                });
            });
        }
    });
}

/// `1033` as `1,033`.
fn group_thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// The Script Reference dialog's contents (the TypeScript `#reference-dialog`).
fn reference_contents(ui: &mut Ui) {
    let code = |text: &str| RichText::new(text).monospace().color(tokens::SCRIPT_FUNCTION);
    ui.label(RichText::new("BOUNDED PARTICLE LANGUAGE").monospace().color(tokens::ACCENT).size(tokens::TYPE_XS));
    ui.heading("Script Reference");
    egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
        let paragraph = |ui: &mut Ui, text: &str| {
            ui.add(egui::Label::new(text).wrap());
        };
        paragraph(ui, "Particles use XYZ world space, with Y up in the perspective preview. Size is in world units; rotation and spin roll the camera-facing sprite. PARTICLE LAB uses its own deterministic language, not JavaScript.");
        ui.add_space(tokens::SPACE_SM);
        ui.label(RichText::new("Particle Values").strong());
        paragraph(ui, "Writable containers: position, velocity, and acceleration are vectors with .x, .y, .z components. color has .r, .g, .b, .a components.");
        ui.label(code("velocity = vector(0, 80, 0);   color = rgba(1, 0.4, 0.1, 1);   position = origin + vector(0, 10, 0);   velocity += acceleration * dt;"));
        paragraph(ui, "Component writes such as color.a = 1 - t; remain available.");
        paragraph(ui, "Writable scalars: drag, size, rotation, spin, lifetime, lightIntensity, lightRange, and u0–u3.");
        paragraph(ui, "u0–u3 are four custom numeric storage slots per particle. They start at zero and persist from spawn through updates; use them to remember an initial size, random phase, or other authored state. Local let values last only for that script invocation.");
        paragraph(ui, "Read only: origin is the root or parent spawn position; parentVelocity is the parent's velocity at child birth (zero for roots). Both are vectors. Other inputs are dt, age, t, index, time, pi, tau, and named properties.");
        paragraph(ui, "Vector and color properties use the same component accessors, for example wind.x or tint.a, and can be copied whole: color = tint; Color properties use RGBA in 0–1; scripted particle RGB may exceed one for HDR brightness. Alpha is opacity. Optional lights inherit particle RGB and alpha.");
        paragraph(ui, "Existing version-1 scalar spellings such as vx, alpha, originX, parentVx, and vector-property X/Y/Z suffixes remain readable for saved assets.");
        ui.add_space(tokens::SPACE_SM);
        ui.label(RichText::new("Expressions").strong());
        paragraph(ui, "Use let, assignments, += -= *= /=, and if (...) { ... } else { ... }. Operators: + - * / % ^ < > <= >= == != && || !.");
        paragraph(ui, "Functions: sin, cos, tan, abs, sqrt, floor, ceil, round, exp, log, min, max, pow, atan2, clamp, mix, smoothstep, noise, rand, vector, and rgba.");
        paragraph(ui, "Vectors/colors support same-type addition and subtraction, scalar multiplication and division, and mix(a, b, t). Assignments copy values. Conditions, comparisons, and other math functions use scalars.");
        ui.add_space(tokens::SPACE_SM);
        ui.label(RichText::new("Children and Limits").strong());
        paragraph(ui, "Root emitters run their timeline. Trail and death links spawn particles directly from a child template, inherit parent velocity inputs, and stop after four child generations. Assets allow up to 4,096 live particles, 128 bursts per emitter, 30 seconds per timeline/lifetime, and bounded spawn work. Loops, arbitrary objects, strings, dynamic indexing, and browser or host calls are unavailable.");
    });
}

/// Apply the authoring look once per context.
pub fn apply_look(context: &egui::Context) {
    let applied = context.data(|data| data.get_temp::<bool>(egui::Id::new("particle-lab-look"))).unwrap_or(false);
    if !applied {
        theme::apply_authoring(context);
        context.data_mut(|data| data.insert_temp(egui::Id::new("particle-lab-look"), true));
    }
}
