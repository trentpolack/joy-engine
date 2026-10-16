// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The workbench's panes and inspector tabs (UI module). Ported from `workspace-layout.ts` and the editor's
//! `PanelLayout` setup in `main.ts`: the authoring kit's `PanelLayout` places Script, Preview, and Inspector with
//! persisted widths, the View menu hides or restores each panel (keeping at least one), and Maximize keeps the panes
//! while the tool hides its own chrome. Preferences never alter a study.

use joy_engine::development::{MaximizeMode, PaneLimits, PanelLayout, PanelLayoutOptions};
use joy_engine::egui::{self, Ui};
use joy_engine::runtime::Storage;

/// Storage key of the pane widths and visibility, shared with the TypeScript tool.
pub const PANEL_STORAGE_KEY: &str = "joy-editor-panels-form-v2";

/// The workbench layout: Script (320 points by default), Preview, and Inspector (260).
pub fn create_panel_layout(storage: Option<Storage>) -> PanelLayout {
    PanelLayout::new(
        PanelLayoutOptions {
            storage_key: PANEL_STORAGE_KEY.to_owned(),
            left: PaneLimits { default_width: 320.0, minimum: 160.0, maximum: 600.0 },
            right: PaneLimits { default_width: 260.0, minimum: 220.0, maximum: 480.0 },
            center_collapsible: true,
            labels: ["Script", "Preview", "Inspector"],
            maximize_mode: MaximizeMode::Workspace,
            ..PanelLayoutOptions::default()
        },
        storage,
    )
}

/// Inspector sections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InspectorTab {
    /// Script parameters.
    #[default]
    Parameters,
    /// Named override sets.
    Variants,
    /// The document material palette.
    Materials,
    /// Preview-only lighting.
    Lighting,
}

impl InspectorTab {
    /// Every tab in order.
    pub const ALL: [Self; 4] = [Self::Parameters, Self::Variants, Self::Materials, Self::Lighting];

    /// The tab label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Parameters => "Parameters",
            Self::Variants => "Variants",
            Self::Materials => "Materials",
            Self::Lighting => "Lighting",
        }
    }
}

/// The inspector's tab strip; Left, Right, Home, and End move between tabs while one has focus.
pub fn inspector_tabs(ui: &mut Ui, tab: &mut InspectorTab) {
    use joy_engine::runtime::theme::tokens;
    ui.horizontal(|ui| {
        let gap = 4.0;
        ui.spacing_mut().item_spacing.x = gap;
        let width = ((ui.available_width() - (gap * 3.0)) / 4.0).max(40.0);
        for candidate in InspectorTab::ALL {
            let selected = *tab == candidate;
            let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
            let fill = if selected { tokens::SURFACE } else { tokens::BUTTON };
            let border = if selected || response.hovered() { tokens::ACCENT } else { tokens::BORDER };
            let painter = ui.painter();
            painter.rect_filled(rect, 0.0, fill);
            painter.rect_stroke(rect, 0.0, egui::Stroke::new(1.0, border), egui::StrokeKind::Inside);
            let color = if selected { tokens::ACCENT_HOVER } else { tokens::TEXT };
            painter.with_clip_rect(rect).text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                candidate.label(),
                egui::FontId::proportional(tokens::TYPE_SM),
                color,
            );
            if response.clicked() {
                *tab = candidate;
                response.request_focus();
            }
            if response.has_focus() {
                let index = InspectorTab::ALL.iter().position(|item| *item == *tab).unwrap_or(0);
                let next = ui.input(|input| {
                    if input.key_pressed(egui::Key::ArrowRight) {
                        Some((index + 1) % 4)
                    } else if input.key_pressed(egui::Key::ArrowLeft) {
                        Some((index + 3) % 4)
                    } else if input.key_pressed(egui::Key::Home) {
                        Some(0)
                    } else if input.key_pressed(egui::Key::End) {
                        Some(3)
                    } else {
                        None
                    }
                });
                if let Some(next) = next {
                    *tab = InspectorTab::ALL[next];
                }
            }
        }
    });
}

/// The header's View menu: one toggle per panel (at least one stays visible) and Maximize.
pub fn view_menu(ui: &mut Ui, layout: &mut PanelLayout) -> egui::Response {
    ui.menu_button("View", |ui| {
        ui.set_min_width(150.0);
        layout.visibility_controls(ui);
    })
    .response
}
