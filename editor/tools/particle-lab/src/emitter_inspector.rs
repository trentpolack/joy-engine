// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The EMITTER tab of the compose panel (the TypeScript `refreshInspector`/`editEmitterInput` markup and handlers):
//! identity, renderer, timing, the sprite texture for textured renderers, particle light emission, bursts, and child
//! emission. Numeric fields are kit number fields, so they share drafts, keyboard steps, and property undo with the
//! parameter panel; switches and menus are structural edits.

use joy_engine::development::{ControlResponse, NumberField, NumericDrafts, components};
use joy_engine::egui::{self, RichText, Ui};
use joy_engine::particles::ParticleRenderer;
use joy_engine::runtime::theme::tokens;

use crate::document::{self, ChildLink};
use crate::lab::ParticleLab;

/// What the inspector asks its owner to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectorAction {
    /// Choose a PNG for the selected emitter's texture.
    UploadTexture,
}

/// Inspector state that is not document data.
#[derive(Default)]
pub struct EmitterInspector {
    id_draft: String,
    id_owner: Option<(u64, usize, String)>,
}

/// Collected edits from one frame.
#[derive(Default)]
struct Edits {
    changed: bool,
    committed: bool,
    structural: bool,
}

impl Edits {
    fn control(&mut self, response: &ControlResponse) {
        self.changed |= response.changed;
        self.committed |= response.committed();
    }
}

