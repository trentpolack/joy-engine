// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM EDITOR's composition root and workbench (UI module).
//!
//! [`FormEditor`] owns the session (document, compile lifecycle, history, recovery), the engine preview, the Output
//! inspector, and the workbench UI: header, project bar, and the Script, Preview, and Inspector panes. It is a [`App`]
//! for standalone runs and an [`AuthoringTool`] for Joy Editor tabs, with the same implementation and formats in both.

use joy_engine::development::components;
use joy_engine::development::{
    AuthoringTool,
    FileDialogEvent,
    FileDialogs,
    FileFilter,
    NumericDrafts,
    OpenRequest,
    PanelLayout,
    SaveRequest,
    ScriptDiagnostic,
    ScriptEditor,
    ToolDocument,
    ToolFileReader,
    ToolHosting,
    ViewportGesture,
    ViewportShortcut,
    viewport_input,
};
use joy_engine::egui::{
    self,
    Color32,
    RichText,
    Ui,
    UiBuilder,
};
use joy_engine::math::Vec2;
use joy_engine::rendering::{
    GpuBackend,
    PostProcessSettings,
};
use joy_engine::runtime::theme::{
    colors,
    layout,
};
use joy_engine::runtime::{
    App,
    AppResult,
    Frame,
    RenderTarget,
    StartContext,
    Storage,
};

use crate::artifact_dialog::{
    ArtifactDialog,
    ArtifactKind,
    ArtifactRequest,
};
use crate::constants::ViewMode;
use crate::document::{
    FormDocument,
    file_stem,
    serialize_document,
};
use crate::editor::create_editor;
use crate::form_assets::{
    DEFAULT_FORM_ASSET_PATH,
    create_initial_form_document,
    form_assets,
};
use crate::form_library::{
    FormLibraryAsset,
    library_reader,
    open_form_library_asset,
};
use crate::material_controls::{
    MaterialAction,
    MaterialControls,
    image_data_url,
    merge_materialx,
    with_texture,
};
use crate::materialx::{
    export_materialx,
    import_materialx,
};
use crate::output_inspection::OutputInspection;
use crate::output_inspector::{
    attribute_panel,
    result_toolbar,
};
use crate::parameter_controls::{
    ParameterAction,
    ParameterControls,
};
use crate::parameter_values::{
    MAXIMUM_GLB_BYTES,
    import_glb_mesh,
};
use crate::preview_lighting_controls::PreviewLightingControls;
use crate::reference_dialog::ReferenceDialog;
use crate::rendering::viewport::{
    PaneRect,
    Viewport,
};
use crate::script_guide::{
    RECIPES,
    append_recipe,
};
use crate::session::{
    ExportFormat,
    PreviewState,
    SaveFormat,
    Session,
    SessionEvent,
    StatusState,
    group_thousands,
};
use crate::variant_controls::{
    VariantAction,
    VariantControls,
};
use crate::workspace_layout::{
    InspectorTab,
    create_panel_layout,
    inspector_tabs,
    view_menu,
};

/// The tool's version, shown beside its name.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Product name shown in the standalone header.
const PRODUCT_NAME: &str = "FORM EDITOR";

/// Storage folder (natively) for recovery and layout preferences; keeps the FORM LAB spelling so existing recovery
/// drafts and saved layouts keep loading after the FORM EDITOR rename.
pub const STORAGE_APPLICATION: &str = "form-lab";

/// Largest MaterialX file imported, in bytes.
const MAXIMUM_MATERIALX_BYTES: usize = 1_000_000;

/// Below this preview width, give the camera actions their own row instead of covering the heading.
const COMPACT_PREVIEW_WIDTH: f32 = 540.0;

