// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Materials inspector (UI module): the document palette's base color,
//! emissive, metallic, roughness, normal, ambient occlusion, and alpha policy, with PNG, JPEG, or WebP maps, adding,
//! renaming, and deleting materials, and MaterialX import and export. Scripts select materials with `material(Name)`;
//! edits never touch compiled geometry, and every change is validated against FORM's palette budget before it applies.

use std::collections::HashMap;

use joy_engine::development::components;
use joy_engine::development::{
    ColorControl,
    ComponentGroup,
    EditPhase,
    NumericDrafts,
    ScalarControl,
};
use joy_engine::egui::{
    self,
    RichText,
    Ui,
};
use joy_engine::form::{
    DOCUMENT_TEXTURE_BYTES,
    default_material,
    read_materials,
};
use joy_engine::rendering::{
    ImageFormat,
    TextureAsset,
};
use joy_engine::runtime::theme::{
    colors,
    layout,
};
use joy_engine::serde_json::{
    Value as Json,
    json,
};

/// Largest emissive channel the material editor accepts (linear HDR).
const MAXIMUM_EMISSIVE: f64 = 1000.0;

/// Texture slots in inspector order with their titles.
pub const TEXTURE_FIELDS: [(&str, &str); 6] = [
    ("baseColor", "Base Color"),
    ("emissive", "Emissive"),
    ("metallic", "Metallic"),
    ("roughness", "Roughness"),
    ("normal", "Normal"),
    ("ao", "Ambient Occlusion"),
];

/// What the author did in the Materials inspector.
#[derive(Clone, Debug, PartialEq)]
pub enum MaterialAction {
    /// Replace the palette with a validated one.
    SetMaterials(Vec<Json>),
    /// Choose an image for a material's texture slot.
    ChooseTexture {
        /// Material name.
        material: String,
        /// Texture slot (`baseColor`, `normal`, …).
        field: &'static str,
    },
    /// Open a MaterialX file to merge.
    ImportMaterialX,
    /// Save the palette as MaterialX.
    ExportMaterialX,
}

/// Inspector state: the selected material, the new-material name, the status message, and map thumbnails.
#[derive(Default)]
pub struct MaterialControls {
    /// The selected material's name.
    pub selected: String,
    /// Name typed for Add Material.
    pub new_name: String,
    /// Status or error line.
    pub message: String,
    thumbnails: HashMap<String, Option<egui::TextureHandle>>,
}

impl MaterialControls {
    /// Controls with Default selected.
    pub fn new() -> Self {
        Self {
            selected: "Default".into(),
            ..Self::default()
        }
    }

    /// Select Default and clear messages (a new document).
    pub fn reset(&mut self) {
        self.selected = "Default".into();
        self.message.clear();
        self.thumbnails.clear();
    }

