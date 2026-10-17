// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Output inspector's controls (UI module): the RESULT menu and kind above the
//! preview, and the collapsible ATTRIBUTES panel with its domain and visualization menus, paging, selection readout,
//! and a read-only table whose index buttons select elements.

use joy_engine::egui::{
    self,
    RichText,
    Ui,
};
use joy_engine::runtime::theme::tokens;

use crate::output_inspection::OutputInspection;
use crate::rendering::viewport::Viewport;

/// The RESULT toolbar above the preview. Returns the preview's error when it rejects the chosen output.
pub fn result_toolbar(ui: &mut Ui, inspection: &mut OutputInspection, viewport: &mut Viewport) -> Option<String> {
    let mut failure = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new("RESULT").color(tokens::MUTED).size(tokens::TYPE_XS));
        let choices = inspection.output_choices();
        let current = choices
            .iter()
            .find(|(name, _)| name == inspection.output())
            .map_or_else(|| "All outputs".to_owned(), |(_, label)| label.clone());
        let mut chosen = None;
        ui.add_enabled_ui(!inspection.pending(), |ui| {
            egui::ComboBox::from_id_salt("output-select").selected_text(current).show_ui(ui, |ui| {
                for (name, label) in &choices {
                    if ui.selectable_label(name == inspection.output(), label).clicked() {
                        chosen = Some(name.clone());
                    }
                }
            })
        });
        if let Some(name) = chosen
            && let Err(error) = inspection.choose_output(&name, viewport)
        {
            failure = Some(error);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(inspection.kind()).color(tokens::MUTED).size(tokens::TYPE_XS).monospace());
        });
    });
    failure
}

/// The ATTRIBUTES panel below the preview; `reveal` opens it (a viewport pick selected an element).
pub fn attribute_panel(ui: &mut Ui, inspection: &mut OutputInspection, viewport: &mut Viewport, reveal: bool) {
    let id = ui.make_persistent_id("form-lab-attributes");
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    if reveal {
        state.set_open(true);
    }
    let summary = inspection.summary();
    state
        .show_header(ui, |ui| {
            ui.label(RichText::new("ATTRIBUTES").strong().size(tokens::TYPE_SM));
            ui.label(RichText::new(summary).color(tokens::MUTED).size(tokens::TYPE_XS).monospace());
        })
        .body(|ui| attribute_body(ui, inspection, viewport));
}

/// Menus, paging, readouts, and the table.
fn attribute_body(ui: &mut Ui, inspection: &mut OutputInspection, viewport: &mut Viewport) {
    ui.add_enabled_ui(!inspection.pending(), |ui| {
        ui.horizontal(|ui| {
            let domains = inspection.domains();
            let mut domain = inspection.domain();
            egui::ComboBox::from_id_salt("attribute-domain").selected_text(domain.name().to_uppercase()).show_ui(
                ui,
                |ui| {
                    for candidate in &domains {
                        ui.selectable_value(&mut domain, *candidate, candidate.name().to_uppercase());
                    }
                },
            );
            if domain != inspection.domain() {
                inspection.choose_domain(domain, viewport);
            }
            ui.label(RichText::new("VIEW").color(tokens::MUTED).size(tokens::TYPE_XS));
            let views = inspection.view_names();
            let mut view = inspection.view().to_owned();
            let label = |name: &str| if name.is_empty() { "Material".to_owned() } else { name.to_owned() };
            egui::ComboBox::from_id_salt("attribute-view").selected_text(label(&view)).show_ui(ui, |ui| {
                for candidate in &views {
                    ui.selectable_value(&mut view, candidate.clone(), label(candidate));
                }
            });
            if view != inspection.view() {
                inspection.choose_view(&view, viewport);
            }
            if ui
                .add_enabled(inspection.has_previous_page(), egui::Button::new("←"))
                .on_hover_text("Previous attribute page")
                .clicked()
            {
                inspection.previous_page();
            }
            if ui
                .add_enabled(inspection.has_next_page(), egui::Button::new("→"))
                .on_hover_text("Next attribute page")
                .clicked()
            {
                inspection.next_page();
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(inspection.selection_info()).color(tokens::TEXT).size(tokens::TYPE_XS));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(inspection.range()).color(tokens::MUTED).size(tokens::TYPE_XS));
            });
        });
        let headings = inspection.headings();
        let rows = inspection.rows();
        let selected = inspection.selected();
        let mut clicked = None;
        egui::ScrollArea::both().id_salt("attribute-scroll").max_height(160.0).show(ui, |ui| {
            egui::Grid::new("attribute-table").striped(true).spacing([12.0, 2.0]).show(ui, |ui| {
                for heading in &headings {
                    ui.label(RichText::new(heading).color(tokens::MUTED).size(tokens::TYPE_XS).strong());
                }
                ui.end_row();
                for (index, cells) in &rows {
                    let selected_row = selected == Some(*index);
                    let button = egui::Button::new(RichText::new(index.to_string()).monospace().size(tokens::TYPE_XS))
                        .selected(selected_row)
                        .small();
                    if ui.add(button).on_hover_text(format!("Select {} {index}", inspection.domain().name())).clicked()
                    {
                        clicked = Some(*index);
                    }
                    for (cell, heading) in cells.iter().zip(headings.iter().skip(1)) {
                        ui.label(RichText::new(cell).monospace().size(tokens::TYPE_XS))
                            .on_hover_text(format!("{heading}: {cell}"));
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(index) = clicked {
            inspection.select(Some(index), viewport);
        }
    });
}