/// What the pending file dialog is for.
#[derive(Clone, Debug, PartialEq)]
enum FilePurpose {
    OpenDocument,
    ImportMesh(String),
    ChooseTexture { material: String, field: &'static str },
    ImportMaterialX,
    ExportMaterialX,
    Artifact { kind: ArtifactKind, format: &'static str, contents: String },
}

/// A replacement waiting for the author's confirmation because the current study has edits.
#[derive(Clone)]
struct PendingReplacement {
    prompt: &'static str,
    source: DocumentSource,
}

/// A document to load and where it came from: the project reader its references and modules load through, and its
/// path relative to that reader's root.
#[derive(Clone)]
struct DocumentSource {
    document: FormDocument,
    files: Option<ToolFileReader>,
    path: String,
    // The native folder the reader's root stands for, so edited referenced scripts can be written back.
    disk_root: Option<std::path::PathBuf>,
}

/// FORM EDITOR: one study, its preview, and the workbench around them.
pub struct FormEditor {
    // The session and what displays it.
    session: Session,
    viewport: Viewport,
    inspection: OutputInspection,
    assets: Vec<FormLibraryAsset>,

    // Workbench UI state.
    editor: ScriptEditor,
    layout: PanelLayout,
    drafts: NumericDrafts,
    tab: InspectorTab,
    parameters: ParameterControls,
    materials: MaterialControls,
    lighting: PreviewLightingControls,
    variants: VariantControls,
    artifact: ArtifactDialog,
    reference: ReferenceDialog,
    dialogs: FileDialogs,
    file_purpose: Option<FilePurpose>,
    replacement: Option<PendingReplacement>,
    name_field: String,
    name_focused: bool,
    cursor: (usize, usize),
    project_info: bool,
    title_open: bool,
    reveal_attributes: bool,
    gpu_error: Option<String>,
    backend: String,
    hosting: ToolHosting,
    raw_form: bool,
    // The host's project reader (Joy Editor), handed to documents it opens.
    host_files: Option<ToolFileReader>,
    // The native folder the current document's reader reads from, when it was opened from disk.
    disk_root: Option<std::path::PathBuf>,
    themed: bool,
    // Where the main controls were drawn last frame, for hosts and interface tests.
    targets: std::collections::HashMap<&'static str, egui::Rect>,
    panes: joy_engine::development::PanelRects,
}

impl FormEditor {
    /// A standalone FORM EDITOR with browser recovery and layout preferences in the default storage.
    pub fn new() -> Self {
        Self::with_storage(ToolHosting::Standalone, Some(Storage::new(STORAGE_APPLICATION)))
    }

    /// A FORM EDITOR for Joy Editor: no browser recovery (the editor owns files); layout preferences persist.
    pub fn hosted() -> Self {
        Self::with_storage(
            ToolHosting::Hosted {
                area: egui::Rect::NOTHING,
            },
            Some(Storage::new(STORAGE_APPLICATION)),
        )
    }

    /// A FORM EDITOR with explicit storage (`None` keeps nothing between sessions), for tests and hosts.
    pub fn with_storage(hosting: ToolHosting, storage: Option<Storage>) -> Self {
        let assets = form_assets();
        let recovery = if hosting == ToolHosting::Standalone { storage.clone() } else { None };
        let mut session = Session::new(create_initial_form_document(&assets), recovery);
        // The first study comes from the library unless recovery restored the author's own work.
        if !session.has_edits() {
            session.set_files(Some(library_reader(&assets)), DEFAULT_FORM_ASSET_PATH);
        }
        let name_field = session.name.clone();
        Self {
            session,
            viewport: Viewport::new(),
            inspection: OutputInspection::new(),
            assets,
            editor: create_editor(),
            layout: create_panel_layout(storage),
            drafts: NumericDrafts::default(),
            tab: InspectorTab::Parameters,
            parameters: ParameterControls::default(),
            materials: MaterialControls::new(),
            lighting: PreviewLightingControls::default(),
            variants: VariantControls::default(),
            artifact: ArtifactDialog::default(),
            reference: ReferenceDialog::default(),
            dialogs: FileDialogs::new(),
            file_purpose: None,
            replacement: None,
            name_field,
            name_focused: false,
            cursor: (1, 1),
            project_info: true,
            title_open: false,
            reveal_attributes: false,
            gpu_error: None,
            backend: "STARTING JOY ENGINE".into(),
            hosting,
            raw_form: false,
            host_files: None,
            disk_root: None,
            themed: false,
            targets: std::collections::HashMap::new(),
            panes: joy_engine::development::PanelRects::default(),
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // State for hosts, tests, and captures
    // -----------------------------------------------------------------------------------------------------------------

    /// The session.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// The session, mutably (tests drive edits directly).
    pub fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }

    /// The preview.
    pub fn viewport(&self) -> &Viewport {
        &self.viewport
    }

    /// The preview, mutably.
    pub fn viewport_mut(&mut self) -> &mut Viewport {
        &mut self.viewport
    }

    /// The Output inspector state.
    pub fn inspection(&self) -> &OutputInspection {
        &self.inspection
    }

    /// The pane layout.
    pub fn layout(&self) -> &PanelLayout {
        &self.layout
    }

    /// The pane layout, mutably.
    pub fn layout_mut(&mut self) -> &mut PanelLayout {
        &mut self.layout
    }

    /// The open inspector tab.
    pub fn tab(&self) -> InspectorTab {
        self.tab
    }

    /// Open an inspector tab.
    pub fn set_tab(&mut self, tab: InspectorTab) {
        self.tab = tab;
    }

    /// The Save/Export dialog.
    pub fn artifact_dialog(&mut self) -> &mut ArtifactDialog {
        &mut self.artifact
    }

    /// The tool's file dialogs, so hosts and tests can deliver files.
    pub fn file_dialogs(&mut self) -> &mut FileDialogs {
        &mut self.dialogs
    }

    /// The Script Reference dialog.
    pub fn reference_dialog(&self) -> &ReferenceDialog {
        &self.reference
    }

    /// The preview error, when the preview could not draw or rejected a build.
    pub fn gpu_error(&self) -> Option<&str> {
        self.gpu_error.as_deref()
    }

    /// Where a named control was drawn last frame (`run`, `save`, `export`, `view`, `shaded`, `points`, …).
    pub fn control_rect(&self, name: &str) -> Option<egui::Rect> {
        self.targets.get(name).copied()
    }

    /// Where a parameter's main control (or the Parameters tab's `search`, `undo`, and `redo`) was drawn last frame.
    pub fn parameter_rect(&self, name: &str) -> Option<egui::Rect> {
        self.parameters.control_rect(name)
    }

    /// Open a file the author chose, as the Open dialog does (hosts that read files themselves, and tests).
    pub fn open_file(&mut self, name: &str, text: &str) {
        self.handle_opened(FilePurpose::OpenDocument, name, text.as_bytes(), None);
    }

    /// The panes drawn last frame.
    pub fn panes(&self) -> joy_engine::development::PanelRects {
        self.panes
    }

    /// Remember where a control was drawn and pass its response through.
    fn track(&mut self, name: &'static str, response: egui::Response) -> egui::Response {
        self.targets.insert(name, response.rect);
        response
    }

    /// The Examples library.
    pub fn assets(&self) -> &[FormLibraryAsset] {
        &self.assets
    }

    /// Whether a replacement is waiting for confirmation.
    pub fn confirmation_pending(&self) -> bool {
        self.replacement.is_some()
    }

    /// Answer the pending replacement confirmation.
    pub fn confirm_replacement(&mut self, replace: bool) {
        if let Some(replacement) = self.replacement.take()
            && replace
        {
            self.load(replacement.source);
        }
    }

    /// Open an Examples asset by path (asking first when the study has edits).
    pub fn open_example(&mut self, path: &str) -> Result<(), String> {
        let asset =
            self.assets.iter().find(|asset| asset.path == path).ok_or_else(|| format!("{path} is not an example."))?;
        let document =
            open_form_library_asset(asset, &self.assets).map_err(|error| format!("Could not open {path}: {error}"))?;
        let source = DocumentSource {
            document,
            files: Some(library_reader(&self.assets)),
            path: asset.path.clone(),
            disk_root: None,
        };
        self.replace_with(source, "Replace this study with an example? Save first if you want to keep your edits.");
        Ok(())
    }

    /// Replace the study, or ask first when it has edits.
    fn replace_with(&mut self, source: DocumentSource, prompt: &'static str) {
        if self.session.has_edits() {
            self.replacement = Some(PendingReplacement {
                prompt,
                source,
            });
        } else {
            self.load(source);
        }
    }

    /// Load a document into every view.
    fn load(&mut self, source: DocumentSource) {
        let DocumentSource {
            document,
            files,
            path,
            disk_root,
        } = source;
        self.disk_root = disk_root;
        self.session.set_files(files, &path);
        self.parameters.clear(&mut self.drafts);
        self.materials.reset();
        self.variants.reset();
        self.inspection.reset(&mut self.viewport);
        self.editor.set_diagnostics(Vec::new());
        self.editor.reset_history();
        self.session.load(document);
        self.name_field = self.session.name.clone();
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Frame work
    // -----------------------------------------------------------------------------------------------------------------

    /// Advance the compile lifecycle and file dialogs; called every frame before drawing.
    pub fn update(&mut self) {
        while let Some(event) = self.session.update() {
            self.handle_session_event(event);
        }
        if let Some(event) = self.dialogs.poll() {
            self.handle_file_event(event);
        }
    }

    /// Wait for the outstanding build and show it, as frames would (tests and captures).
    pub fn finish_build(&mut self) {
        if let Some(event) = self.session.finish_build() {
            self.handle_session_event(event);
        }
    }

    fn handle_session_event(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::BuildReady {
                data,
                milliseconds,
                frame,
            } => {
                let accepted = (*data).clone();
                match self.inspection.update(*data, frame, &mut self.viewport) {
                    Ok(()) => {
                        self.session.accept_build(accepted, milliseconds);
                        self.editor.set_diagnostics(Vec::new());
                        self.gpu_error = None;
                        if !self.name_field_focused() {
                            self.name_field = self.session.name.clone();
                        }
                    }
                    Err(error) => {
                        self.session.reject_preview("");
                        self.gpu_error = Some(error);
                    }
                }
            }
            SessionEvent::BuildRejected(diagnostic) => {
                self.editor.set_diagnostics(vec![ScriptDiagnostic::from(&diagnostic)]);
            }
        }
    }

    fn name_field_focused(&self) -> bool {
        self.name_focused
    }

    fn start_file(&mut self, purpose: FilePurpose, request: Result<(), joy_engine::development::FileDialogError>) {
        match request {
            Ok(()) => self.file_purpose = Some(purpose),
            Err(error) => self.session.set_status(error.to_string(), StatusState::Error),
        }
    }

    fn handle_file_event(&mut self, event: FileDialogEvent) {
        let Some(purpose) = self.file_purpose.take() else {
            return;
        };
        match (purpose, event) {
            (
                FilePurpose::Artifact {
                    kind,
                    format,
                    contents,
                },
                FileDialogEvent::Saved {
                    ..
                },
            ) => {
                if kind == ArtifactKind::Save {
                    let format = if format == "form" { SaveFormat::Form } else { SaveFormat::Instance };
                    self.session.save_completed(format, &contents, !cfg!(target_arch = "wasm32"));
                }
                self.artifact.completed();
            }
            (
                FilePurpose::Artifact {
                    ..
                },
                FileDialogEvent::Cancelled,
            ) => self.artifact.cancelled(),
            (
                FilePurpose::Artifact {
                    ..
                },
                FileDialogEvent::Failed(message),
            ) => self.artifact.failed(message),
            (purpose, FileDialogEvent::Opened(file)) => {
                self.handle_opened(purpose, &file.name, file.contents.as_bytes(), file.path.as_deref())
            }
            (FilePurpose::ImportMesh(name), FileDialogEvent::Failed(message)) => {
                self.parameters.messages.insert(name, message);
            }
            (
                FilePurpose::ExportMaterialX
                | FilePurpose::ImportMaterialX
                | FilePurpose::ChooseTexture {
                    ..
                },
                FileDialogEvent::Failed(message),
            ) => {
                self.materials.message = message;
            }
            (_, FileDialogEvent::Failed(message)) => self.session.set_status(message, StatusState::Error),
            (
                FilePurpose::ExportMaterialX,
                FileDialogEvent::Saved {
                    ..
                },
            ) => {
                self.materials.message = "MaterialX saved.".into();
            }
            _ => {}
        }
    }

    fn handle_opened(&mut self, purpose: FilePurpose, name: &str, bytes: &[u8], location: Option<&std::path::Path>) {
        match purpose {
            FilePurpose::OpenDocument => {
                let text = String::from_utf8_lossy(bytes);
                // A file opened from disk reads its script and modules from beside it.
                let (files, path, disk_root) = match location.and_then(disk_reader) {
                    Some((files, path, root)) => (Some(files), path, Some(root)),
                    None => (None, String::new(), None),
                };
                match Session::read_project_file(name, &text, &path, files.as_ref()) {
                    Ok(document) => {
                        self.raw_form = false;
                        let source = DocumentSource {
                            document,
                            files,
                            path,
                            disk_root,
                        };
                        self.replace_with(
                            source,
                            "Replace this study with the opened file? Save first to keep your edits.",
                        );
                    }
                    Err(error) => self.session.set_status(error.to_string(), StatusState::Error),
                }
            }
            FilePurpose::ImportMesh(parameter) => match import_glb_mesh(bytes, name) {
                Ok(mesh) => {
                    self.session.set_override(&parameter, Some(mesh));
                    self.parameters.messages.insert(parameter, "Geometry loaded.".into());
                }
                Err(error) => {
                    self.parameters.messages.insert(parameter, error);
                }
            },
            FilePurpose::ChooseTexture {
                material,
                field,
            } => {
                let result = image_data_url(bytes)
                    .and_then(|source| with_texture(self.session.materials(), &material, field, Some(source)));
                match result {
                    Ok(materials) => {
                        self.materials.message.clear();
                        self.session.set_materials(materials);
                    }
                    Err(error) => self.materials.message = error,
                }
            }
            FilePurpose::ImportMaterialX => {
                let result = String::from_utf8(bytes.to_vec())
                    .map_err(|_| "MaterialX source must be text.".to_owned())
                    .and_then(|text| import_materialx(&text).map_err(|error| error.0))
                    .and_then(|imported| {
                        let count = imported.len();
                        merge_materialx(self.session.materials(), imported).map(|merged| (merged, count))
                    });
                match result {
                    Ok((merged, count)) => {
                        self.session.set_materials(merged);
                        self.materials.message = format!(
                            "Imported {count} materials. Matching names were updated; existing textures were preserved."
                        );
                    }
                    Err(error) => self.materials.message = error,
                }
            }
            FilePurpose::ExportMaterialX
            | FilePurpose::Artifact {
                ..
            } => {}
        }
    }

    /// The text a standalone save writes. A referencing instance whose script was edited writes that script back
    /// beside it on disk first; where it cannot (browsers, the read-only library), the instance is saved packed so the
    /// edits are kept.
    fn instance_contents(&mut self, format: SaveFormat) -> Result<String, String> {
        if format == SaveFormat::Form {
            return self.session.save_contents(format);
        }
        let Some((path, source)) = self.session.script_dependency() else {
            return self.session.save_contents(format);
        };
        match &self.disk_root {
            Some(root) => {
                std::fs::write(root.join(&path), &source)
                    .map_err(|error| format!("Cannot write the instance's script {path}: {error}"))?;
                self.session.mark_script_saved();
                self.session.save_contents(format)
            }
            None => {
                self.session.set_status(
                    format!("{path} can't be written here, so this instance was saved with its script packed."),
                    StatusState::Ready,
                );
                serialize_document(&self.session.document().packed()).map_err(|error| error.to_string())
            }
        }
    }

    /// Produce and write a confirmed Save or Export.
    fn write_artifact(&mut self, request: ArtifactRequest) {
        let filter = FileFilter::new(request.format.label, &[request.format.id]);
        let produced = match request.kind {
            ArtifactKind::Save => {
                let format = if request.format.id == "form" { SaveFormat::Form } else { SaveFormat::Instance };
                let ready = if format == SaveFormat::Instance { self.drafts.require_valid() } else { Ok(()) };
                ready.and_then(|()| self.instance_contents(format)).map(|text| (text.clone().into_bytes(), Some(text)))
            }
            ArtifactKind::Export => {
                let format = match request.format.id {
                    "glb" => ExportFormat::Glb,
                    "obj" => ExportFormat::Obj,
                    "ply" => ExportFormat::Ply,
                    _ => ExportFormat::Json,
                };
                self.session.export(format).map(|bytes| (bytes, None))
            }
        };
        match produced {
            Ok((bytes, text)) => {
                let title = request.kind.title();
                let save = match &text {
                    Some(text) => SaveRequest::text(title, &request.name, filter, text.clone()),
                    None if request.format.id == "glb" => SaveRequest::binary(title, &request.name, filter, bytes),
                    None => {
                        SaveRequest::text(title, &request.name, filter, String::from_utf8_lossy(&bytes).into_owned())
                    }
                };
                let started = self.dialogs.save(save);
                match started {
                    Ok(()) => {
                        self.file_purpose = Some(FilePurpose::Artifact {
                            kind: request.kind,
                            format: request.format.id,
                            contents: text.unwrap_or_default(),
                        })
                    }
                    Err(error) => self.artifact.failed(error.to_string()),
                }
            }
            Err(error) => self.artifact.failed(error),
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------------------------------------------------------

    /// Draw the whole tool inside `area` (logical points).
    pub fn draw(&mut self, context: &egui::Context, area: egui::Rect) {
        if !self.themed {
            joy_engine::runtime::theme::apply_editor(context);
            self.themed = true;
        }
        let standalone = self.hosting == ToolHosting::Standalone;
        if standalone
            && self.replacement.is_none()
            && context.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::S))
        {
            self.artifact.open(ArtifactKind::Save, &file_stem(&self.session.name));
        }
        if standalone
            && self.replacement.is_none()
            && context.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
        {
            self.open_document();
        }
        egui::Area::new(egui::Id::new("form-editor-root"))
            .fixed_pos(area.min)
            .order(egui::Order::Background)
            .fade_in(false)
            .show(context, |ui| {
                ui.set_clip_rect(area);
                ui.scope_builder(UiBuilder::new().max_rect(area), |ui| {
                    let maximized = self.layout.maximized();
                    if !maximized {
                        egui::Panel::top("form-editor-header")
                            .exact_size(if standalone { 58.0 } else { 44.0 })
                            .frame(egui::Frame::new().fill(colors::PANEL).inner_margin(egui::Margin::symmetric(16, 8)))
                            .show(ui, |ui| self.header(ui, standalone));
                        egui::Panel::top("form-editor-project")
                            .exact_size(46.0)
                            .frame(
                                egui::Frame::new().fill(colors::SURFACE).inner_margin(egui::Margin::symmetric(16, 6)),
                            )
                            .show(ui, |ui| self.project_bar(ui, standalone));
                    }
                    let workbench = ui.available_rect_before_wrap();
                    let panes = self.layout.panes(workbench);
                    self.panes = panes;
                    if let Some(rect) = panes.left {
                        ui.scope_builder(UiBuilder::new().max_rect(rect).id_salt("script-pane"), |ui| {
                            self.script_pane(ui)
                        });
                    }
                    match panes.center {
                        Some(rect) => {
                            ui.scope_builder(UiBuilder::new().max_rect(rect).id_salt("preview-pane"), |ui| {
                                self.preview_pane(ui)
                            });
                        }
                        None => self.viewport.set_pane(None, {
                            let size = context.content_rect().size();
                            Vec2::new(size.x, size.y)
                        }),
                    }
                    if let Some(rect) = panes.right {
                        ui.scope_builder(UiBuilder::new().max_rect(rect).id_salt("inspector-pane"), |ui| {
                            self.inspector_pane(ui)
                        });
                    }
                    for rect in [panes.left, panes.center, panes.right].into_iter().flatten() {
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            egui::Stroke::new(1.0, colors::BORDER),
                            egui::StrokeKind::Inside,
                        );
                    }
                    self.layout.dividers(ui, &panes);
                });
            });
        self.dialogs_ui(context);
    }

    fn header(&mut self, ui: &mut Ui, standalone: bool) {
        ui.horizontal_centered(|ui| {
            if standalone {
                components::app_title(ui, PRODUCT_NAME, VERSION);
                ui.add_space(layout::SPACE_LG);
            }
            let view = view_menu(ui, &mut self.layout);
            self.track("view", view);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.track("help", ui.add(components::button("?")).on_hover_text("Language reference")).clicked() {
                    self.reference.open();
                }
                let export = ui.add_enabled(self.session.data().is_some(), components::button("EXPORT"));
                let export = self.track("export", export);
                if export.clicked() {
                    self.artifact.open(ArtifactKind::Export, &file_stem(&self.session.name));
                }
                if standalone {
                    if self
                        .track(
                            "save",
                            ui.add(components::button("SAVE"))
                                .on_hover_text("Save a FORM script or FORM instance (Ctrl/Command+S)"),
                        )
                        .clicked()
                    {
                        self.artifact.open(ArtifactKind::Save, &file_stem(&self.session.name));
                    }
                    if self
                        .track(
                            "open",
                            ui.add(components::button("OPEN"))
                                .on_hover_text("Open a .form script or .forminst instance (Ctrl/Command+O)"),
                        )
                        .clicked()
                    {
                        self.open_document();
                    }
                }
            });
        });
        let rect = ui.max_rect();
        let line = egui::Rect::from_min_max(
            egui::pos2(rect.left() - 16.0, rect.bottom() + 6.0),
            egui::pos2(rect.right() + 16.0, rect.bottom() + 8.0),
        );
        ui.painter().rect_filled(line, 0.0, colors::ACCENT);
    }

