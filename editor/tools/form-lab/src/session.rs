// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The working session: the document being edited, its compile lifecycle, the last accepted build, property history,
//! variants, browser recovery, and the save and export artifacts. This is the UI-free half of the TypeScript
//! composition root (`site/src/main.ts`); the workbench (`site/src/main.rs`) draws it and the preview displays its
//! builds.
//!
//! The session keeps four things distinct, as the TypeScript tool does: browser recovery (origin-local autosave of
//! the whole document), document saving (`.form` writes the raw script exactly; `.formlab` keeps overrides,
//! materials, meshes, and variants), the last accepted build (what the preview shows and exports use), and baked
//! exports (which never carry the recipe).

use joy_engine::development::PropertyHistory;
use joy_engine::form::{Diagnostic, GeometryData, default_material, read_materials};
use joy_engine::runtime::{Stopwatch, Storage};
use joy_engine::serde_json::{Map, Value as Json};

use crate::compiler::{CompileOutcome, CompilePhase, CompileRequest, Compiler};
use crate::document::{
    DocumentError, FormDocument, MAXIMUM_SOURCE_LENGTH, Overrides, ParameterVariant, read_document, read_overrides,
    read_variants, serialize_document, utf16_length,
};
use crate::export::{export_geometry_json, export_obj, export_ply};
use crate::export_glb::export_glb;
use crate::parameter_values::{Parameter, is_property_override, parameter_signature, read_parameters};
use crate::project_metadata::project_name_change;

/// Storage key of browser recovery, shared with the TypeScript tool's `localStorage` key.
pub const RECOVERY_KEY: &str = "joy-form-lab-v1";

/// Delay between the last edit and a live rebuild, in milliseconds.
pub const LIVE_DEBOUNCE_MILLISECONDS: f64 = 240.0;

/// How the status line reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatusState {
    /// Settled.
    #[default]
    Ready,
    /// Waiting for or running a build.
    Busy,
    /// The last build or action failed.
    Error,
}

/// The status line under the script.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// What happened.
    pub message: String,
    /// How it reads.
    pub state: StatusState,
}

/// The preview badge: whether what is on screen is the latest script.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewState {
    /// Edits are waiting for a build.
    #[default]
    Pending,
    /// The preview shows the latest build.
    Accepted,
    /// The latest build failed; the previous one stays on screen.
    Failed,
}

/// Saved formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    /// The raw script (`.form`).
    Form,
    /// The FORM LAB document (`.formlab`).
    FormLab,
}

/// Baked export formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    /// glTF binary with authored materials.
    Glb,
    /// Wavefront OBJ.
    Obj,
    /// ASCII PLY.
    Ply,
    /// Generated geometry JSON.
    Json,
}

/// A format the artifact dialog offers: its extension, label, media type, and description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtifactFormat {
    /// Extension without the dot; also the format id.
    pub id: &'static str,
    /// Menu label.
    pub label: &'static str,
    /// Media type for downloads and save pickers.
    pub mime: &'static str,
    /// What the file contains.
    pub description: &'static str,
}

/// The Save formats (`SAVE_FORMATS`).
pub const SAVE_FORMATS: [ArtifactFormat; 2] = [
    ArtifactFormat {
        id: "form",
        label: "FORM script (.form)",
        mime: "text/plain",
        description: "Editable script source. Inspector overrides, named variants, imported meshes, and document \
                      materials are not included. Edit script defaults to keep tuned values in this file.",
    },
    ArtifactFormat {
        id: "formlab",
        label: "FORM LAB document (.formlab)",
        mime: "application/json",
        description: "Script, parameter overrides, materials, imported meshes, and named variants. Opens in FORM LAB and \
                      Joy Editor; Joy Engine can rebuild it as a scene asset.",
    },
];

