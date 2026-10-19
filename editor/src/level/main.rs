// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The level and object editor's document logic.
//!
//! [`LevelEditor`] owns one `.joylevel` (or a `.joyobject`, edited as a one-entity level) through the engine's
//! [`LevelDocument`]: every edit is one validated transaction with undo and redo, and the workspace owns files, saves,
//! and conflicts. Inspector text that has not been committed lives in [`FieldDrafts`]; invalid drafts and invalid
//! Advanced Source block saving, selection changes, and further edits until repaired.
//!
//! Units: transforms are meters, degrees in the inspector (stored as radians), and scale factors. Linked object
//! instances keep their identity and placement locally; component edits become explicit overrides.

use std::collections::HashMap;

use joy_engine::core::{
    RandomSource,
    SeededRandom,
};
use joy_engine::gameplay::{
    ComponentFieldKind,
    ComponentIssue,
    ComponentSchemaCatalog,
    EntityDefinition,
    LevelData,
    LevelDocument,
    LevelError,
    ObjectDocument,
    component_preset,
    component_type,
    create_object_instance,
    entity_type,
    parse_level,
    parse_object,
    resolve_object_instance,
    serialize_level,
    serialize_object,
};
use joy_engine::math::Vec3;
use joy_engine::particles::{
    ParticleParameter,
    parse_particle_effect,
};
use joy_engine::rendering::engine_component_catalog;
use joy_engine::runtime::unix_time_milliseconds;
use joy_engine::serde_json::{
    self,
    Map,
    Value,
    json,
};

use super::constants::{
    LevelTool,
    PRIMITIVE_COLOR,
};
use super::property_editor::{
    FieldMetadata,
    value_at,
};
use super::viewport::LevelViewport;
use crate::interaction::numeric_controls::{
    FieldDrafts,
    NumberBounds,
    display_number,
    json_number,
    parse_number,
};
use crate::interaction::viewport_input::snap_point;
use crate::server::AssetEntry;

/// Project access the level editor needs: the asset catalog, reads of linked content, and object creation. The
/// workspace implements it for the document's project.
pub trait AssetAccess {
    /// The project's supported assets.
    fn catalog(&self) -> &[AssetEntry];
    /// The text of a project asset.
    fn read(&mut self, path: &str) -> Result<String, String>;
    /// Create a new asset without replacing an existing file.
    fn create(&mut self, path: &str, text: &str) -> Result<(), String>;
}

/// An inspector field whose typed text may be pending.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldKey {
    /// The level's name.
    LevelName,
    /// The selected entity's name.
    EntityName,
    /// The selected entity's type.
    EntityType,
    /// The selected entity's comma-separated tags.
    EntityTags,
    /// One transform channel of the selected entity.
    Transform(TransformField, usize),
    /// A scalar component field, by component path.
    Property(Vec<String>),
    /// One channel of a component vector.
    PropertyChannel(Vec<String>, usize),
}

/// Transform vectors in inspector order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransformField {
    /// Position in meters.
    Position,
    /// Rotation, edited in degrees and stored in radians.
    Rotation,
    /// Scale factors.
    Scale,
}

impl TransformField {
    /// Every transform field in display order.
    pub const ALL: [Self; 3] = [Self::Position, Self::Rotation, Self::Scale];

    /// Inspector heading.
    pub fn label(self) -> &'static str {
        match self {
            Self::Position => "POSITION",
            Self::Rotation => "ROTATION · DEGREES",
            Self::Scale => "SCALE",
        }
    }

    /// Factor from stored to displayed units.
    fn display_factor(self) -> f32 {
        if self == Self::Rotation { 180.0 / std::f32::consts::PI } else { 1.0 }
    }

    /// Arrow-key step of the displayed value.
    pub fn step(self) -> f64 {
        if self == Self::Rotation { 5.0 } else { 0.25 }
    }
}

/// A component the author can add.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComponentChoice {
    /// A schema preset id.
    Preset(String),
    /// An unschematized record named by the author.
    Custom,
}

/// Error wrapper for edits that may fail before or after document validation.
#[derive(Debug)]
struct EditError(String);

impl From<LevelError> for EditError {
    fn from(error: LevelError) -> Self {
        Self(error.to_string())
    }
}

/// Owns one authored level or object, its selection, inspector drafts, and viewport; the workspace owns the file.
pub struct LevelEditor {
    // Authored document and its file identity.
    document: LevelDocument,
    path: String,
    object_mode: bool,
    generation: u64,

    // Component vocabulary and linked content.
    schemas: ComponentSchemaCatalog,
    objects: HashMap<String, ObjectDocument>,
    form_parameters: HashMap<String, Vec<Value>>,
    metadata: HashMap<String, FieldMetadata>,
    parameter_defaults: Vec<(String, String, Value)>,

    // Inspector state.
    drafts: FieldDrafts<FieldKey>,
    /// The Advanced Source buffer for the selected entity's components.
    pub components_source: String,
    components_snapshot: String,
    /// The text the document was loaded from, until the first authored change.
    loaded_text: Option<String>,
    /// The scene filter text (view only).
    pub filter: String,
    /// The reusable object chosen for **+ INSTANCE**.
    pub instance_choice: Option<String>,
    /// The component chosen for **+ ADD**.
    pub component_choice: ComponentChoice,

