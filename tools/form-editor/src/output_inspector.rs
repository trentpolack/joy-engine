// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Output inspector's controls (UI module): the SHOW menu above the preview (every output together, or one named
//! `output` or `inspect` snapshot) with a note on how it relates to Export, and the ATTRIBUTES panel below it (a button
//! that opens its domain and visualization menus, paging, selection readout, and a read-only table whose index buttons
//! select elements).

use joy_engine::development::components;
use joy_engine::egui::{
    self,
    RichText,
    Ui,
};
use joy_engine::runtime::theme::{
    colors,
    layout,
};

use crate::output_inspection::{
    ALL_OUTPUTS_LABEL,
    OutputInspection,
};
use crate::rendering::viewport::Viewport;

/// The SHOW toolbar above the preview. Returns the preview's error when it rejects the chosen output.
pub fn result_toolbar(ui: &mut Ui, inspection: &mut OutputInspection, viewport: &mut Viewport) -> Option<String> {
    let mut failure = None;
    ui.horizontal_wrapped(|ui| {
        let caption = ui.label(RichText::new("SHOW").color(colors::MUTED).size(layout::TYPE_XS));
        let choices = inspection.output_choices();
        let current = choices
            .iter()
            .find(|(name, _)| name == inspection.output())
            .map_or_else(|| ALL_OUTPUTS_LABEL.to_owned(), |(_, label)| label.clone());
        let mut chosen = None;
        ui.add_enabled_ui(!inspection.pending(), |ui| {
            egui::ComboBox::from_id_salt("output-select")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (name, label) in &choices {
                        if ui.selectable_label(name == inspection.output(), label).clicked() {
                            chosen = Some(name.clone());
                        }
                    }
                })
                .response
                .labelled_by(caption.id)
                .on_hover_text("Preview every output together, or one named output() or inspect() snapshot");
        });
        if let Some(name) = chosen
            && let Err(error) = inspection.choose_output(&name, viewport)
        {
            failure = Some(error);
        }
        ui.label(RichText::new(inspection.kind()).color(colors::MUTED).size(layout::TYPE_XS));
    });
    failure
}

/// The ATTRIBUTES panel below the preview; `reveal` opens it (a viewport pick selected an element).
pub fn attribute_panel(ui: &mut Ui, inspection: &mut OutputInspection, viewport: &mut Viewport, reveal: bool) {
    let id = ui.make_persistent_id("form-editor-attributes");
    let mut state = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
    if reveal {
        state.set_open(true);
    }
    // The whole heading is one button (not just egui's small arrow), so the panel opens wherever the author clicks.
    let open = state.is_open();
    ui.horizontal(|ui| {
        let (arrow, hint) =
            if open { ("▼", "Hide the attribute table") } else { ("▶", "Show the attribute table") };
        let toggle = ui.add(components::button(format!("{arrow}  ATTRIBUTES")).selected(open)).on_hover_text(hint);
        if toggle.clicked() {
            state.toggle(ui);
        }
        ui.label(RichText::new(inspection.summary()).color(colors::MUTED).size(layout::TYPE_XS).monospace());
    });
    state.store(ui.ctx());
    if state.is_open() {
        attribute_body(ui, inspection, viewport);
    }
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
            ui.label(RichText::new("VIEW").color(colors::MUTED).size(layout::TYPE_XS));
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
            ui.label(RichText::new(inspection.selection_info()).color(colors::TEXT).size(layout::TYPE_XS));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(RichText::new(inspection.range()).color(colors::MUTED).size(layout::TYPE_XS));
            });
        });
        let headings = inspection.headings();
        let rows = inspection.rows();
        let selected = inspection.selected();
        let mut clicked = None;
        egui::ScrollArea::both().id_salt("attribute-scroll").max_height(160.0).show(ui, |ui| {
            egui::Grid::new("attribute-table").striped(true).spacing([12.0, 2.0]).show(ui, |ui| {
                for heading in &headings {
                    ui.label(RichText::new(heading).color(colors::MUTED).size(layout::TYPE_XS).strong());
                }
                ui.end_row();
                for (index, cells) in &rows {
                    let selected_row = selected == Some(*index);
                    let button = egui::Button::new(RichText::new(index.to_string()).monospace().size(layout::TYPE_XS))
                        .selected(selected_row)
                        .small();
                    if ui.add(button).on_hover_text(format!("Select {} {index}", inspection.domain().name())).clicked()
                    {
                        clicked = Some(*index);
                    }
                    for (cell, heading) in cells.iter().zip(headings.iter().skip(1)) {
                        ui.label(RichText::new(cell).monospace().size(layout::TYPE_XS))
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