    /// Ask for a `.form` script or `.forminst` instance (or an older `.formlab` document) to open.
    fn open_document(&mut self) {
        let request = OpenRequest::text(
            "Open",
            FileFilter::new("FORM documents", &["forminst", "form", "formlab", "txt", "json"]),
        )
        .maximum_bytes(joy_engine::form::DOCUMENT_FILE_BYTES);
        let started = self.dialogs.open(request);
        self.start_file(FilePurpose::OpenDocument, started);
    }

    fn project_bar(&mut self, ui: &mut Ui, standalone: bool) {
        ui.horizontal_centered(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter().rect_filled(dot, 0.0, colors::ACCENT);
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.name_field)
                    .char_limit(120)
                    .desired_width(210.0)
                    .font(egui::FontId::new(layout::TYPE_MD, joy_engine::runtime::theme::text_styles::strong_family()))
                    .margin(egui::vec2(5.0, 6.0)),
            );
            self.name_focused = field.has_focus();
            if field.lost_focus() {
                // An invalid script or name keeps the current name; the status line says why.
                let next = self.name_field.clone();
                let _ = self.session.rename(&next);
                self.name_field = self.session.name.clone();
            } else if !field.has_focus() && self.name_field != self.session.name {
                self.name_field = self.session.name.clone();
            }
            if standalone && let Some((text, title)) = self.session.save_state() {
                let label = ui.label(RichText::new(text).color(colors::MUTED).size(layout::TYPE_SM));
                if !title.is_empty() {
                    label.on_hover_text(title);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut chosen = None;
                let examples = egui::ComboBox::from_id_salt("form-editor-examples")
                    .selected_text("Choose an example")
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        let mut folder = "";
                        for asset in &self.assets {
                            if asset.folder != folder {
                                folder = &asset.folder;
                                ui.label(RichText::new(folder).color(colors::MUTED).size(layout::TYPE_XS));
                            }
                            if ui.selectable_label(false, &asset.filename).on_hover_text(&asset.path).clicked() {
                                chosen = Some(asset.path.clone());
                            }
                        }
                    });
                self.targets.insert("examples", examples.response.rect);
                ui.label(RichText::new("EXAMPLES").color(colors::TEXT).size(layout::TYPE_SM));
                if let Some(path) = chosen
                    && let Err(error) = self.open_example(&path)
                {
                    self.session.set_status(error, StatusState::Error);
                }
            });
        });
    }

    fn script_pane(&mut self, ui: &mut Ui) {
        egui::Panel::top("script-heading")
            .exact_size(48.0)
            .frame(components::panel_frame().fill(colors::SURFACE).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    components::panel_heading(ui, "01", "SCRIPT");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self
                            .track(
                                "run",
                                ui.add(components::button("Run ▶")).on_hover_text("Build now (Ctrl/Command+Enter)"),
                            )
                            .clicked()
                        {
                            self.session.compile();
                        }
                        let mut live = self.session.live();
                        if components::checkbox(ui, &mut live, "Live").changed() {
                            self.session.set_live(live);
                        }
                    });
                });
            });
        egui::Panel::bottom("script-diagnostic")
            .min_size(38.0)
            .frame(egui::Frame::new().fill(colors::BUILD_STATUS).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let status = self.session.status().clone();
                    let color = match status.state {
                        StatusState::Ready => colors::SUCCESS,
                        StatusState::Busy => colors::ACCENT_HOVER,
                        StatusState::Error => colors::DANGER,
                    };
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.5, color);
                    ui.label(RichText::new(status.message).color(color).size(layout::TYPE_SM));
                    if let Some((line, _column)) = self.session.error_location()
                        && self.track("go-to-error", ui.small_button("Go to error ↗")).clicked()
                    {
                        self.editor.go_to_line(line);
                    }
                });
            });
        egui::Panel::bottom("script-bottom")
            .frame(egui::Frame::new().fill(colors::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Ln {}, Col {}", self.cursor.0, self.cursor.1))
                            .color(colors::MUTED)
                            .size(layout::TYPE_XS)
                            .monospace(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if self
                            .track(
                                "script-reference",
                                ui.add(components::button("Script Reference"))
                                    .on_hover_text("FORM SCRIPT language reference"),
                            )
                            .clicked()
                        {
                            self.reference.open();
                        }
                        if self
                            .track(
                                "recipes",
                                ui.add(components::button("Recipes ↗")).on_hover_text("Append a recipe to the script"),
                            )
                            .clicked()
                        {
                            self.reference.open_recipes();
                        }
                    });
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(colors::SCRIPT)).show(ui, |ui| {
            let response = self.editor.show(ui, &mut self.session.source);
            self.cursor = (response.line, response.column);
            if response.changed {
                self.session.source_changed();
            }
            if response.run_requested {
                self.session.compile();
            }
        });
    }

    fn preview_pane(&mut self, ui: &mut Ui) {
        let compact = ui.available_width() < COMPACT_PREVIEW_WIDTH;
        egui::Panel::top("preview-heading")
            .min_size(if compact { 78.0 } else { 48.0 })
            .frame(components::panel_frame().fill(colors::SURFACE).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ui, |ui| {
                if compact {
                    self.preview_heading(ui);
                    ui.horizontal_wrapped(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center).with_main_wrap(true), |ui| {
                            self.preview_actions(ui);
                        });
                    });
                } else {
                    ui.horizontal_centered(|ui| {
                        ui.vertical(|ui| self.preview_heading(ui));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| self.preview_actions(ui));
                    });
                }
            });
        egui::Panel::top("preview-toolbar").frame(components::toolbar_frame()).show(ui, |ui| {
            if let Some(error) = result_toolbar(ui, &mut self.inspection, &mut self.viewport) {
                self.gpu_error = Some(error);
            }
        });
        egui::Panel::bottom("preview-generation")
            .frame(components::toolbar_frame())
            .show(ui, |ui| self.generation_summary(ui));
        egui::Panel::bottom("preview-attributes")
            .frame(egui::Frame::new().fill(colors::PANEL).inner_margin(egui::Margin::symmetric(10, 4)))
            .show(ui, |ui| {
                let reveal = std::mem::take(&mut self.reveal_attributes);
                attribute_panel(ui, &mut self.inspection, &mut self.viewport, reveal);
            });
        egui::Panel::bottom("preview-bottom")
            .frame(egui::Frame::new().fill(colors::PANEL).inner_margin(egui::Margin::symmetric(10, 6)))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    components::checkbox(ui, &mut self.project_info, "Project info");
                    components::checkbox(ui, &mut self.viewport.show_grid, "Line grid")
                        .on_hover_text("Editor-only line grid; minor spacing follows 1/2/5 world units, major lines mark five divisions");
                    let point_size_label = ui.label("Point Size");
                    let previous_style = ui.style().clone();
                    ui.spacing_mut().slider_width = 60.0;
                    ui.spacing_mut().slider_rail_height = 7.0;
                    ui.spacing_mut().interact_size.y = 16.0;
                    ui.visuals_mut().selection.bg_fill = colors::ACCENT;
                    ui.visuals_mut().widgets.inactive.bg_fill = colors::BUTTON;
                    ui.visuals_mut().widgets.inactive.fg_stroke = egui::Stroke::new(2.0, colors::SLIDER_EDGE);
                    ui.add(
                        egui::Slider::new(&mut self.viewport.point_size, 1.0..=6.0)
                            .show_value(false)
                            .step_by(0.5)
                            .handle_shape(egui::style::HandleShape::Circle)
                            .trailing_fill(true),
                    )
                    .labelled_by(point_size_label.id)
                    .on_hover_text("Point size");
                    ui.set_style(previous_style);
                    ui.label(RichText::new(&self.backend).color(colors::SUCCESS).size(layout::TYPE_XS).monospace());
                });
            });
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| {
            let rect = ui.max_rect();
            let viewport_size = ui.ctx().content_rect().size();
            self.viewport.set_pane(
                Some(PaneRect {
                    min: Vec2::new(rect.min.x, rect.min.y),
                    size: Vec2::new(rect.width(), rect.height()),
                }),
                Vec2::new(viewport_size.x, viewport_size.y),
            );
            let actions = viewport_input(ui, rect, egui::Id::new("form-editor-viewport"));
            match actions.drag {
                Some((ViewportGesture::Orbit, delta)) => self.viewport.orbit(Vec2::new(delta.x, delta.y)),
                Some((ViewportGesture::Pan, delta)) => self.viewport.pan(Vec2::new(delta.x, delta.y)),
                None => {}
            }
            if actions.zoom != 0.0 {
                self.viewport.zoom(actions.zoom);
            }
            for shortcut in actions.shortcuts {
                self.viewport.shortcut(match shortcut {
                    ViewportShortcut::Frame => "f",
                    ViewportShortcut::Reset => "0",
                    ViewportShortcut::Left => "arrowleft",
                    ViewportShortcut::Right => "arrowright",
                    ViewportShortcut::Up => "arrowup",
                    ViewportShortcut::Down => "arrowdown",
                });
            }
            if let Some(point) = actions.click {
                let picked = self.viewport.pick(Vec2::new(point.x, point.y));
                if picked.is_some() {
                    self.reveal_attributes = true;
                }
                self.inspection.pick(picked, &mut self.viewport);
            }
            self.viewport_overlays(ui, rect);
        });
    }

    fn viewport_overlays(&mut self, ui: &mut Ui, rect: egui::Rect) {
        if self.project_info {
            let project = self.session.project();
            let title = project
                .and_then(|project| project.get("title"))
                .and_then(|title| title.as_str())
                .map_or_else(|| self.session.name.to_uppercase(), str::to_owned);
            let info = project
                .and_then(|project| project.get("info"))
                .and_then(|info| info.as_str())
                .unwrap_or("PROCEDURAL STUDY")
                .to_owned();
            let position = rect.min + egui::vec2(14.0, 14.0);
            egui::Area::new(ui.id().with("project-title"))
                .fixed_pos(position)
                .order(egui::Order::Middle)
                .fade_in(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .fill(Color32::from_rgba_unmultiplied(16, 23, 34, 200))
                        .stroke(egui::Stroke::new(1.0, colors::ACCENT))
                        .inner_margin(egui::Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            let arrow = if self.title_open { "▼" } else { "▶" };
                            let response = ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{arrow}  {}", title.replace('\n', " ")))
                                        .color(colors::ACCENT)
                                        .strong(),
                                )
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .sense(egui::Sense::click()),
                            );
                            if response.clicked() {
                                self.title_open = !self.title_open;
                            }
                            if self.title_open {
                                ui.label(RichText::new(info).color(colors::MUTED).size(layout::TYPE_XS));
                            }
                        });
                });
        }
        let painter = ui.painter_at(rect);
        let mono = egui::FontId::monospace(layout::TYPE_XS);
        painter.text(
            rect.right_top() + egui::vec2(-30.0, 22.0),
            egui::Align2::CENTER_CENTER,
            "Y ↑",
            mono.clone(),
            colors::SUCCESS,
        );
        painter.text(
            rect.right_top() + egui::vec2(-46.0, 40.0),
            egui::Align2::CENTER_CENTER,
            "Z ↙",
            mono.clone(),
            Color32::from_rgb(0x87, 0xb5, 0xe3),
        );
        painter.text(
            rect.right_top() + egui::vec2(-16.0, 40.0),
            egui::Align2::CENTER_CENTER,
            "X →",
            mono.clone(),
            colors::ACCENT,
        );
        painter.text(
            rect.center_bottom() + egui::vec2(0.0, -22.0),
            egui::Align2::CENTER_CENTER,
            if rect.width() < COMPACT_PREVIEW_WIDTH {
                "DRAG TO ORBIT · SCROLL TO ZOOM"
            } else {
                "DRAG TO ORBIT  ·  SHIFT + DRAG TO PAN  ·  SCROLL TO ZOOM  ·  CLICK TO INSPECT"
            },
            mono,
            colors::MUTED,
        );
        if let Some(error) = &self.gpu_error {
            let error_rect = egui::Rect::from_min_size(
                rect.min + egui::vec2(14.0, rect.height() - 90.0),
                egui::vec2(rect.width() - 28.0, 40.0),
            );
            painter.rect_filled(error_rect, 0.0, Color32::from_rgba_unmultiplied(60, 20, 20, 220));
            painter.text(
                error_rect.left_center() + egui::vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                error,
                egui::FontId::proportional(layout::TYPE_SM),
                colors::DANGER,
            );
        }
    }

    fn preview_heading(&self, ui: &mut Ui) {
        components::panel_heading(ui, "02", "PREVIEW");
        let color = match self.session.preview_state() {
            PreviewState::Accepted => colors::SUCCESS,
            PreviewState::Pending => colors::MUTED,
            PreviewState::Failed => colors::DANGER,
        };
        ui.label(RichText::new(self.session.preview_text()).color(color).size(layout::TYPE_XS));
    }

    fn preview_actions(&mut self, ui: &mut Ui) {
        let maximize = if self.layout.maximized() { "Restore" } else { "Maximize" };
        if self
            .track(
                "maximize",
                ui.add(components::button(maximize).fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE)),
            )
            .clicked()
        {
            self.layout.set_maximized(!self.layout.maximized());
        }
        if self
            .track(
                "reset-camera",
                ui.add(components::button("Reset").fill(egui::Color32::TRANSPARENT).stroke(egui::Stroke::NONE))
                    .on_hover_text("Reset camera (0)"),
            )
            .clicked()
        {
            self.viewport.reset_camera();
        }
        if self.track("frame", ui.add(components::button("Frame  F")).on_hover_text("Frame all geometry (F)")).clicked()
        {
            self.viewport.frame();
        }
        let points = components::toggle_button(ui, self.viewport.mode == ViewMode::Points, "Points");
        if self.track("points", points).clicked() {
            self.viewport.set_mode(ViewMode::Points);
        }
        let shaded = components::toggle_button(ui, self.viewport.mode == ViewMode::Shaded, "Shaded");
        if self.track("shaded", shaded).clicked() {
            self.viewport.set_mode(ViewMode::Shaded);
        }
    }

    fn generation_summary(&mut self, ui: &mut Ui) {
        let statistics = self.session.statistics();
        let has_data = self.session.data().is_some();
        let value = |text: String| if has_data { text } else { "—".to_owned() };
        ui.horizontal_wrapped(|ui| {
            let mono = |text: String| RichText::new(text).color(colors::MUTED).size(layout::TYPE_XS).monospace();
            ui.label(mono(format!("Generation Time: {}", value(format!("{:.1} ms", statistics.milliseconds)))));
            ui.label(mono(format!(
                "Data Size: {}",
                value(format!("{:.1} KiB", statistics.data_bytes as f64 / 1024.0))
            )))
            .on_hover_text("Numeric geometry payload, excluding array overhead, preview buffers, and exports");
            ui.label(mono(format!("Vertices: {}", value(group_thousands(statistics.vertices)))));
            ui.label(mono(format!("Polygons: {}", value(group_thousands(statistics.triangles)))));
            let points = ui.label(mono(format!("Points: {}", value(group_thousands(statistics.points)))));
            points.on_hover_text(if self.viewport.mode == ViewMode::Points {
                "Point preview samples at most 40,000 markers. Exports contain every point."
            } else {
                "Total points in the accepted build."
            });
        });
    }

    fn inspector_pane(&mut self, ui: &mut Ui) {
        egui::Panel::top("inspector-heading")
            .exact_size(48.0)
            .frame(components::panel_frame().fill(colors::SURFACE).inner_margin(egui::Margin::symmetric(12, 8)))
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    components::panel_heading(ui, "03", "INSPECTOR");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let reset = ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Reset params")
                                        .color(colors::ACCENT)
                                        .text_style(egui::TextStyle::Button)
                                        .size(layout::TYPE_XS),
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(1.0, colors::ACCENT))
                                .min_size(egui::vec2(0.0, 30.0)),
                            )
                            .on_hover_text(
                                "Restore all parameter defaults; materials and preview lighting are unchanged",
                            );
                        if self.track("reset-params", reset).clicked() {
                            self.drafts.clear();
                            self.session.reset_parameters();
                        }
                    });
                });
            });
        egui::Panel::top("inspector-tabs")
            .frame(egui::Frame::new().fill(colors::PANEL).inner_margin(egui::Margin::symmetric(6, 6)))
            .show(ui, |ui| inspector_tabs(ui, &mut self.tab));
        egui::CentralPanel::default()
            .frame(components::panel_frame().fill(colors::SURFACE).inner_margin(egui::Margin::same(12)))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().id_salt(("inspector", self.tab.label())).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match self.tab {
                        InspectorTab::Parameters => self.parameters_tab(ui),
                        InspectorTab::Variants => self.variants_tab(ui),
                        InspectorTab::Materials => self.materials_tab(ui),
                        InspectorTab::Lighting => {
                            let current = self.viewport.lighting();
                            if let Some(next) = self.lighting.show(ui, current, &mut self.drafts)
                                && let Err(error) = self.viewport.set_lighting(next)
                            {
                                self.lighting.message = error;
                            }
                        }
                    }
                });
            });
    }

    fn parameters_tab(&mut self, ui: &mut Ui) {
        let overridden = self.session.overrides().keys().cloned().collect();
        let signature = self.session.parameter_signature();
        let (can_undo, can_redo) = (self.session.can_undo_properties(), self.session.can_redo_properties());
        let guidance = self.override_guidance();
        let actions = self.parameters.show(
            ui,
            self.session.parameters_mut(),
            signature,
            &overridden,
            &mut self.drafts,
            can_undo,
            can_redo,
            guidance,
        );
        for action in actions {
            match action {
                ParameterAction::Set {
                    name,
                    value,
                    commit,
                } => {
                    self.session.set_override(&name, Some(value));
                    if commit {
                        self.session.commit_properties();
                    }
                }
                ParameterAction::SetMesh {
                    name,
                    value,
                } => {
                    self.parameters.messages.remove(&name);
                    self.session.set_override(&name, Some(value));
                }
                ParameterAction::Reset(name) => {
                    self.parameters.messages.remove(&name);
                    self.session.reset_parameter(&name);
                }
                ParameterAction::ImportMesh(name) => {
                    let request = OpenRequest::binary("Import GLB geometry", FileFilter::new("GLB", &["glb"]))
                        .maximum_bytes(MAXIMUM_GLB_BYTES);
                    let started = self.dialogs.open(request);
                    self.parameters.messages.insert(name.clone(), "Reading geometry…".into());
                    self.start_file(FilePurpose::ImportMesh(name), started);
                }
                ParameterAction::Undo => {
                    self.drafts.clear();
                    self.session.undo_properties();
                }
                ParameterAction::Redo => {
                    self.drafts.clear();
                    self.session.redo_properties();
                }
            }
        }
    }

    /// What saving keeps of inspector overrides in this hosting mode (`#override-state`).
    fn override_guidance(&self) -> &'static str {
        match (self.hosting, self.raw_form) {
            (ToolHosting::Standalone, _) => {
                "Save a .form script, or choose .forminst to keep overrides, materials, and variants. Browser recovery \
                 stays in this browser."
            }
            (_, true) => {
                "Parameter overrides are temporary. Save writes script source only; exports use the accepted preview."
            }
            (_, false) => {
                "Parameter overrides and materials are saved in this FORM instance. Exports use the accepted preview."
            }
        }
    }

    fn variants_tab(&mut self, ui: &mut Ui) {
        let Some(action) = self.variants.show(ui, self.session.variants()) else {
            return;
        };
        let result = match action {
            VariantAction::Save(name) => self.session.save_variant(&name).map(|index| {
                self.variants.selected = index;
                "Variant saved. Changes to current controls will not alter it.".to_owned()
            }),
            VariantAction::Apply(index) => {
                let ready = self.drafts.require_valid();
                ready.and_then(|()| self.session.apply_variant(index)).map(|name| {
                    self.drafts.clear();
                    format!("Applied {name}. Parameters removed from the script are ignored.")
                })
            }
            VariantAction::Rename(index, name) => {
                self.session.rename_variant(index, &name).map(|()| "Variant renamed.".to_owned())
            }
            VariantAction::Delete(index) => self.session.delete_variant(index).map(|()| {
                self.variants.selected = 0;
                "Variant deleted. Current parameters are unchanged.".to_owned()
            }),
        };
        self.variants.status = match result {
            Ok(message) | Err(message) => message,
        };
    }

    fn materials_tab(&mut self, ui: &mut Ui) {
        let materials = self.session.materials().to_vec();
        for action in self.materials.show(ui, &materials, &mut self.drafts) {
            match action {
                MaterialAction::SetMaterials(materials) => self.session.set_materials(materials),
                MaterialAction::ChooseTexture {
                    material,
                    field,
                } => {
                    let request = OpenRequest::binary(
                        "Choose texture",
                        FileFilter::new("Images", &["png", "jpg", "jpeg", "webp"]),
                    )
                    .maximum_bytes(joy_engine::form::DOCUMENT_TEXTURE_BYTES);
                    let started = self.dialogs.open(request);
                    self.start_file(
                        FilePurpose::ChooseTexture {
                            material,
                            field,
                        },
                        started,
                    );
                }
                MaterialAction::ImportMaterialX => {
                    let request =
                        OpenRequest::binary("Import MaterialX", FileFilter::new("MaterialX", &["mtlx", "xml"]))
                            .maximum_bytes(MAXIMUM_MATERIALX_BYTES);
                    let started = self.dialogs.open(request);
                    self.start_file(FilePurpose::ImportMaterialX, started);
                }
                MaterialAction::ExportMaterialX => match export_materialx(self.session.materials()) {
                    Ok(text) => {
                        let request = SaveRequest::text(
                            "Export MaterialX",
                            "form-editor-materials.mtlx",
                            FileFilter::new("MaterialX", &["mtlx"]),
                            text,
                        );
                        let started = self.dialogs.save(request);
                        self.start_file(FilePurpose::ExportMaterialX, started);
                    }
                    Err(error) => self.materials.message = error.0,
                },
            }
        }
    }

    fn dialogs_ui(&mut self, context: &egui::Context) {
        if let Some(request) = self.artifact.show(context, self.session.export_note()) {
            self.write_artifact(request);
        }
        if let Some(index) = self.reference.show(context) {
            let appended = append_recipe(&self.session.source, &RECIPES[index]);
            self.session.source = appended.source;
            self.session.source_changed();
            self.editor.request_focus();
        }
        let mut answer = None;
        if let Some(replacement) = &self.replacement {
            egui::Modal::new(egui::Id::new("form-editor-confirm")).show(context, |ui| {
                ui.set_width(340.0);
                ui.label(replacement.prompt);
                ui.add_space(layout::SPACE_SM);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(components::primary_button("Replace")).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
        }
        if let Some(replace) = answer {
            self.confirm_replacement(replace);
        }
    }

    /// The area the tool draws in this frame.
    fn area(&self, context: &egui::Context) -> egui::Rect {
        match self.hosting {
            ToolHosting::Hosted {
                area,
            } if area.is_positive() => area,
            _ => context.content_rect(),
        }
    }
}

impl Default for FormEditor {
    fn default() -> Self {
        Self::new()
    }
}

/// The backend label beside the preview (`JOY ENGINE / WEBGPU`).
fn backend_label(backend: GpuBackend) -> String {
    format!("JOY ENGINE / {}", backend.label().to_uppercase())
}

impl App for FormEditor {
    fn start(&mut self, context: &mut StartContext<'_>) -> AppResult {
        self.backend = backend_label(context.capabilities.backend);
        if let Err(error) = self.viewport.prepare(context.renderer) {
            self.gpu_error = Some(error);
        }
        self.session.compile();
        Ok(())
    }

    fn simulation(&mut self, _frame: &mut Frame<'_>) -> AppResult {
        self.update();
        Ok(())
    }

    fn render(&mut self, _frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        if let Err(error) = self.viewport.render(target) {
            self.gpu_error = Some(error);
        }
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, _frame: &mut Frame<'_>) {
        let area = self.area(context);
        self.draw(context, area);
    }
}

impl AuthoringTool for FormEditor {
    fn tool_name(&self) -> &'static str {
        "FORM EDITOR"
    }

    fn document_extensions(&self) -> &'static [&'static str] {
        &["form", "forminst"]
    }

    fn open_document(&mut self, document: ToolDocument) -> Result<(), String> {
        let file_name = document.name.rsplit(['/', '\\']).next().unwrap_or(&document.name).to_owned();
        let raw = file_name.to_ascii_lowercase().ends_with(".form");
        let opened = if raw {
            if document.contents.chars().count() > crate::document::MAXIMUM_SOURCE_LENGTH {
                return Err("Script size limit is 200,000 characters.".into());
            }
            FormDocument::from_script(&file_name, &document.contents)
        } else {
            Session::read_project_file(&file_name, &document.contents, &document.path, self.host_files.as_ref())
                .map_err(|error| error.to_string())?
        };
        self.raw_form = raw;
        let files = if document.path.is_empty() { None } else { self.host_files.clone() };
        self.load(DocumentSource {
            document: opened,
            files,
            path: document.path,
            disk_root: None,
        });
        Ok(())
    }

    fn save_document(&mut self, name: &str) -> Result<String, String> {
        if name.to_ascii_lowercase().ends_with(".form") {
            return self.session.save_contents(SaveFormat::Form);
        }
        self.drafts.require_valid()?;
        self.session.save_contents(SaveFormat::Instance)
    }

    fn save_dependencies(&self) -> Vec<ToolDocument> {
        self.session
            .script_dependency()
            .map(|(path, contents)| ToolDocument {
                name: path.rsplit('/').next().unwrap_or(&path).to_owned(),
                contents,
                path,
            })
            .into_iter()
            .collect()
    }

    fn set_file_reader(&mut self, reader: Option<ToolFileReader>) {
        self.host_files = reader;
    }

    fn has_unsaved_changes(&self) -> bool {
        self.session.has_edits()
    }

    fn mark_saved(&mut self) {
        // The host writes an edited referenced script with the instance (see `save_dependencies`).
        self.session.mark_saved();
        self.session.mark_script_saved();
    }

    fn set_hosting(&mut self, hosting: ToolHosting) {
        if matches!(hosting, ToolHosting::Hosted { .. }) {
            self.session.set_recovery(None);
        }
        self.hosting = hosting;
    }

    fn activated(&mut self, context: &mut StartContext<'_>) {
        self.backend = backend_label(context.capabilities.backend);
        context.renderer.set_post_process(PostProcessSettings::DEFAULT);
        let _ = context.renderer.set_scene_effects(None);
        if let Err(error) = self.viewport.prepare(context.renderer) {
            self.gpu_error = Some(error);
        }
    }
}

/// A project reader for a file opened from disk: rooted at the file system root (or drive), so instances and scripts
/// can reach scripts and modules in any folder above them. Returns the reader, the file's path relative to that root,
/// and the root itself. Browsers have no locations, so they never get here.
fn disk_reader(location: &std::path::Path) -> Option<(ToolFileReader, String, std::path::PathBuf)> {
    let root = location.ancestors().last()?.to_path_buf();
    let relative = location.strip_prefix(&root).ok()?.to_string_lossy().replace('\\', "/");
    let base = root.clone();
    let reader: ToolFileReader = std::sync::Arc::new(move |path: &str| {
        std::fs::read_to_string(base.join(path)).map_err(|error| error.to_string())
    });
    Some((reader, relative, root))
}