    // Presentation.
    /// The viewport.
    pub viewport: LevelViewport,
    scene_dirty: bool,
    parameters_dirty: bool,

    // Messages and requests for the workspace.
    status: String,
    open_requests: Vec<String>,
    ids: SeededRandom,
}

impl LevelEditor {
    /// Open `text` as the document at `path`; `.joyobject` paths open as reusable objects.
    pub fn load(path: &str, text: &str) -> Result<Self, String> {
        let (document, object_mode) = parse_document(path, text)?;
        let mut editor = Self {
            document,
            path: path.to_owned(),
            object_mode,
            generation: 0,
            schemas: engine_component_catalog(),
            objects: HashMap::new(),
            form_parameters: HashMap::new(),
            metadata: HashMap::new(),
            parameter_defaults: Vec::new(),
            drafts: FieldDrafts::new(),
            components_source: String::new(),
            components_snapshot: String::new(),
            loaded_text: Some(text.to_owned()),
            filter: String::new(),
            instance_choice: None,
            component_choice: ComponentChoice::Preset(component_preset::RENDER_PRIMITIVE.to_owned()),
            viewport: LevelViewport::new(),
            scene_dirty: true,
            parameters_dirty: true,
            status: "Select an entity, or add a primitive. Edits save through the workspace.".to_owned(),
            open_requests: Vec::new(),
            ids: SeededRandom::new(unix_time_milliseconds() as i64),
        };
        editor.refresh_source();
        editor.viewport.focus(None);
        Ok(editor)
    }