    /// Draw the Materials tab for `materials` (validated, Default first) and return the author's actions.
    pub fn show(&mut self, ui: &mut Ui, materials: &[Json], drafts: &mut NumericDrafts) -> Vec<MaterialAction> {
        let mut actions = Vec::new();
        components::field_title(ui, "MATERIALS");
        components::guidance(
            ui,
            "Linear RGB · Select in script with material(Name). Vertex colors multiply base color.",
        );
        ui.add_space(layout::SPACE_SM);
        if !materials.iter().any(|material| material["name"] == self.selected.as_str()) {
            self.selected = "Default".into();
        }
        egui::ComboBox::from_id_salt("form-editor-material-select")
            .selected_text(self.selected.clone())
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for material in materials {
                    let name = material["name"].as_str().unwrap_or_default();
                    if ui.selectable_label(name == self.selected, name).clicked() {
                        self.selected = name.to_owned();
                    }
                }
            });
        let Some(index) = materials.iter().position(|material| material["name"] == self.selected.as_str()) else {
            return actions;
        };
        let mut palette = materials.to_vec();
        let changed = ui.push_id(("form-editor-material", self.selected.clone()), |ui| {
            self.material_fields(ui, &mut palette, index, drafts, &mut actions)
        });
        if changed.inner {
            self.commit(palette, &mut actions);
        }

        ui.add_space(layout::SPACE_SM);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_name)
                    .hint_text("NewMaterial")
                    .desired_width(110.0)
                    .char_limit(64),
            );
            if ui.button("Add Material").clicked() {
                let mut material = default_material();
                material["name"] = json!(self.new_name.trim());
                let mut next = materials.to_vec();
                next.push(material);
                match read_materials(Some(&Json::Array(next))) {
                    Ok(validated) => {
                        self.selected = self.new_name.trim().to_owned();
                        self.new_name.clear();
                        self.message.clear();
                        actions.push(MaterialAction::SetMaterials(validated));
                    }
                    Err(error) => self.message = error.0,
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.add_enabled(self.selected != "Default", egui::Button::new("Delete Material")).clicked() {
                let removed = self.selected.clone();
                let next: Vec<Json> = materials.iter().filter(|material| material["name"] != removed.as_str()).cloned().collect();
                if let Ok(validated) = read_materials(Some(&Json::Array(next))) {
                    self.selected = "Default".into();
                    actions.push(MaterialAction::SetMaterials(validated));
                    self.message = format!(
                        "Deleted {removed}. Replace material({removed}) in the script with material() or another material name."
                    );
                }
            }
            if ui.button("Import .mtlx").clicked() {
                actions.push(MaterialAction::ImportMaterialX);
            }
            if ui.button("Export .mtlx").clicked() {
                actions.push(MaterialAction::ExportMaterialX);
            }
        });
        if !self.message.is_empty() {
            components::guidance(ui, self.message.clone());
        }
        ui.add_space(layout::SPACE_SM);
        components::guidance(ui, "GPU metallic/roughness lighting. MaterialX supports constant gltf_pbr inputs only.");
        actions
    }

    /// Validate and publish an edited palette, keeping the current one (with a message) when it is rejected.
    fn commit(&mut self, palette: Vec<Json>, actions: &mut Vec<MaterialAction>) {
        match read_materials(Some(&Json::Array(palette))) {
            Ok(validated) => {
                self.message.clear();
                actions.push(MaterialAction::SetMaterials(validated));
            }
            Err(error) => self.message = error.0,
        }
    }

    /// The selected material's fields; returns whether a value changed.
    fn material_fields(
        &mut self,
        ui: &mut Ui,
        palette: &mut [Json],
        index: usize,
        drafts: &mut NumericDrafts,
        actions: &mut Vec<MaterialAction>,
    ) -> bool {
        let mut changed = false;
        let material = &mut palette[index];
        let name = material["name"].as_str().unwrap_or_default().to_owned();
        let mut draft_name = ui
            .data_mut(|data| data.get_temp::<String>(egui::Id::new(("material-name", &name))))
            .unwrap_or_else(|| name.clone());
        let field = ui.add_enabled(
            name != "Default",
            egui::TextEdit::singleline(&mut draft_name).char_limit(64).desired_width(f32::INFINITY),
        );
        ui.data_mut(|data| data.insert_temp(egui::Id::new(("material-name", &name)), draft_name.clone()));
        if field.lost_focus() && draft_name.trim() != name {
            let next = draft_name.trim().to_owned();
            material["name"] = json!(next);
            ui.data_mut(|data| data.remove::<String>(egui::Id::new(("material-name", &name))));
            self.selected = next.clone();
            self.message = format!("Renamed {name} to {next}. Update material({name}) in the script.");
            changed = true;
        }

        for (field, title, channels) in [("baseColor", "Base Color", 4usize), ("emissive", "Emissive", 3usize)] {
            section(ui, title);
            let mut values: Vec<f64> =
                (0..channels).map(|channel| material[field][channel].as_f64().unwrap_or(1.0)).collect();
            // Base color is a reflectance in 0–1; emission is linear light and may exceed one for HDR glow.
            let minimums = vec![0.0; channels];
            let maximum = if field == "emissive" { MAXIMUM_EMISSIVE } else { 1.0 };
            let maximums = vec![maximum; channels];
            let mut edited = false;
            let mut committed = false;
            ui.horizontal(|ui| {
                let picked = ColorControl::new(("picker", field), &mut values, &minimums, &maximums).show(ui, drafts);
                edited |= picked.changed;
                committed |= picked.committed();
                self.texture_row(ui, material, &name, field, actions, &mut changed);
            });
            if !material["textures"][field].is_null() {
                components::guidance(ui, "Color factor × texture");
            }
            let group = if channels == 4 {
                ComponentGroup::rgba(("channels", field), &mut values)
            } else {
                ComponentGroup::new(("channels", field), &mut values, &["R", "G", "B"])
            };
            let response = group.bounds(&minimums, &maximums).step(0.01).without_sliders().show(ui, drafts);
            edited |= response.changed;
            committed |= response.committed() || response.phase == EditPhase::Committed;
            if edited || committed {
                material[field] =
                    Json::Array(values.iter().map(|value| joy_engine::core::json_number(*value)).collect());
                changed = true;
            }
        }
        for (field, title) in [("metallic", "Metallic"), ("roughness", "Roughness")] {
            section(ui, title);
            ui.horizontal(|ui| self.texture_row(ui, material, &name, field, actions, &mut changed));
            let mut value = material[field].as_f64().unwrap_or_default();
            let label = if material["textures"][field].is_null() {
                format!("{title} value")
            } else {
                format!("{title} factor × texture")
            };
            let response = ScalarControl::new(field, &mut value)
                .range(0.0, 1.0)
                .step(0.01)
                .show(ui, drafts)
                .response
                .on_hover_text(label);
            let _ = response;
            if material[field].as_f64() != Some(value) {
                material[field] = joy_engine::core::json_number(value);
                changed = true;
            }
        }
        for (field, title) in [("normal", "Normal"), ("ao", "Ambient Occlusion")] {
            section(ui, title);
            ui.horizontal(|ui| self.texture_row(ui, material, &name, field, actions, &mut changed));
        }
        section(ui, "Alpha Mode");
        let mut mode = material["alphaMode"].as_str().unwrap_or("OPAQUE").to_owned();
        egui::ComboBox::from_id_salt("alpha-mode").selected_text(mode.clone()).show_ui(ui, |ui| {
            for candidate in ["OPAQUE", "MASK", "BLEND"] {
                ui.selectable_value(&mut mode, candidate.to_owned(), candidate);
            }
        });
        if material["alphaMode"] != mode.as_str() {
            material["alphaMode"] = json!(mode);
            changed = true;
        }
        if mode == "MASK" {
            let mut cutoff = material["alphaCutoff"].as_f64().unwrap_or(0.5);
            ScalarControl::new("alpha-cutoff", &mut cutoff).range(0.0, 1.0).step(0.01).label("Cutoff").show(ui, drafts);
            if material["alphaCutoff"].as_f64() != Some(cutoff) {
                material["alphaCutoff"] = joy_engine::core::json_number(cutoff);
                changed = true;
            }
        }
        changed
    }

    /// Choose/Replace, Clear, and a thumbnail (or "Not Set") for one texture slot.
    fn texture_row(
        &mut self,
        ui: &mut Ui,
        material: &mut Json,
        name: &str,
        field: &'static str,
        actions: &mut Vec<MaterialAction>,
        changed: &mut bool,
    ) {
        let source = material["textures"][field].as_str().map(str::to_owned);
        let label = if source.is_some() { "Replace Texture" } else { "Choose Texture" };
        if ui.small_button(label).clicked() {
            actions.push(MaterialAction::ChooseTexture {
                material: name.to_owned(),
                field,
            });
        }
        if ui.add_enabled(source.is_some(), egui::Button::new("Clear").small()).clicked() {
            material["textures"][field] = Json::Null;
            *changed = true;
        }
        match source {
            Some(source) => {
                let thumbnail =
                    self.thumbnails.entry(source.clone()).or_insert_with(|| thumbnail(ui.ctx(), &source)).clone();
                match thumbnail {
                    Some(texture) => {
                        ui.add(egui::Image::new(&texture).fit_to_exact_size(egui::vec2(28.0, 28.0)))
                            .on_hover_text(format!("{field} texture preview"));
                    }
                    None => {
                        ui.label(RichText::new("Unreadable").color(colors::DANGER).size(layout::TYPE_XS));
                    }
                }
            }
            None => {
                ui.label(RichText::new("Not Set").color(colors::MUTED).size(layout::TYPE_XS));
            }
        }
    }
}