/// The Export formats (`EXPORT_FORMATS`).
pub const EXPORT_FORMATS: [ArtifactFormat; 4] = [
    ArtifactFormat {
        id: "glb",
        label: "GLB (.glb)",
        mime: "model/gltf-binary",
        description: "Baked geometry with authored materials and textures. Does not include the procedural script.",
    },
    ArtifactFormat {
        id: "obj",
        label: "OBJ (.obj)",
        mime: "text/plain",
        description: "Baked geometry without materials or the procedural script.",
    },
    ArtifactFormat {
        id: "ply",
        label: "PLY (.ply)",
        mime: "text/plain",
        description: "Baked mesh and point data for geometry tools. Does not include the procedural script.",
    },
    ArtifactFormat {
        id: "json",
        label: "Geometry data (.json)",
        mime: "application/json",
        description: "Generated geometry and metadata for custom integrations. This is not an editable FORM LAB \
                      document.",
    },
];

/// Statistics of the accepted build.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BuildStatistics {
    /// Evaluation time in milliseconds.
    pub milliseconds: f64,
    /// Vertices.
    pub vertices: usize,
    /// Triangles.
    pub triangles: usize,
    /// Points.
    pub points: usize,
    /// Numeric payload in bytes (eight per number), excluding preview buffers and exports.
    pub data_bytes: usize,
}

/// What a frame's update asks the workbench to do.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionEvent {
    /// A build succeeded; show it in the preview, then call [`Session::accept_build`] or
    /// [`Session::reject_preview`].
    BuildReady {
        /// The generated geometry.
        data: Box<GeometryData>,
        /// Evaluation time in milliseconds.
        milliseconds: f64,
        /// Whether the preview should frame it (the first build of a document).
        frame: bool,
    },
    /// A build failed at a source location; mark it in the editor and offer Go to error.
    BuildRejected(Diagnostic),
}

/// Bounded property history snapshots: the overrides that are numbers, vectors, colors, or materials.
pub type PropertySnapshot = Overrides;

/// The working session for one FORM LAB view.
pub struct Session {
    // The document.
    /// Display name (also the project metadata name).
    pub name: String,
    /// Script source, borrowed by the script editor.
    pub source: String,
    overrides: Overrides,
    materials: Vec<Json>,
    variants: Vec<ParameterVariant>,

    // The last accepted build and its controls.
    data: Option<GeometryData>,
    parameters: Vec<Parameter>,
    statistics: BuildStatistics,
    project: Option<Json>,

    // Compile lifecycle.
    compiler: Compiler,
    live: bool,
    pending_since: Option<Stopwatch>,
    should_frame: bool,
    status: Status,
    error_location: Option<(usize, usize)>,
    preview: PreviewState,
    export_stale: bool,

    // Edits, history, and recovery.
    has_edits: bool,
    history: PropertyHistory<PropertySnapshot>,
    recovery: Option<Storage>,
    save_state: Option<(String, String)>,
}

impl Session {
    /// A session for `initial`, or for the browser recovery copy in `recovery` when one exists (recovery may be the only
    /// copy of the work, so it keeps replacement protection). `recovery` is `None` when hosted.
    pub fn new(initial: FormDocument, recovery: Option<Storage>) -> Self {
        let restored = recovery
            .as_ref()
            .and_then(|storage| storage.load(RECOVERY_KEY).ok().flatten())
            .and_then(|text| read_document(&text).ok());
        let has_edits = restored.is_some();
        let document = restored.unwrap_or(initial);
        let mut session = Self {
            name: document.name.clone(),
            source: document.source.clone(),
            overrides: document.overrides.clone(),
            materials: read_materials(
                document.materials.as_ref().map(|materials| Json::Array(materials.clone())).as_ref(),
            )
            .unwrap_or_else(|_| vec![default_material()]),
            variants: document.variants.clone().unwrap_or_default(),
            data: None,
            parameters: Vec::new(),
            statistics: BuildStatistics::default(),
            project: None,
            compiler: Compiler::new(),
            live: true,
            pending_since: None,
            should_frame: true,
            status: Status { message: "Preparing your workspace…".into(), state: StatusState::Busy },
            error_location: None,
            preview: PreviewState::Pending,
            export_stale: true,
            has_edits,
            history: PropertyHistory::new(Map::new()),
            recovery,
            save_state: None,
        };
        session.history.reset(session.property_snapshot());
        session
    }