    /// Replace the document with disk text (an external edit), keeping the camera and view settings.
    pub fn reload(&mut self, text: &str) -> Result<(), String> {
        let selected = self.document.selected_id().map(str::to_owned);
        let (mut document, object_mode) = parse_document(&self.path, text)?;
        if !object_mode {
            document.select(selected.as_deref());
        }
        self.document = document;
        self.object_mode = object_mode;
        self.drafts.clear();
        self.objects.clear();
        self.changed();
        self.loaded_text = Some(text.to_owned());
        self.refresh_source();
        Ok(())
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Document state
    // -----------------------------------------------------------------------------------------------------------------

    /// The project path this editor opened.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Whether the document is a reusable `.joyobject`.
    pub fn object_mode(&self) -> bool {
        self.object_mode
    }

    /// The authored document.
    pub fn document(&self) -> &LevelDocument {
        &self.document
    }

    /// The component schemas.
    pub fn schemas(&self) -> &ComponentSchemaCatalog {
        &self.schemas
    }

    /// Increases whenever authored data changes, so the workspace knows when to capture.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The latest message for the editor's status line.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Replace the status message.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    /// Asset paths whose source the author asked to open, drained by the workspace.
    pub fn take_open_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.open_requests)
    }

    /// Pending inspector drafts.
    pub fn drafts(&self) -> &FieldDrafts<FieldKey> {
        &self.drafts
    }

    /// Mutable inspector drafts, for the UI to record typing.
    pub fn drafts_mut(&mut self) -> &mut FieldDrafts<FieldKey> {
        &mut self.drafts
    }

    /// The selected entity as authored (template links unresolved).
    pub fn selected(&self) -> Option<&EntityDefinition> {
        let id = self.document.selected_id()?;
        self.document.level().entities.iter().find(|entity| entity.id == id)
    }

    /// An entity with its linked object resolved, when the source is loaded.
    pub fn resolve(&self, entity: &EntityDefinition) -> EntityDefinition {
        entity
            .template
            .as_ref()
            .and_then(|template| self.objects.get(template))
            .and_then(|source| resolve_object_instance(entity, source).ok())
            .unwrap_or_else(|| entity.clone())
    }

    /// Entities matching the scene filter (name, type, or tag), as authored.
    pub fn filtered_entities(&self) -> Vec<&EntityDefinition> {
        let query = self.filter.trim().to_lowercase();
        self.document
            .level()
            .entities
            .iter()
            .filter(|entity| {
                let resolved = self.resolve(entity);
                let mut haystack = vec![resolved.name.clone(), resolved.kind.clone()];
                haystack.extend(resolved.tags.iter().cloned());
                haystack.join(" ").to_lowercase().contains(&query)
            })
            .collect()
    }

    /// The selected entity's issues against the component schemas (resolved data, not display defaults).
    pub fn selected_issues(&self) -> Vec<ComponentIssue> {
        self.selected().map(|entity| self.schemas.validate_entity(&self.resolve(entity))).unwrap_or_default()
    }

    /// The selected entity's components for display: resolved, with FORM and JOYFX parameter defaults shown (but not
    /// stored) for asset-backed render and effect components.
    pub fn display_components(&self) -> Map<String, Value> {
        let Some(entity) = self.selected() else {
            return Map::new();
        };
        let resolved = self.resolve(entity);
        let mut components: Map<String, Value> =
            resolved.components.iter().map(|(name, record)| (name.clone(), Value::Object(record.clone()))).collect();
        for (component, name, value) in &self.parameter_defaults {
            let Some(Value::Object(record)) = components.get_mut(component) else {
                continue;
            };
            let parameters = record.entry("parameters").or_insert_with(|| Value::Object(Map::new()));
            if let Value::Object(parameters) = parameters {
                parameters.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
        components
    }

    /// Display metadata (bounds) for asset-declared parameters, by dotted path.
    pub fn parameter_metadata(&self) -> &HashMap<String, FieldMetadata> {
        &self.metadata
    }

    /// The current value of a transform channel in displayed units (degrees for rotation).
    pub fn transform_value(&self, field: TransformField, axis: usize) -> Option<f32> {
        let entity = self.resolve(self.selected()?);
        let vector = match field {
            TransformField::Position => entity.transform.position,
            TransformField::Rotation => entity.transform.rotation,
            TransformField::Scale => entity.transform.scale,
        };
        Some(vector[axis] * field.display_factor())
    }

    /// Serialize the document; `passive` captures (polling) refuse while anything is pending instead of committing it.
    ///
    /// An explicit capture commits valid drafts and Advanced Source first and fails, keeping every draft, while any
    /// input is invalid.
    pub fn capture(&mut self, passive: bool) -> Result<String, String> {
        if passive {
            if !self.drafts.is_empty() || self.components_source != self.components_snapshot {
                return Err("Pending inspector input.".to_owned());
            }
        } else {
            self.commit_drafts()?;
            self.apply_components()?;
        }
        Ok(self.serialize())
    }

    /// The document's text as written to disk: the loaded text until the first edit (so opening never dirties a
    /// file whose formatting differs from the canonical form), then the canonical serialization.
    pub fn serialize(&self) -> String {
        if let Some(text) = &self.loaded_text {
            return text.clone();
        }
        let level = self.document.level();
        if self.object_mode
            && let Some(entity) = level.entities.first()
        {
            let object = ObjectDocument { version: 1, name: level.name.clone(), entity: entity.clone() };
            if let Ok(text) = serialize_object(&object) {
                return text;
            }
        }
        serialize_level(level)
    }

    /// Project content changed on disk: re-read linked objects, meshes, and effects while keeping history.
    pub fn assets_changed(&mut self) {
        self.objects.clear();
        self.form_parameters.clear();
        self.scene_dirty = true;
        self.parameters_dirty = true;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Presentation
    // -----------------------------------------------------------------------------------------------------------------

    /// Bring the viewport and parameter metadata up to date with the document, reading content through `access`.
    /// Failures keep the previous presentation and report `Content: …`.
    pub fn sync(&mut self, access: &mut dyn AssetAccess) {
        if self.scene_dirty {
            self.scene_dirty = false;
            self.parameters_dirty = true;
            let level = self.document.level();
            let selected = self.document.selected_id().map(str::to_owned);
            let objects = &mut self.objects;
            let read = |path: &str| -> Result<String, String> {
                let text = access.read(path)?;
                if path.ends_with(".joyobject")
                    && let Ok(object) = parse_object(&text)
                {
                    objects.insert(path.to_owned(), object);
                }
                Ok(text)
            };
            if let Err(error) = self.viewport.sync(&level.entities, selected.as_deref(), read) {
                self.status = format!("Content: {error}");
            }
            // Resolved values may have changed with newly read objects.
            if self.components_source == self.components_snapshot {
                self.refresh_source();
            }
        }
        if self.parameters_dirty {
            self.parameters_dirty = false;
            self.refresh_parameters(access);
        }
    }

    /// The tab became active: clear scene effects other tabs may have left on; the viewport resolves post-processing
    /// every frame. Effect previews start over, since the renderer's effects were reset.
    pub fn activated(&mut self, renderer: &mut joy_engine::rendering::Renderer) {
        let _ = renderer.set_scene_effects(None);
        self.viewport.effects = joy_engine::development::SceneEffectsControls::new();
    }

    /// Advance attached effects while the tab is active.
    pub fn step(&mut self, seconds: f64) {
        self.viewport.step(seconds);
        if let Some(error) = self.viewport.effect_error() {
            self.status = error.to_owned();
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Selection and inspector commits
    // -----------------------------------------------------------------------------------------------------------------

    /// Select an entity (or clear the selection) after committing pending input; invalid input blocks the change.
    pub fn select(&mut self, id: Option<&str>) -> bool {
        if let Err(error) = self.commit_drafts().and_then(|()| self.apply_components()) {
            self.status = error;
            return false;
        }
        self.document.select(id);
        let selected = self.document.selected_id().map(str::to_owned);
        self.viewport.select(selected.as_deref());
        self.parameters_dirty = true;
        self.refresh_source();
        true
    }

    /// Commit every pending draft; the first invalid one fails and every draft stays visible.
    pub fn commit_drafts(&mut self) -> Result<(), String> {
        for key in self.drafts.keys() {
            self.commit_draft(&key)?;
        }
        Ok(())
    }

    /// Commit one draft; an invalid value keeps the draft with its error and reports the message.
    pub fn commit_draft(&mut self, key: &FieldKey) -> Result<(), String> {
        let Some(draft) = self.drafts.get(key).cloned() else {
            return Ok(());
        };
        let result = match draft.number {
            Some(bounds) => parse_number(&draft.text, &bounds).and_then(|value| self.commit_number(key, value)),
            None => self.commit_text(key, &draft.text),
        };
        match result {
            Ok(()) => {
                self.drafts.discard(key);
                Ok(())
            }
            Err(message) => {
                let message = match key {
                    FieldKey::Property(path) | FieldKey::PropertyChannel(path, _) => {
                        format!("{}: {message}", path.join("."))
                    }
                    _ => message,
                };
                self.drafts.fail(key, message.clone());
                self.status = message.clone();
                Err(message)
            }
        }
    }

    /// Apply a committed numeric value.
    fn commit_number(&mut self, key: &FieldKey, value: f64) -> Result<(), String> {
        match key {
            FieldKey::Transform(field, axis) => {
                let (field, axis) = (*field, *axis);
                // Display rounding must not change untouched authored precision.
                let shown = self.transform_value(field, axis).map(|current| display_number(f64::from(current)));
                if shown.as_deref() == Some(display_number(value).as_str()) {
                    return Ok(());
                }
                let stored = (value as f32) / field.display_factor();
                self.edit_selected(|entity| {
                    let vector = match field {
                        TransformField::Position => &mut entity.transform.position,
                        TransformField::Rotation => &mut entity.transform.rotation,
                        TransformField::Scale => &mut entity.transform.scale,
                    };
                    vector[axis] = stored;
                })
            }
            FieldKey::Property(path) => self.change_property(path, Some(json_number(value))),
            FieldKey::PropertyChannel(path, index) => {
                let components = Value::Object(self.display_components());
                let mut values: Vec<Value> =
                    value_at(&components, path).and_then(Value::as_array).cloned().unwrap_or_default();
                if *index >= values.len() {
                    return Err("Enter a finite vector component.".to_owned());
                }
                values[*index] = json_number(value);
                self.change_property(path, Some(Value::Array(values)))
            }
            _ => self.commit_text(key, &value.to_string()),
        }
    }

    /// Apply committed text.
    fn commit_text(&mut self, key: &FieldKey, text: &str) -> Result<(), String> {
        match key {
            FieldKey::LevelName => {
                self.apply_components()?;
                let name = text.to_owned();
                self.transact(|level| level.name = name)?;
                self.status = "Property updated.".to_owned();
                Ok(())
            }
            FieldKey::EntityName => {
                let name = text.to_owned();
                self.edit_selected(|entity| entity.name = name)
            }
            FieldKey::EntityType => {
                let kind = text.to_owned();
                let resolved = self.selected().map(|entity| self.resolve(entity).kind);
                self.edit_selected(|entity| {
                    if entity.template.is_some() {
                        if resolved.as_deref() != Some(kind.as_str()) {
                            entity.overrides.get_or_insert_with(Map::new).insert("type".to_owned(), Value::from(kind));
                        }
                    } else {
                        entity.kind = kind;
                    }
                })
            }
            FieldKey::EntityTags => {
                let tags: Vec<String> =
                    text.split(',').map(str::trim).filter(|tag| !tag.is_empty()).map(str::to_owned).collect();
                let resolved = self.selected().map(|entity| self.resolve(entity).tags);
                self.edit_selected(|entity| {
                    if entity.template.is_some() {
                        if resolved.as_ref() != Some(&tags) {
                            entity.overrides.get_or_insert_with(Map::new).insert("tags".to_owned(), Value::from(tags));
                        }
                    } else {
                        entity.tags = tags;
                    }
                })
            }
            FieldKey::Property(path) => self.change_property(path, Some(Value::from(text.to_owned()))),
            FieldKey::Transform(..) | FieldKey::PropertyChannel(..) => Err("Enter a finite number.".to_owned()),
        }
    }

    /// Commit the Advanced Source buffer when it changed; invalid JSON or components fail and keep the buffer.
    pub fn apply_components(&mut self) -> Result<(), String> {
        if self.document.selected_id().is_none() || self.components_source == self.components_snapshot {
            return Ok(());
        }
        let parsed: Value = serde_json::from_str(&self.components_source)
            .map_err(|error| format!("Advanced Source is not valid JSON: {error}"))?;
        let Value::Object(components) = parsed else {
            return Err("Advanced Source must be a record of components.".to_owned());
        };
        let id = self.document.selected_id().map(str::to_owned);
        self.try_transact(|level| {
            let entity = level.entities.iter_mut().find(|entity| Some(&entity.id) == id.as_ref());
            if let Some(entity) = entity {
                if entity.template.is_some() {
                    entity
                        .overrides
                        .get_or_insert_with(Map::new)
                        .insert("components".to_owned(), Value::Object(components));
                } else {
                    entity.components = Default::default();
                    for (name, value) in components {
                        let Value::Object(record) = value else {
                            return Err(EditError(format!("Component {name} must be a record.")));
                        };
                        entity.components.insert(name, record);
                    }
                }
            }
            Ok(())
        })?;
        self.components_snapshot = self.components_source.clone();
        Ok(())
    }

    /// Discard Advanced Source edits.
    pub fn revert_components(&mut self) {
        self.components_source = self.components_snapshot.clone();
    }

    /// Whether Advanced Source holds uncommitted text.
    pub fn components_pending(&self) -> bool {
        self.components_source != self.components_snapshot
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Entity edits
    // -----------------------------------------------------------------------------------------------------------------

    /// Add a box or sphere primitive with a matching static collider at the snapped camera target.
    pub fn add_primitive(&mut self, sphere: bool) {
        let preset = if sphere { component_preset::PHYSICS_SPHERE } else { component_preset::PHYSICS_BOX };
        let collider = self.schemas.create_component(preset).map(|created| created.data).unwrap_or_default();
        let shape = if sphere { "sphere" } else { "box" };
        let mut components = Map::new();
        components.insert(component_type::RENDER.to_owned(), json!({"shape": shape, "color": PRIMITIVE_COLOR}));
        components.insert(component_type::PHYSICS.to_owned(), Value::Object(collider));
        let name = if sphere { "Sphere" } else { "Box" };
        if self.add_entity(name, components, 0.5) {
            self.status = "Primitive added. Use MOVE and click the ground to place it.".to_owned();
        }
    }

    /// Add an editable point light with a small visible marker, three meters up.
    pub fn add_light(&mut self) {
        let light = self
            .schemas
            .create_component(component_preset::LIGHT_POINT)
            .map(|created| created.data)
            .unwrap_or_default();
        let mut components = Map::new();
        components.insert(component_type::LIGHT.to_owned(), Value::Object(light));
        components.insert(
            component_type::RENDER.to_owned(),
            json!({"shape": "sphere", "size": [0.25, 0.25, 0.25], "color": [1, 0.72, 0.3]}),
        );
        if self.add_entity("Point Light", components, 3.0) {
            self.status = "Point Light added. Components define its runtime behavior.".to_owned();
        }
    }

    /// Add an entity without assuming a render or physics implementation.
    pub fn add_empty(&mut self) {
        if self.add_entity("Entity", Map::new(), 0.0) {
            self.status = "Entity added. Components define its runtime behavior.".to_owned();
        }
    }

    /// Duplicate the selection as a separate entity, offset by one snap step along X.
    pub fn duplicate(&mut self) {
        let Some(mut copy) = self.selected().cloned() else {
            return;
        };
        copy.id = self.new_id();
        copy.name.push_str(" copy");
        copy.transform.position.x += if self.viewport.snap > 0.0 { self.viewport.snap } else { 1.0 };
        let id = copy.id.clone();
        if self.guarded(|editor| editor.transact(|level| level.entities.push(copy))) {
            self.select_after_edit(&id);
        }
    }

    /// Delete the selected entity.
    pub fn remove_selected(&mut self) {
        let Some(id) = self.document.selected_id().map(str::to_owned) else {
            return;
        };
        if self.guarded(|editor| editor.transact(|level| level.entities.retain(|entity| entity.id != id))) {
            self.refresh_source();
        }
    }

    /// Undo or redo one authored transaction.
    pub fn history(&mut self, redo: bool) {
        if let Err(error) = self.commit_drafts().and_then(|()| self.apply_components()) {
            self.status = error;
            return;
        }
        let moved = if redo { self.document.redo() } else { self.document.undo() };
        if moved {
            self.changed();
            self.refresh_source();
        }
    }

    /// Move the selected entity horizontally to a ground point, keeping its height (Move mode ground clicks).
    pub fn move_to_ground(&mut self, point: Vec3) {
        if self
            .edit_selected(|entity| {
                entity.transform.position.x = point.x;
                entity.transform.position.z = point.z;
            })
            .is_ok()
        {
            self.status = "Property updated.".to_owned();
        }
    }

    /// Commit a finished gizmo drag as one transaction.
    pub fn move_entity(&mut self, id: &str, position: Vec3) {
        let id = id.to_owned();
        let result = self.guarded(|editor| {
            editor.transact(|level| {
                if let Some(entity) = level.entities.iter_mut().find(|entity| entity.id == id) {
                    entity.transform.position = position;
                }
            })
        });
        if result {
            self.status = "Property updated.".to_owned();
            self.refresh_source();
        } else {
            self.scene_dirty = true;
        }
    }

    /// Apply a mutation to the selected entity's draft; document validation commits or rolls back.
    pub fn edit_selected(&mut self, edit: impl FnOnce(&mut EntityDefinition)) -> Result<(), String> {
        self.apply_components()?;
        let Some(id) = self.document.selected_id().map(str::to_owned) else {
            return Ok(());
        };
        self.transact(|level| {
            if let Some(entity) = level.entities.iter_mut().find(|entity| entity.id == id) {
                edit(entity);
            }
        })?;
        self.status = "Property updated.".to_owned();
        self.refresh_source();
        Ok(())
    }

    /// Commit one nested component edit (`None` removes the property), preserving linked source values elsewhere.
    ///
    /// Instances store edits as overrides: arrays override as a whole, and removing an inherited nested property
    /// requires editing the source or detaching.
    pub fn change_property(&mut self, path: &[String], value: Option<Value>) -> Result<(), String> {
        self.apply_components()?;
        let Some(selected) = self.selected().cloned() else {
            return Ok(());
        };
        let resolved = Value::Object(
            self.resolve(&selected)
                .components
                .iter()
                .map(|(name, record)| (name.clone(), Value::Object(record.clone())))
                .collect(),
        );
        let id = selected.id.clone();
        let path = path.to_vec();
        self.try_transact(|level| {
            let Some(entity) = level.entities.iter_mut().find(|entity| entity.id == id) else {
                return Ok(());
            };
            if entity.template.is_some() {
                if value.is_none() && path.len() > 1 {
                    return Err(EditError(
                        "Edit the object source or detach this instance to remove an inherited property.".to_owned(),
                    ));
                }
                let overrides = entity.overrides.get_or_insert_with(Map::new);
                let components = overrides.entry("components").or_insert_with(|| Value::Object(Map::new()));
                if !components.is_object() {
                    *components = Value::Object(Map::new());
                }
                // Arrays override as units; numeric indices must not become record keys.
                let mut current = &resolved;
                let mut array_index = None;
                for (index, part) in path.iter().enumerate() {
                    if current.is_array() {
                        array_index = Some(index);
                        break;
                    }
                    current = current.get(part).unwrap_or(&Value::Null);
                }
                match (array_index, current) {
                    (Some(index), Value::Array(_)) => {
                        let mut copy = current.clone();
                        set_path(&mut copy, &path[index..], value);
                        set_path(components, &path[..index], Some(copy));
                    }
                    _ => set_path(components, &path, Some(value.unwrap_or(Value::Null))),
                }
            } else {
                let mut components = Value::Object(
                    entity
                        .components
                        .iter()
                        .map(|(name, record)| (name.clone(), Value::Object(record.clone())))
                        .collect(),
                );
                set_path(&mut components, &path, value);
                entity.components = Default::default();
                if let Value::Object(components) = components {
                    for (name, record) in components {
                        let Value::Object(record) = record else {
                            return Err(EditError(format!("Component {name} must be a record.")));
                        };
                        entity.components.insert(name, record);
                    }
                }
            }
            Ok(())
        })?;
        self.status = "Property updated.".to_owned();
        self.refresh_source();
        Ok(())
    }

    /// Add the chosen preset (with unassigned asset fields set to the first matching project asset) or a custom record
    /// named `custom_name`.
    pub fn add_component(&mut self, choice: &ComponentChoice, custom_name: Option<&str>, assets: &[AssetEntry]) {
        let (name, data) = match choice {
            ComponentChoice::Custom => match custom_name.map(str::trim).filter(|name| !name.is_empty()) {
                Some(name) => (name.to_owned(), Map::new()),
                None => return,
            },
            ComponentChoice::Preset(id) => match self.schemas.create_component(id) {
                Ok(created) => {
                    let mut data = created.data;
                    self.assign_first_assets(&created.kind, &mut data, assets);
                    (created.kind, data)
                }
                Err(error) => {
                    self.status = error.to_string();
                    return;
                }
            },
        };
        if let Some(entity) = self.selected()
            && self.resolve(entity).components.contains_key(&name)
        {
            self.status = format!("This object already has a {name} component. Remove it first to replace it.");
            return;
        }
        if let Err(error) = self.change_property(&[name], Some(Value::Object(data))) {
            self.status = error;
        }
    }

    /// Fill empty asset fields of a new component with the first project asset of an accepted kind.
    fn assign_first_assets(&self, kind: &str, data: &mut Map<String, Value>, assets: &[AssetEntry]) {
        let Some(schema) = self.schemas.get(kind) else {
            return;
        };
        for field in &schema.fields {
            if field.kind != ComponentFieldKind::Asset || data.get(&field.name) != Some(&Value::from("")) {
                continue;
            }
            if let Some(asset) = assets.iter().find(|asset| field.asset_kinds.contains(&asset.kind)) {
                data.insert(field.name.clone(), Value::from(asset.path.clone()));
            }
        }
    }

    /// The default **Save As Object** path for the selection, beneath `assets/`.
    pub fn suggested_object_path(&self) -> String {
        let name = self.selected().map(|entity| entity.name.to_lowercase()).unwrap_or_default();
        let mut slug = String::new();
        let mut separator = false;
        for character in name.chars() {
            if character.is_ascii_lowercase() || character.is_ascii_digit() {
                slug.push(character);
                separator = false;
            } else if !separator {
                slug.push('-');
                separator = true;
            }
        }
        format!("{slug}.joyobject")
    }

    /// Save the selection as a reusable `.joyobject` at `filename` (beneath `assets/`) and link it as an instance.
    pub fn save_object(&mut self, filename: &str, access: &mut dyn AssetAccess) -> Result<String, String> {
        self.commit_drafts()?;
        self.apply_components()?;
        let Some(entity) = self.selected().cloned() else {
            return Err("Select an entity to save as an object.".to_owned());
        };
        let filename = filename.trim();
        if filename.is_empty() {
            return Err("Name the reusable object.".to_owned());
        }
        let path = if filename.starts_with("assets/") { filename.to_owned() } else { format!("assets/{filename}") };
        let mut definition = self.resolve(&entity);
        definition.id = "object".to_owned();
        definition.transform.position = Vec3::ZERO;
        definition.template = None;
        definition.overrides = None;
        let object = ObjectDocument { version: 1, name: entity.name.clone(), entity: definition };
        let text = serialize_object(&object).map_err(|error| error.to_string())?;
        access.create(&path, &text)?;
        self.objects.insert(path.clone(), object);
        let template = path.clone();
        self.transact(|level| {
            if let Some(current) = level.entities.iter_mut().find(|candidate| candidate.id == entity.id) {
                current.template = Some(template);
                current.overrides = Some(Map::new());
                current.components = Default::default();
            }
        })?;
        self.refresh_source();
        self.status = format!("Created {path}. This instance now follows its source.");
        Ok(path)
    }

    /// Add a linked instance of the object at `path` at the camera target.
    pub fn add_instance(&mut self, path: &str, access: &mut dyn AssetAccess) -> Result<(), String> {
        self.apply_components()?;
        let object = parse_object(&access.read(path)?).map_err(|error| error.to_string())?;
        let id = self.new_id();
        let mut entity = create_object_instance(&object, path, &id).map_err(|error| error.to_string())?;
        entity.transform.position = self.viewport.camera.target;
        self.objects.insert(path.to_owned(), object);
        self.transact(|level| level.entities.push(entity))?;
        self.select_after_edit(&id);
        Ok(())
    }

    /// Clear the selected instance's overrides so it follows its source again.
    pub fn reset_overrides(&mut self) {
        if let Err(error) = self.edit_selected(|entity| {
            if entity.template.is_some() {
                entity.overrides = Some(Map::new());
            }
        }) {
            self.status = error;
        }
    }

    /// Replace the selected instance with an independent copy of its resolved values.
    pub fn detach(&mut self) {
        let Some(entity) = self.selected() else {
            return;
        };
        let resolved = self.resolve(entity);
        if let Err(error) = self.edit_selected(|entity| {
            entity.template = None;
            entity.overrides = None;
            entity.kind = resolved.kind;
            entity.tags = resolved.tags;
            entity.components = resolved.components;
        }) {
            self.status = error;
        }
    }

    /// Ask the workspace to open a referenced object, FORM, or JOYFX source.
    pub fn open_source(&mut self, path: &str) {
        self.open_requests.push(path.to_owned());
    }

    /// Switch the viewport tool.
    pub fn set_tool(&mut self, tool: LevelTool) {
        self.viewport.tool = tool;
        self.viewport.cancel_gizmo();
        self.status = match tool {
            LevelTool::Move => {
                "Drag a gizmo handle, or click the ground, to move the selected entity. Height is preserved."
            }
            LevelTool::Select => "Click an entity to select it. Drag to orbit.",
        }
        .to_owned();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------------------------------------------------

    /// Add an entity at the snapped camera target with `height` meters of elevation; returns whether it was added.
    fn add_entity(&mut self, name: &str, components: Map<String, Value>, height: f32) -> bool {
        let target = snap_point(self.viewport.camera.target, self.viewport.snap);
        let id = self.new_id();
        let mut entity = EntityDefinition::new(id.clone());
        entity.name = name.to_owned();
        entity.kind = entity_type::ENTITY.to_owned();
        entity.transform.position = Vec3::new(target.x, height, target.z);
        for (kind, record) in components {
            if let Value::Object(record) = record {
                entity.components.insert(kind, record);
            }
        }
        let added = self.guarded(|editor| editor.transact(|level| level.entities.push(entity)));
        if added {
            self.select_after_edit(&id);
        }
        added
    }

    /// Run an edit after committing pending input, reporting failures; returns whether it succeeded.
    fn guarded(&mut self, edit: impl FnOnce(&mut Self) -> Result<(), String>) -> bool {
        let result = self.commit_drafts().and_then(|()| self.apply_components()).and_then(|()| edit(self));
        match result {
            Ok(()) => true,
            Err(error) => {
                self.status = error;
                false
            }
        }
    }

    /// Select a new entity created by an edit.
    fn select_after_edit(&mut self, id: &str) {
        self.document.select(Some(id));
        self.parameters_dirty = true;
        self.refresh_source();
    }

    /// One validated transaction.
    fn transact(&mut self, edit: impl FnOnce(&mut LevelData)) -> Result<(), String> {
        self.try_transact(|level| {
            edit(level);
            Ok(())
        })
    }

    /// One validated transaction whose edit may fail; marks the presentation stale when data changed.
    fn try_transact(&mut self, edit: impl FnOnce(&mut LevelData) -> Result<(), EditError>) -> Result<(), String> {
        let changed = self.document.try_transact(edit).map_err(|error| error.0)?;
        if changed {
            self.changed();
        }
        Ok(())
    }

    /// Record an authored change.
    fn changed(&mut self) {
        self.loaded_text = None;
        self.generation += 1;
        self.scene_dirty = true;
        self.parameters_dirty = true;
    }

    /// Reset Advanced Source to the selection's resolved components.
    fn refresh_source(&mut self) {
        let text = self
            .selected()
            .map(|entity| {
                let resolved = self.resolve(entity);
                let components: Map<String, Value> =
                    resolved.components.into_iter().map(|(name, record)| (name, Value::Object(record))).collect();
                serde_json::to_string_pretty(&Value::Object(components)).unwrap_or_default()
            })
            .unwrap_or_default();
        self.components_source = text.clone();
        self.components_snapshot = text;
    }

    /// Recompute asset-declared parameter defaults and bounds for the selection.
    fn refresh_parameters(&mut self, access: &mut dyn AssetAccess) {
        self.metadata.clear();
        self.parameter_defaults.clear();
        let Some(entity) = self.selected().map(|entity| self.resolve(entity)) else {
            return;
        };
        let asset = |component: &str| {
            entity
                .components
                .get(component)
                .and_then(|record| record.get("asset"))
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
        };
        let mut found: Vec<FoundParameter> = Vec::new();
        if let Some(path) = asset(component_type::RENDER)
            && (path.ends_with(".form") || path.ends_with(".formlab"))
        {
            if !self.form_parameters.contains_key(&path)
                && let Ok(text) = access.read(&path)
            {
                self.form_parameters.insert(path.clone(), form_parameters(&path, &text));
            }
            for parameter in self.form_parameters.get(&path).into_iter().flatten() {
                let (Some(name), Some(value)) = (parameter.get("name").and_then(Value::as_str), parameter.get("value"))
                else {
                    continue;
                };
                let (Some(min), Some(max)) = (parameter.get("min"), parameter.get("max")) else {
                    continue;
                };
                // FORM numbers are `f64`; JSON writes integral defaults as integers, as JavaScript does.
                let value = value.as_f64().map_or_else(|| value.clone(), json_number);
                found.push((component_type::RENDER.to_owned(), name.to_owned(), value, min.as_f64(), max.as_f64()));
            }
        }
        if let Some(path) = asset(component_type::EFFECT)
            && let Ok(text) = access.read(&path)
            && let Ok(effect) = parse_particle_effect(&text)
        {
            for (name, parameter) in &effect.asset().parameters {
                let entry = match parameter {
                    ParticleParameter::Scalar { value, min, max } => (json_number(*value), Some(*min), Some(*max)),
                    ParticleParameter::Vector3 { value, .. } => {
                        (Value::from(value.map(json_number).to_vec()), None, None)
                    }
                    ParticleParameter::Color { value, .. } => {
                        (Value::from(value.map(json_number).to_vec()), None, None)
                    }
                };
                found.push((component_type::EFFECT.to_owned(), name.clone(), entry.0, entry.1, entry.2));
            }
        }
        for (component, name, value, min, max) in found {
            self.metadata
                .insert(format!("{component}.parameters.{name}"), FieldMetadata { min, max, ..Default::default() });
            self.parameter_defaults.push((component, name, value));
        }
    }

    /// A fresh entity id unlike any in the level.
    fn new_id(&mut self) -> String {
        loop {
            let mut digits = String::with_capacity(36);
            for group in [8, 4, 4, 4, 12] {
                if !digits.is_empty() {
                    digits.push('-');
                }
                for _ in 0..group {
                    let nibble = (self.ids.next_unit() * 16.0) as u32;
                    digits.push(char::from_digit(nibble.min(15), 16).unwrap_or('0'));
                }
            }
            if !self.document.level().entities.iter().any(|entity| entity.id == digits) {
                return digits;
            }
        }
    }
}

/// An asset-declared parameter: component, name, default value, and numeric bounds.
type FoundParameter = (String, String, Value, Option<f64>, Option<f64>);

/// Parse a level, or a `.joyobject` as a one-entity level with that entity selected.
fn parse_document(path: &str, text: &str) -> Result<(LevelDocument, bool), String> {
    if path.to_lowercase().ends_with(".joyobject") {
        let object = parse_object(text).map_err(|error| error.to_string())?;
        let mut level = LevelData::new(object.name.clone());
        let id = object.entity.id.clone();
        level.entities.push(object.entity);
        let mut document = LevelDocument::new(&level).map_err(|error| error.to_string())?;
        document.select(Some(&id));
        return Ok((document, true));
    }
    let level = parse_level(text).map_err(|error| error.to_string())?;
    Ok((LevelDocument::new(&level).map_err(|error| error.to_string())?, false))
}

/// FORM parameter records (`name`, `type`, `value`, `min`, `max`, …) from a `.form` or `.formlab` text, or none when it
/// does not compile.
pub fn form_parameters(path: &str, text: &str) -> Vec<Value> {
    let (source, overrides, materials) = if path.ends_with(".formlab") {
        let Ok(document) = serde_json::from_str::<Value>(text) else {
            return Vec::new();
        };
        let source = document.get("source").and_then(Value::as_str).unwrap_or_default().to_owned();
        let overrides =
            document.get("overrides").cloned().filter(Value::is_object).unwrap_or(Value::Object(Map::new()));
        (source, overrides, document.get("materials").cloned())
    } else {
        (text.to_owned(), Value::Object(Map::new()), None)
    };
    joy_engine::form::compile(&source, &overrides, materials.as_ref())
        .map(|geometry| geometry.parameters)
        .unwrap_or_default()
}

/// Set (or with `None`, remove) a nested own property, creating intermediate records.
pub fn set_path(root: &mut Value, path: &[String], value: Option<Value>) {
    let Some((key, parents)) = path.split_last() else {
        if let Some(value) = value {
            *root = value;
        }
        return;
    };
    let mut target = root;
    for part in parents {
        let next = match target {
            Value::Array(items) => match part.parse::<usize>() {
                Ok(index) if index < items.len() => &mut items[index],
                _ => return,
            },
            Value::Object(record) => {
                let entry = record.entry(part.clone()).or_insert(Value::Null);
                if !entry.is_object() && !entry.is_array() {
                    *entry = Value::Object(Map::new());
                }
                entry
            }
            _ => return,
        };
        target = next;
    }
    match (target, value) {
        (Value::Array(items), None) => {
            if let Ok(index) = key.parse::<usize>()
                && index < items.len()
            {
                items.remove(index);
            }
        }
        (Value::Array(items), Some(value)) => {
            if let Ok(index) = key.parse::<usize>() {
                if index < items.len() {
                    items[index] = value;
                } else if index == items.len() {
                    items.push(value);
                }
            }
        }
        (Value::Object(record), None) => {
            record.shift_remove(key);
        }
        (Value::Object(record), Some(value)) => {
            record.insert(key.clone(), value);
        }
        _ => {}
    }
}

/// Bounds for transform channels: finite values only.
pub fn transform_bounds(field: TransformField) -> NumberBounds {
    NumberBounds { step: Some(field.step()), ..Default::default() }
}