/// A material property heading.
fn section(ui: &mut Ui, title: &str) {
    ui.add_space(layout::SPACE_XS);
    ui.label(RichText::new(title).color(colors::MUTED).size(layout::TYPE_SM).strong());
}

/// Decode an embedded map into a small UI texture.
fn thumbnail(context: &egui::Context, source: &str) -> Option<egui::TextureHandle> {
    let texture = TextureAsset::decode_data_url(source).ok()?;
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [texture.width() as usize, texture.height() as usize],
        texture.pixels(),
    );
    Some(context.load_texture(format!("form-editor-map-{}", texture.id()), image, egui::TextureOptions::LINEAR))
}

/// Turn an opened image file into an embedded data URL, refusing unsupported formats and files over the per-map
/// budget.
pub fn image_data_url(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() > DOCUMENT_TEXTURE_BYTES {
        return Err("Textures must be 4 MB or smaller.".into());
    }
    let format = ImageFormat::sniff(bytes).ok_or("Textures must be PNG, JPEG, or WebP images.")?;
    Ok(format!("data:{};base64,{}", format.media_type(), joy_engine::core::encode_base64(bytes)))
}

/// A palette with one material's texture slot set, validated against FORM's budgets.
pub fn with_texture(
    materials: &[Json],
    material: &str,
    field: &str,
    source: Option<String>,
) -> Result<Vec<Json>, String> {
    let mut next = materials.to_vec();
    let entry = next.iter_mut().find(|entry| entry["name"] == material).ok_or("The material no longer exists.")?;
    entry["textures"][field] = source.map_or(Json::Null, Json::String);
    read_materials(Some(&Json::Array(next))).map_err(|error| error.0)
}

/// Merge imported MaterialX materials: matching names update their factors and keep their maps.
pub fn merge_materialx(materials: &[Json], imported: Vec<Json>) -> Result<Vec<Json>, String> {
    let mut merged = materials.to_vec();
    for mut material in imported {
        match merged.iter_mut().find(|existing| existing["name"] == material["name"]) {
            Some(existing) => {
                material["textures"] = existing["textures"].clone();
                *existing = material;
            }
            None => merged.push(material),
        }
    }
    read_materials(Some(&Json::Array(merged))).map_err(|error| error.0)
}