    /// Use a compiler with other budgets (tests).
    pub fn set_compiler(&mut self, compiler: Compiler) {
        self.compiler = compiler;
    }

    /// The document as saved to `.formlab` and recovery: palette and variants always included.
    pub fn document(&self) -> FormDocument {
        FormDocument {
            name: self.name.clone(),
            source: self.source.clone(),
            overrides: self.overrides.clone(),
            materials: Some(self.materials.clone()),
            variants: Some(self.variants.clone()),
        }
    }

    /// Replace the working document (Examples, Open, or a host). Starts a framed build.
    pub fn load(&mut self, document: FormDocument) {
        self.compiler.cancel();
        self.pending_since = None;
        self.parameters.clear();
        self.name = document.name;
        self.source = document.source;
        self.overrides = document.overrides;
        self.materials =
            read_materials(document.materials.map(Json::Array).as_ref()).unwrap_or_else(|_| vec![default_material()]);
        self.variants = document.variants.unwrap_or_default();
        self.should_frame = true;
        self.has_edits = false;
        self.save_state = None;
        self.history.reset(self.property_snapshot());
        self.persist();
        self.compile();
    }

    /// Read a file the author opened: `.form` and `.txt` are raw script, anything else a FORM LAB document.
    pub fn read_file(name: &str, text: &str) -> Result<FormDocument, DocumentError> {
        if text.len() > joy_engine::form::DOCUMENT_FILE_BYTES {
            return Err(DocumentError::TooLarge);
        }
        let lower = name.to_ascii_lowercase();
        let document = if lower.ends_with(".form") || lower.ends_with(".txt") {
            let stem = name.rfind('.').map_or(name, |dot| &name[..dot]);
            FormDocument::from_script(stem, text)
        } else {
            read_document(text)?
        };
        if utf16_length(&document.source) > MAXIMUM_SOURCE_LENGTH {
            return Err(DocumentError::Overrides("Script size limit is 200,000 characters.".into()));
        }
        Ok(document)
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------------------------------------------------

    /// Parameter overrides.
    pub fn overrides(&self) -> &Overrides {
        &self.overrides
    }

    /// The validated material palette, Default first.
    pub fn materials(&self) -> &[Json] {
        &self.materials
    }

    /// Named variants.
    pub fn variants(&self) -> &[ParameterVariant] {
        &self.variants
    }

    /// The last accepted full build, the source of every export.
    pub fn data(&self) -> Option<&GeometryData> {
        self.data.as_ref()
    }

    /// Parameters of the last accepted build, with the latest edits applied to their values.
    pub fn parameters(&self) -> &[Parameter] {
        &self.parameters
    }

    /// Mutable parameters, for controls that edit values in place before committing them.
    pub fn parameters_mut(&mut self) -> &mut [Parameter] {
        &mut self.parameters
    }

    /// Declaration signature of the current parameters (changes only when controls must rebuild).
    pub fn parameter_signature(&self) -> String {
        parameter_signature(&self.parameters)
    }

    /// Statistics of the accepted build.
    pub fn statistics(&self) -> BuildStatistics {
        self.statistics
    }

    /// Project metadata of the accepted build (`name`, `title`, `info`).
    pub fn project(&self) -> Option<&Json> {
        self.project.as_ref()
    }

    /// The status line.
    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The located error of the latest failed build, for Go to error.
    pub fn error_location(&self) -> Option<(usize, usize)> {
        self.error_location
    }

    /// The preview badge state.
    pub fn preview_state(&self) -> PreviewState {
        self.preview
    }

    /// The preview badge text.
    pub fn preview_text(&self) -> &'static str {
        match (self.preview, self.data.is_some()) {
            (PreviewState::Accepted, _) => "Preview accepted",
            (PreviewState::Pending, true) => "Preview pending · last accepted geometry",
            (PreviewState::Pending, false) => "Preview pending · no accepted geometry",
            (PreviewState::Failed, true) => "Preview failed · last accepted geometry",
            (PreviewState::Failed, false) => "Preview failed · no accepted geometry",
        }
    }