impl EmitterInspector {
    /// Draw the selected emitter's settings and apply edits to `lab`. `texture` is the decoded sprite preview, when the
    /// emitter has a texture.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        lab: &mut ParticleLab,
        texture: Option<&egui::TextureHandle>,
    ) -> Option<InspectorAction> {
        let index = lab.current_emitter();
        let generation = lab.generation();
        let others: Vec<String> = lab
            .asset()
            .emitters
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, emitter)| emitter.id.clone())
            .collect();
        let current_id = lab.asset().emitters[index].id.clone();
        if self.id_owner.as_ref() != Some(&(generation, index, current_id.clone())) {
            self.id_draft = current_id.clone();
            self.id_owner = Some((generation, index, current_id.clone()));
        }
        let mut action = None;
        let mut edits = Edits::default();
        let mut rename = None;
        let mut renderer_change = None;
        let mut light_change = None;
        let mut child_changes = Vec::new();
        let mut delete_burst = None;
        let mut add_burst = false;

        let column = ((ui.available_width() - tokens::SPACE_MD) * 0.5).max(80.0);
        {
            let (asset, drafts) = lab.edit_parts();
            let emitter = &mut asset.emitters[index];
            ui.push_id(("emitter", generation, index), |ui| {
                // Identity, renderer, and timing.
                ui.horizontal_top(|ui| {
                    field(ui, column, "ID", |ui| {
                        if ui.add(egui::TextEdit::singleline(&mut self.id_draft).desired_width(column)).changed() {
                            rename = Some(self.id_draft.clone());
                        }
                    });
                    field(ui, column, "Renderer", |ui| {
                        egui::ComboBox::from_id_salt("renderer").width(column - 8.0).selected_text(emitter.renderer.as_str()).show_ui(ui, |ui| {
                            for renderer in ParticleRenderer::ALL {
                                if ui.selectable_label(renderer == emitter.renderer, renderer.as_str()).clicked() && renderer != emitter.renderer {
                                    renderer_change = Some(renderer);
                                }
                            }
                        });
                    });
                });
                ui.horizontal_top(|ui| {
                    field(ui, column, "Duration (s)", |ui| {
                        edits.control(&NumberField::new("duration", &mut emitter.duration).range(0.0, 30.0).step(0.01).width(column).show(ui, drafts));
                    });
                    field(ui, column, "Rate / s", |ui| {
                        edits.control(&NumberField::new("rate", &mut emitter.rate).range(0.0, 2000.0).step(1.0).width(column).show(ui, drafts));
                    });
                });
                ui.horizontal_top(|ui| {
                    field(ui, column, "Lifetime (s)", |ui| {
                        edits.control(&NumberField::new("lifetime", &mut emitter.lifetime).range(0.01, 30.0).step(0.01).width(column).show(ui, drafts));
                    });
                    field(ui, column, "", |ui| {
                        edits.structural |= ui.checkbox(&mut emitter.enabled, "Enabled").changed();
                    });
                });
                edits.structural |= ui.checkbox(&mut emitter.root, "Root Emitter").changed();

                // Sprite texture.
                if emitter.renderer.is_textured() {
                    section(ui, "Sprite Texture");
                    ui.horizontal(|ui| {
                        if let Some(texture) = texture {
                            let size = egui::vec2(48.0, 48.0);
                            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                            joy_engine::development::paint_checkerboard(ui, rect);
                            ui.painter().image(texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                        }
                        if ui.button("Upload PNG").clicked() {
                            action = Some(InspectorAction::UploadTexture);
                        }
                    });
                    components::guidance(ui, "PNG · up to 1 MiB · 2048 × 2048 pixels");
                    if let Some(texture) = &mut emitter.texture {
                        ui.horizontal_top(|ui| {
                            field(ui, column, "columns", |ui| edits.control(&integer_field(ui, "columns", &mut texture.columns, column, drafts)));
                            field(ui, column, "rows", |ui| edits.control(&integer_field(ui, "rows", &mut texture.rows, column, drafts)));
                        });
                        ui.horizontal_top(|ui| {
                            field(ui, column, "frames", |ui| edits.control(&integer_field(ui, "frames", &mut texture.frames, column, drafts)));
                            field(ui, column, "Frames / s", |ui| {
                                edits.control(&NumberField::new("fps", &mut texture.fps).minimum(0.0).step(1.0).width(column).show(ui, drafts));
                            });
                        });
                        edits.structural |= ui.checkbox(&mut texture.looping, "Loop Animation").changed();
                    }
                }

                // Particle light.
                section(ui, "Particle Light");
                let mut emits = emitter.light.is_some();
                if ui.checkbox(&mut emits, "Emit Light").changed() {
                    light_change = Some(emits);
                }
                components::guidance(ui, "Each particle inherits its RGB color and alpha. Scripts can animate lightIntensity and lightRange.");
                if let Some(light) = &mut emitter.light {
                    ui.horizontal_top(|ui| {
                        field(ui, column, "Intensity", |ui| {
                            edits.control(&NumberField::new("light-intensity", &mut light.intensity).range(0.0, 1000.0).step(0.1).width(column).show(ui, drafts));
                        });
                        field(ui, column, "Range", |ui| {
                            edits.control(&NumberField::new("light-range", &mut light.range).range(0.01, 10000.0).step(1.0).width(column).show(ui, drafts));
                        });
                    });
                }

                // Bursts.
                section(ui, "Bursts");
                for (burst_index, burst) in emitter.bursts.iter_mut().enumerate() {
                    ui.push_id(("burst", burst_index), |ui| {
                        ui.horizontal(|ui| {
                            edits.control(&NumberField::new("time", &mut burst.time).minimum(0.0).step(0.01).width(72.0).label("Burst time").show(ui, drafts));
                            ui.label(RichText::new("s").color(tokens::MUTED));
                            edits.control(&integer_field(ui, "count", &mut burst.count, 72.0, drafts));
                            if ui.small_button("×").on_hover_text("Delete burst").clicked() {
                                delete_burst = Some(burst_index);
                            }
                        });
                    });
                }
                add_burst = ui.button("+ Add Burst").clicked();

                // Child emission.
                section(ui, "Child Emission");
                ui.horizontal_top(|ui| {
                    field(ui, column, "Trail Target", |ui| {
                        if let Some(target) = target_menu(ui, "trail-target", emitter.trail.as_ref().map(|trail| trail.emitter.as_str()), &others, column) {
                            child_changes.push((ChildLink::Trail, target));
                        }
                    });
                    field(ui, column, "Interval", |ui| {
                        ui.add_enabled_ui(emitter.trail.is_some(), |ui| match &mut emitter.trail {
                            Some(trail) => edits.control(&NumberField::new("trail-interval", &mut trail.interval).minimum(0.001).step(0.01).width(column).show(ui, drafts)),
                            None => disabled_number(ui, "0.1", column),
                        });
                    });
                });
                ui.horizontal_top(|ui| {
                    field(ui, column, "Trail Count", |ui| {
                        ui.add_enabled_ui(emitter.trail.is_some(), |ui| match &mut emitter.trail {
                            Some(trail) => edits.control(&integer_field(ui, "trail-count", &mut trail.count, column, drafts)),
                            None => disabled_number(ui, "1", column),
                        });
                    });
                    field(ui, column, "Death Target", |ui| {
                        if let Some(target) = target_menu(ui, "death-target", emitter.death.as_ref().map(|death| death.emitter.as_str()), &others, column) {
                            child_changes.push((ChildLink::Death, target));
                        }
                    });
                });
                field(ui, column, "Death Count", |ui| {
                    ui.add_enabled_ui(emitter.death.is_some(), |ui| match &mut emitter.death {
                        Some(death) => edits.control(&integer_field(ui, "death-count", &mut death.count, column, drafts)),
                        None => disabled_number(ui, "1", column),
                    });
                });
            });
        }

        // Apply structural edits through the document, then record.
        let mut structural = edits.structural;
        if let Some(next) = rename {
            match document::rename_emitter(lab.asset_mut(), index, &next) {
                Ok(()) => {
                    self.id_owner = Some((generation, index, next));
                    structural = true;
                }
                Err(message) => {
                    lab.show_error(message);
                    self.id_draft = current_id;
                }
            }
        }
        if let Some(renderer) = renderer_change {
            document::set_renderer(&mut lab.asset_mut().emitters[index], renderer);
            structural = true;
        }
        if let Some(enabled) = light_change {
            document::set_light_enabled(&mut lab.asset_mut().emitters[index], enabled);
            structural = true;
        }
        for (link, target) in child_changes {
            document::set_child_target(&mut lab.asset_mut().emitters[index], link, target.as_deref());
            structural = true;
        }
        if (add_burst || delete_burst.is_some()) && lab.drafts.require_valid().is_ok() {
            let emitter = &mut lab.asset_mut().emitters[index];
            if let Some(burst) = delete_burst {
                emitter.bursts.remove(burst);
            }
            if add_burst {
                document::add_burst(emitter);
            }
            structural = true;
        }
        lab.property_edit(edits.changed, edits.committed);
        if structural {
            lab.structure_changed();
        }
        action
    }
}