    /// The export note: whether exports use a stale build.
    pub fn export_note(&self) -> &'static str {
        if self.export_stale {
            "Changes pending or build failed. Export uses the previous successful build, without the recipe."
        } else {
            "Export uses the last successful build. Save keeps editable source; export does not."
        }
    }

    /// Whether live rebuilds are on.
    pub fn live(&self) -> bool {
        self.live
    }

    /// Whether the compiler is starting or evaluating.
    pub fn compiling(&self) -> bool {
        self.compiler.phase() != CompilePhase::Idle
    }

    /// Whether a live rebuild is waiting for its debounce.
    pub fn rebuild_scheduled(&self) -> bool {
        self.pending_since.is_some()
    }

    /// Whether there is work the author might lose by replacing the document.
    pub fn has_edits(&self) -> bool {
        self.has_edits
    }

    /// Record that the current document was written by a host.
    pub fn mark_saved(&mut self) {
        self.has_edits = false;
    }

    /// The last save or recovery message and its tooltip.
    pub fn save_state(&self) -> Option<(&str, &str)> {
        self.save_state.as_ref().map(|(text, title)| (text.as_str(), title.as_str()))
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Edits
    // -----------------------------------------------------------------------------------------------------------------

    /// The script text changed (the editor already wrote it into [`source`](Self::source)).
    pub fn source_changed(&mut self) {
        self.changed();
    }

    /// Set or remove one parameter override from a control. Call [`commit_properties`](Self::commit_properties) when the
    /// gesture ends.
    pub fn set_override(&mut self, name: &str, value: Option<Json>) {
        match value {
            Some(value) => {
                self.overrides.insert(name.to_owned(), value);
            }
            None => {
                self.overrides.shift_remove(name);
            }
        }
        self.has_edits = true;
        self.persist();
        self.schedule();
    }

    /// Record a property history entry after a committed control gesture.
    pub fn commit_properties(&mut self) {
        let snapshot = self.property_snapshot();
        self.history.commit(snapshot);
    }

    /// Remove one override; the next build computes its default. Participates in property undo.
    pub fn reset_parameter(&mut self, name: &str) {
        self.overrides.shift_remove(name);
        for parameter in &mut self.parameters {
            match parameter {
                Parameter::Numeric(numeric) if numeric.name == name => numeric.value = numeric.default_value.clone(),
                Parameter::Resource(resource) if resource.name == name => {
                    resource.value = resource.default_value.clone()
                }
                _ => {}
            }
        }
        self.commit_properties();
        self.has_edits = true;
        self.persist();
        self.schedule();
    }

    /// Restore every parameter default (**Reset params**); materials and preview lighting are unchanged.
    pub fn reset_parameters(&mut self) {
        self.overrides.clear();
        self.commit_properties();
        self.has_edits = true;
        self.persist();
        self.compile();
    }

    /// Whether property undo is available.
    pub fn can_undo_properties(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether property redo is available.
    pub fn can_redo_properties(&self) -> bool {
        self.history.can_redo()
    }

    /// Undo the last property edit.
    pub fn undo_properties(&mut self) -> bool {
        let current = self.property_snapshot();
        match self.history.undo(&current) {
            Some(snapshot) => {
                self.restore_properties(snapshot);
                true
            }
            None => false,
        }
    }

    /// Redo the last undone property edit.
    pub fn redo_properties(&mut self) -> bool {
        let current = self.property_snapshot();
        match self.history.redo(&current) {
            Some(snapshot) => {
                self.restore_properties(snapshot);
                true
            }
            None => false,
        }
    }

    /// The overrides property history covers; imported meshes stay current.
    pub fn property_snapshot(&self) -> PropertySnapshot {
        self.overrides
            .iter()
            .filter(|(_, value)| is_property_override(value))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect()
    }

    fn restore_properties(&mut self, snapshot: PropertySnapshot) {
        self.overrides.retain(|_, value| !is_property_override(value));
        self.overrides.extend(snapshot);
        for parameter in &mut self.parameters {
            match parameter {
                Parameter::Numeric(numeric) => {
                    numeric.value = match self.overrides.get(&numeric.name) {
                        Some(Json::Number(number)) => vec![number.as_f64().unwrap_or_default()],
                        Some(Json::Array(values)) => values.iter().filter_map(Json::as_f64).collect(),
                        _ => numeric.default_value.clone(),
                    };
                }
                Parameter::Resource(resource) => {
                    resource.value =
                        self.overrides.get(&resource.name).cloned().unwrap_or_else(|| resource.default_value.clone());
                }
            }
        }
        self.has_edits = true;
        self.persist();
        self.schedule();
    }

    /// Replace the document palette from the Materials inspector.
    pub fn set_materials(&mut self, materials: Vec<Json>) {
        self.materials = materials;
        self.changed();
    }

    /// Rename the document through the script's project metadata; returns the new source on success.
    pub fn rename(&mut self, next: &str) -> Result<(), String> {
        let next = crate::document::trim_javascript_whitespace(next);
        if next.is_empty() || next == self.name {
            return Ok(());
        }
        match project_name_change(&self.source, next) {
            Ok(change) => {
                self.name = next.to_owned();
                self.source = change.apply(&self.source);
                self.source_changed();
                Ok(())
            }
            Err(error) => {
                let message = format!("Fix the script before renaming its project metadata: {error}");
                self.set_status(message.clone(), StatusState::Error);
                Err(message)
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Variants
    // -----------------------------------------------------------------------------------------------------------------

    /// Save the current overrides as a new named variant. The document must still serialize.
    pub fn save_variant(&mut self, name: &str) -> Result<usize, String> {
        let mut next = variants_json(&self.variants);
        next.push(variant_json(name, &self.overrides));
        let variants = read_variants(&Json::Array(next)).map_err(|error| error.to_string())?;
        self.validate_variants(&variants)?;
        self.variants = variants;
        self.has_edits = true;
        self.persist();
        Ok(self.variants.len() - 1)
    }

    /// Replace the current overrides with a variant's and rebuild; removed parameters are ignored by FORM.
    pub fn apply_variant(&mut self, index: usize) -> Result<String, String> {
        let variant = self.variants.get(index).ok_or("Choose a saved variant.")?.clone();
        let overrides = read_overrides(&Json::Object(variant.overrides.clone())).map_err(|error| error.to_string())?;
        let mut document = self.document();
        document.overrides = overrides.clone();
        serialize_document(&document).map_err(|error| error.to_string())?;
        self.overrides = overrides;
        self.parameters.clear();
        self.changed();
        self.compile();
        Ok(variant.name)
    }

    /// Rename a variant.
    pub fn rename_variant(&mut self, index: usize, name: &str) -> Result<(), String> {
        let mut next = variants_json(&self.variants);
        let entry = next.get_mut(index).ok_or("Choose a saved variant.")?;
        entry["name"] = Json::String(name.to_owned());
        let variants = read_variants(&Json::Array(next)).map_err(|error| error.to_string())?;
        self.validate_variants(&variants)?;
        self.variants = variants;
        self.has_edits = true;
        self.persist();
        Ok(())
    }

    /// Delete a variant; current parameters are unchanged.
    pub fn delete_variant(&mut self, index: usize) -> Result<(), String> {
        if index >= self.variants.len() {
            return Err("Choose a saved variant.".into());
        }
        self.variants.remove(index);
        self.has_edits = true;
        self.persist();
        Ok(())
    }

    fn validate_variants(&self, variants: &[ParameterVariant]) -> Result<(), String> {
        let mut document = self.document();
        document.variants = Some(variants.to_vec());
        serialize_document(&document).map(|_| ()).map_err(|error| error.to_string())
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Compile lifecycle
    // -----------------------------------------------------------------------------------------------------------------

    /// Turn live rebuilds on (building now) or off.
    pub fn set_live(&mut self, live: bool) {
        self.live = live;
        if live {
            self.compile();
        } else {
            self.compiler.cancel();
            self.pending_since = None;
            self.set_status("Live preview paused. Run to update.", StatusState::Busy);
        }
    }

    /// Build now (Run, Ctrl/Cmd+Enter, or a document change).
    pub fn compile(&mut self) {
        self.compiler.cancel();
        self.pending_since = None;
        self.preview = PreviewState::Pending;
        self.export_stale = true;
        self.set_status("Starting compiler…", StatusState::Busy);
        self.compiler.start(CompileRequest {
            source: self.source.clone(),
            overrides: Json::Object(self.overrides.clone()),
            materials: Some(Json::Array(self.materials.clone())),
        });
    }

    /// Advance debounces and the compiler; returns what the workbench must do this frame.
    pub fn update(&mut self) -> Option<SessionEvent> {
        if self.pending_since.is_some_and(|since| since.elapsed_milliseconds() >= LIVE_DEBOUNCE_MILLISECONDS) {
            self.compile();
        }
        let starting = self.compiler.phase() == CompilePhase::Starting;
        let outcome = self.compiler.poll();
        if starting && outcome.is_none() && self.compiler.phase() == CompilePhase::Compiling {
            self.set_status("Building geometry…", StatusState::Busy);
        }
        match outcome? {
            CompileOutcome::Built { data, milliseconds } => {
                Some(SessionEvent::BuildReady { data, milliseconds, frame: self.should_frame })
            }
            outcome => {
                self.preview = PreviewState::Failed;
                let message = outcome.failure_message().unwrap_or_default();
                self.set_status(message, StatusState::Error);
                if let CompileOutcome::Rejected(diagnostic) = outcome {
                    self.error_location = Some((diagnostic.line, diagnostic.column));
                    return Some(SessionEvent::BuildRejected(diagnostic));
                }
                None
            }
        }
    }

    /// Wait for the current build and handle it as [`update`](Self::update) would (tests and headless runs).
    pub fn finish_build(&mut self) -> Option<SessionEvent> {
        if self.pending_since.is_some() {
            self.compile();
        }
        let mut waited = Stopwatch::start();
        loop {
            if let Some(event) = self.update() {
                return Some(event);
            }
            if !self.compiling() {
                return None;
            }
            if waited.elapsed_milliseconds() > 30_000.0 {
                waited = Stopwatch::start();
                joy_engine::log::warn!("FORM LAB is still waiting for a build");
            }
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// The preview accepted a build: it becomes the export source and drives the parameter controls.
    pub fn accept_build(&mut self, data: GeometryData, milliseconds: f64) {
        if let Some(name) = data.project.as_ref().and_then(|project| project.get("name")).and_then(Json::as_str) {
            self.name = name.to_owned();
        }
        self.project = data.project.clone();
        self.should_frame = false;
        self.parameters = read_parameters(&data.parameters);
        let numbers = data.positions.len()
            + data.normals.len()
            + data.uvs.len()
            + data.triangle_materials.len()
            + data.point_materials.len()
            + data.colors.len()
            + data.alphas.len()
            + data.triangles.len()
            + data.points.len();
        self.statistics = BuildStatistics {
            milliseconds,
            vertices: data.positions.len() / 3,
            triangles: data.triangles.len() / 3,
            points: data.points.len(),
            data_bytes: numbers * 8,
        };
        self.preview = PreviewState::Accepted;
        self.export_stale = false;
        self.error_location = None;
        self.set_status(
            format!("Built successfully · {} vertices", group_thousands(self.statistics.vertices)),
            StatusState::Ready,
        );
        self.data = Some(data);
        self.persist();
    }

    /// The preview could not show a build (for example a material map failed to decode); the previous build stays.
    pub fn reject_preview(&mut self, reason: &str) {
        self.preview = PreviewState::Failed;
        self.export_stale = true;
        let message = if reason.is_empty() {
            "Preview resources rejected this build. Previous preview and export retained.".to_owned()
        } else {
            format!("Preview resources rejected this build. Previous preview and export retained. {reason}")
        };
        self.set_status(message, StatusState::Error);
    }

    /// Replace the status line (and clear Go to error, as the TypeScript `status()` does).
    pub fn set_status(&mut self, message: impl Into<String>, state: StatusState) {
        self.error_location = None;
        self.status = Status { message: message.into(), state };
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Saving, exporting, and recovery
    // -----------------------------------------------------------------------------------------------------------------

    /// The text to save: the raw script exactly for `.form`, the full document for `.formlab`.
    pub fn save_contents(&self, format: SaveFormat) -> Result<String, String> {
        match format {
            SaveFormat::Form => Ok(self.source.clone()),
            SaveFormat::FormLab => serialize_document(&self.document()).map_err(|error| error.to_string()),
        }
    }

    /// A save finished. A source-only save keeps replacement protection while inspector data would be lost.
    pub fn save_completed(&mut self, format: SaveFormat, contents: &str, saved: bool) {
        let current = self.document();
        let source_only =
            current.overrides.is_empty() && self.variants.is_empty() && self.materials == vec![default_material()];
        let matches = match format {
            SaveFormat::FormLab => serialize_document(&current).is_ok_and(|text| text == contents),
            SaveFormat::Form => contents == current.source && source_only,
        };
        if matches {
            self.has_edits = false;
        }
        let kind = if format == SaveFormat::Form { "FORM script" } else { "FORM LAB document" };
        let text = format!("{kind} {}", if saved { "saved" } else { "download requested" });
        self.save_state = Some((text, String::new()));
    }

    /// The bytes of a baked export of the last accepted build; pending or failed source never replaces it.
    pub fn export(&self, format: ExportFormat) -> Result<Vec<u8>, String> {
        let data = self.data.as_ref().ok_or("Build geometry before exporting.")?;
        match format {
            ExportFormat::Glb => export_glb(data).map_err(|error| error.to_string()),
            ExportFormat::Obj => Ok(export_obj(data).into_bytes()),
            ExportFormat::Ply => Ok(export_ply(data).into_bytes()),
            ExportFormat::Json => export_geometry_json(data).map(String::into_bytes).map_err(|error| error.to_string()),
        }
    }

    /// Turn browser recovery on (standalone) or off (hosted, where the editor owns files).
    pub fn set_recovery(&mut self, recovery: Option<Storage>) {
        self.recovery = recovery;
    }

    /// Whether browser recovery is active (standalone only).
    pub fn recovers(&self) -> bool {
        self.recovery.is_some()
    }

    /// Write browser recovery (standalone only).
    pub fn persist(&mut self) {
        let Some(storage) = &self.recovery else {
            return;
        };
        let text = self.document().to_json().to_string();
        self.save_state = Some(match storage.save(RECOVERY_KEY, &text) {
            Ok(()) => (
                "Browser recovery updated".to_owned(),
                "Recovery stays in this browser. Save a .form script, or choose .formlab to include inspector overrides, \
                 materials, and variants."
                    .to_owned(),
            ),
            Err(_) => ("Browser recovery unavailable · use Save".to_owned(), String::new()),
        });
    }

    /// An edit to the source, materials, or all overrides at once (a variant): restart property history (script edits
    /// have their own undo, as in TypeScript), mark edits, persist, and schedule a build.
    fn changed(&mut self) {
        self.history.reset(self.property_snapshot());
        self.has_edits = true;
        self.persist();
        self.schedule();
    }

    /// Mark the preview pending and debounce a live build (or ask for Run).
    fn schedule(&mut self) {
        self.preview = PreviewState::Pending;
        self.compiler.cancel();
        self.export_stale = true;
        if self.live {
            self.set_status("Waiting for your next idea…", StatusState::Busy);
            self.pending_since = Some(Stopwatch::start());
        } else {
            self.pending_since = None;
            self.set_status("Edited · Run to update the preview.", StatusState::Busy);
        }
    }
}

/// A count with comma thousands separators, as `toLocaleString()` prints it in English.
pub fn group_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut output = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(digit);
    }
    output
}

fn variants_json(variants: &[ParameterVariant]) -> Vec<Json> {
    variants.iter().map(|variant| variant_json(&variant.name, &variant.overrides)).collect()
}

fn variant_json(name: &str, overrides: &Overrides) -> Json {
    let mut record = Map::new();
    record.insert("name".into(), Json::String(name.to_owned()));
    record.insert("overrides".into(), Json::Object(overrides.clone()));
    Json::Object(record)
}