/// A labeled field in a column of `width` points.
fn field(ui: &mut Ui, width: f32, label: &str, contents: impl FnOnce(&mut Ui)) {
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        if !label.is_empty() {
            components::field_title(ui, label);
        } else {
            ui.add_space(tokens::TYPE_SM + 6.0);
        }
        contents(ui);
    });
}

/// A small section title.
fn section(ui: &mut Ui, title: &str) {
    ui.add_space(tokens::SPACE_MD);
    ui.label(RichText::new(title).strong().color(tokens::TEXT).size(tokens::TYPE_SM));
}

/// A whole-number field of at least one for a `u32` document value.
fn integer_field(ui: &mut Ui, id: &str, value: &mut u32, width: f32, drafts: &mut NumericDrafts) -> ControlResponse {
    let mut number = f64::from(*value);
    let response =
        NumberField::new(id, &mut number).minimum(1.0).maximum(4096.0).integer().width(width).show(ui, drafts);
    if response.changed {
        *value = number as u32;
    }
    response
}

/// The value a field would show when its link is off.
fn disabled_number(ui: &mut Ui, text: &str, width: f32) {
    let mut placeholder = text.to_owned();
    ui.add_sized(
        [width, ui.spacing().interact_size.y],
        egui::TextEdit::singleline(&mut placeholder).interactive(false),
    );
}

/// A child-target menu: `None` or another emitter's id. Returns the new choice when it changed.
fn target_menu(ui: &mut Ui, id: &str, current: Option<&str>, others: &[String], width: f32) -> Option<Option<String>> {
    let mut choice = None;
    egui::ComboBox::from_id_salt(id).width(width - 8.0).selected_text(current.unwrap_or("None")).show_ui(ui, |ui| {
        if ui.selectable_label(current.is_none(), "None").clicked() && current.is_some() {
            choice = Some(None);
        }
        for other in others {
            if ui.selectable_label(current == Some(other.as_str()), other).clicked() && current != Some(other.as_str())
            {
                choice = Some(Some(other.clone()));
            }
        }
    });
    choice
}
