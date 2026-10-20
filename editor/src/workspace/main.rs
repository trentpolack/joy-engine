// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The workspace model: project selection, the asset list, document
//! tabs and their editors, saves against disk revisions, external-change polling and conflicts, asset lifecycle
//! (create, import, rename, delete), source navigation between documents, and the restored session.
//!
//! Operations that discard work return [`Outcome::Confirm`] instead of asking: the UI shows the question and repeats the
//! operation with `confirmed` set. Every message the author sees goes to [`Workspace::status`].

use std::collections::BTreeSet;

use joy_engine::development::ToolDocument;
use joy_engine::runtime::{
    StartContext,
    Storage,
};

use super::asset_types::{
    AssetEditor,
    asset_editor,
    asset_kind,
    create_asset_text,
    is_supported_asset_path,
    new_asset_path,
};
use super::constants::{
    POLL_SECONDS,
    REFRESH_SECONDS,
};
use super::documents::WorkspaceDocument;
use super::session::{
    LayoutPreferences,
    WorkspaceSession,
};
use super::tool_bridge::{
    ToolRegistry,
    ToolTab,
};
use crate::level::{
    AssetAccess,
    LevelEditor,
};
use crate::server::{
    AssetEntry,
    ProjectError,
    ProjectStore,
    ProjectSummary,
};

/// The result of an operation that may need the author's confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Done, or nothing to do.
    Done,
    /// Refused; the status line explains why.
    Blocked,
    /// Ask this question, then repeat the operation with `confirmed` set.
    Confirm(Confirmation),
}

/// A question the workspace needs answered before discarding work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirmation {
    /// Switch to this project and discard unsaved edits.
    SwitchProject(String),
    /// Close this tab and discard its unsaved edits.
    CloseTab(String),
    /// Delete this asset.
    DeleteAsset(String),
    /// Replace this tab's edits with the disk version.
    ReloadDisk(String),
    /// Quit with unsaved edits.
    Quit,
}

impl Confirmation {
    /// The question.
    pub fn message(&self) -> String {
        match self {
            Self::SwitchProject(_) => "Switch projects and discard unsaved edits?".to_owned(),
            Self::CloseTab(path) => format!("Discard unsaved edits to {path}?"),
            Self::DeleteAsset(path) => format!("Permanently delete {path}?"),
            Self::ReloadDisk(_) => "Discard your local edits and use the version on disk?".to_owned(),
            Self::Quit => "Quit Joy Editor and discard unsaved edits?".to_owned(),
        }
    }

    /// The label of the confirming button.
    pub fn action(&self) -> &'static str {
        match self {
            Self::SwitchProject(_) => "Switch Project",
            Self::CloseTab(_) => "Discard Edits",
            Self::DeleteAsset(_) => "Delete",
            Self::ReloadDisk(_) => "Use Disk Version",
            Self::Quit => "Quit",
        }
    }
}

/// A plain source editor: the repair view for invalid structured assets, or the editor for formats without a tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceView {
    /// The edit buffer.
    pub text: String,
    /// Why the document's editor could not open it, for repair views.
    pub message: Option<String>,
    /// The editor to retry after a save or an external fix.
    pub retry: AssetEditor,
}

/// What a tab shows.
pub enum TabContent {
    /// The level and object editor.
    Level(Box<LevelEditor>),
    /// A hosted authoring tool.
    Tool(Box<ToolTab>),
    /// A source or repair view.
    Source(SourceView),
}

/// One open document.
pub struct Tab {
    /// Text, baseline, revision, and conflict state.
    pub document: WorkspaceDocument,
    /// The editor.
    pub content: TabContent,
    // Level generation last captured into the document.
    captured_generation: u64,
}

impl Tab {
    /// Whether the tab has edits that are not on disk.
    pub fn dirty(&self) -> bool {
        self.document.dirty() || matches!(&self.content, TabContent::Tool(tool) if tool.has_unsaved_changes())
    }

    /// The document's file name.
    pub fn file_name(&self) -> &str {
        self.document.path.rsplit('/').next().unwrap_or(&self.document.path)
    }
}

/// Project access for one project, lent to editors.
pub struct ProjectAccess<'a> {
    store: &'a mut dyn ProjectStore,
    project: &'a str,
    files: &'a [AssetEntry],
    /// Paths created through this access, so the workspace can rescan.
    pub created: Vec<String>,
}

impl AssetAccess for ProjectAccess<'_> {
    fn catalog(&self) -> &[AssetEntry] {
        self.files
    }

    fn read(&mut self, path: &str) -> Result<String, String> {
        self.store.read(self.project, path).map(|disk| disk.text).map_err(|error| error.to_string())
    }

    fn create(&mut self, path: &str, text: &str) -> Result<(), String> {
        self.store.create(self.project, path, text).map_err(|error| error.to_string())?;
        self.created.push(path.to_owned());
        Ok(())
    }
}

/// Owns the project store, open documents, and session; the UI borrows it each frame.
pub struct Workspace {
    // Owned services.
    store: Box<dyn ProjectStore>,
    storage: Option<Storage>,
    tools: ToolRegistry,

    // Projects and their assets.
    projects: Vec<ProjectSummary>,
    project: String,
    files: Vec<AssetEntry>,

    // Documents, in tab order.
    tabs: Vec<Tab>,
    active: Option<String>,
    return_path: Option<String>,
    activation_pending: bool,

    // Content Browser.
    /// The search text.
    pub search: String,
    filters: BTreeSet<String>,
    selected_asset: Option<String>,

    // Session, timers, and messages.
    session: WorkspaceSession,
    status: String,
    next_poll: f64,
    next_refresh: f64,
    /// Whether the active tab temporarily hides the Content Browser (lab Maximize).
    pub maximized: bool,
}

impl Workspace {
    /// A workspace over `store`, restoring the session from `storage` (none in tests that do not persist).
    pub fn new(store: Box<dyn ProjectStore>, storage: Option<Storage>, tools: ToolRegistry) -> Self {
        let session = WorkspaceSession::load(storage.as_ref());
        Self {
            store,
            storage,
            tools,
            projects: Vec::new(),
            project: String::new(),
            files: Vec::new(),
            tabs: Vec::new(),
            active: None,
            return_path: None,
            activation_pending: true,
            search: String::new(),
            filters: BTreeSet::new(),
            selected_asset: None,
            session,
            status: "Connecting to local project…".to_owned(),
            next_poll: POLL_SECONDS,
            next_refresh: REFRESH_SECONDS,
            maximized: false,
        }
    }

    /// Discover projects, then open `requested` (or the restored or first project) with its restored tabs.
    pub fn start(&mut self, requested: Option<&str>) {
        match self.store.list_projects() {
            Ok(projects) => self.projects = projects,
            Err(error) => {
                self.status = error.to_string();
                return;
            }
        }
        let wanted = requested.map(str::to_owned).unwrap_or_else(|| self.session.project.clone());
        let id = self
            .projects
            .iter()
            .find(|project| project.id == wanted)
            .or_else(|| self.projects.first())
            .map(|project| project.id.clone());
        match id {
            Some(id) => {
                self.select_project(&id, true);
            }
            None => {
                self.status = "No projects found. Open a folder with a project.joyproject manifest.".to_owned();
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // State
    // -----------------------------------------------------------------------------------------------------------------

    /// Projects available to open.
    pub fn projects(&self) -> &[ProjectSummary] {
        &self.projects
    }

    /// The selected project id.
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The selected project's summary.
    pub fn project_summary(&self) -> Option<&ProjectSummary> {
        self.projects.iter().find(|project| project.id == self.project)
    }

    /// Every supported asset in the project.
    pub fn files(&self) -> &[AssetEntry] {
        &self.files
    }

    /// Assets matching the search text and type filters (FORM covers `.form` and `.formlab`).
    pub fn filtered_files(&self) -> Vec<&AssetEntry> {
        let query = self.search.to_lowercase();
        self.files
            .iter()
            .filter(|file| query.is_empty() || file.path.to_lowercase().contains(&query))
            .filter(|file| {
                let kind = if file.kind.starts_with("form") { "form" } else { file.kind.as_str() };
                self.filters.is_empty() || self.filters.contains(kind)
            })
            .collect()
    }

    /// The checked asset types.
    pub fn filters(&self) -> &BTreeSet<String> {
        &self.filters
    }

    /// Check or uncheck an asset type (`form`, `joyfx`, `joylevel`, `joyobject`).
    pub fn toggle_filter(&mut self, kind: &str) {
        if !self.filters.remove(kind) {
            self.filters.insert(kind.to_owned());
        }
    }

    /// Clear the type filters (All types).
    pub fn clear_filters(&mut self) {
        self.filters.clear();
    }

    /// The asset highlighted in the Content Browser.
    pub fn selected_asset(&self) -> Option<&str> {
        self.selected_asset.as_deref()
    }

    /// Highlight an asset without opening it.
    pub fn select_asset(&mut self, path: &str) {
        self.selected_asset = Some(path.to_owned());
    }

    /// Open tabs in order.
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// The active document path.
    pub fn active_path(&self) -> Option<&str> {
        self.active.as_deref()
    }

    /// The active tab.
    pub fn active_tab(&self) -> Option<&Tab> {
        let active = self.active.as_deref()?;
        self.tabs.iter().find(|tab| tab.document.path == active)
    }

    /// The active tab, mutably.
    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let active = self.active.clone()?;
        self.tabs.iter_mut().find(|tab| tab.document.path == active)
    }

    /// The tab for `path`.
    pub fn tab(&self, path: &str) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.document.path == path)
    }

    /// The tab for `path`, mutably.
    pub fn tab_mut(&mut self, path: &str) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|tab| tab.document.path == path)
    }

    /// The owner a source navigation can return to.
    pub fn return_path(&self) -> Option<&str> {
        self.return_path.as_deref()
    }

    /// The latest status message.
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Replace the status message.
    pub fn set_status(&mut self, message: impl Into<String>) {
        self.status = message.into();
    }

    /// Whether the store can write project files.
    pub fn writable(&self) -> bool {
        self.store.writable()
    }

    /// Registered tool names.
    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.names()
    }

    /// Restored panel preferences.
    pub fn layout(&self) -> LayoutPreferences {
        self.session.layout
    }

    /// Store panel preferences with the session.
    pub fn set_layout(&mut self, layout: LayoutPreferences) {
        if self.session.layout != layout {
            self.session.layout = layout;
            self.remember();
        }
    }

    /// Paths with edits that are not on disk.
    pub fn unsaved_paths(&self) -> Vec<String> {
        self.tabs.iter().filter(|tab| tab.dirty() || tab.document.saving).map(|tab| tab.document.path.clone()).collect()
    }

    /// The active level editor and project access for it.
    pub fn active_level(&mut self) -> Option<(&mut LevelEditor, ProjectAccess<'_>)> {
        let active = self.active.clone()?;
        let tab = self.tabs.iter_mut().find(|tab| tab.document.path == active)?;
        let TabContent::Level(level) = &mut tab.content else {
            return None;
        };
        let access = ProjectAccess {
            store: self.store.as_mut(),
            project: &self.project,
            files: &self.files,
            created: Vec::new(),
        };
        Some((level.as_mut(), access))
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Projects
    // -----------------------------------------------------------------------------------------------------------------

    /// Capture every tab, confirm discarding unsaved edits, then replace the project and restore its session tabs.
    pub fn select_project(&mut self, id: &str, confirmed: bool) -> Outcome {
        for index in 0..self.tabs.len() {
            if self.capture(index, false).is_err() {
                return Outcome::Blocked;
            }
        }
        if !confirmed && self.tabs.iter().any(|tab| tab.dirty() || tab.document.saving) {
            return Outcome::Confirm(Confirmation::SwitchProject(id.to_owned()));
        }
        self.maximized = false;
        self.selected_asset = None;
        self.return_path = None;
        self.tabs.clear();
        self.active = None;
        self.project = id.to_owned();
        self.files.clear();
        self.refresh_files();
        self.status = if self.store.writable() {
            "Project opened. Saves write directly to this checkout.".to_owned()
        } else {
            "Bundled examples opened read-only. Run the native Joy Editor to save project files.".to_owned()
        };
        let restored = self.session.tabs.get(id).cloned().unwrap_or_default();
        let active = self.session.active.get(id).cloned();
        for path in restored {
            if self.files.iter().any(|file| file.path == path) {
                let _ = self.open(&path);
            }
        }
        if let Some(active) = active.filter(|active| self.tab(active).is_some()) {
            self.activate(&active);
        }
        self.remember();
        Outcome::Done
    }

    /// Add a folder with a manifest for this session and open it.
    pub fn open_folder(&mut self, folder: &str) -> Outcome {
        match self.store.open_folder(folder) {
            Ok(summary) => {
                let id = summary.id.clone();
                if !self.projects.iter().any(|project| project.id == id) {
                    self.projects.push(summary);
                }
                self.select_project(&id, false)
            }
            Err(error) => {
                self.status = error.to_string();
                Outcome::Blocked
            }
        }
    }

    /// Rescan the project's assets.
    pub fn refresh_files(&mut self) {
        if self.project.is_empty() {
            return;
        }
        match self.store.list_files(&self.project) {
            Ok(files) => self.files = files,
            Err(error) => self.status = error.to_string(),
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Documents
    // -----------------------------------------------------------------------------------------------------------------

    /// Open `path` in its editor (or activate its tab). Invalid structured assets open as repair views.
    pub fn open(&mut self, path: &str) -> Result<(), String> {
        if self.tab(path).is_some() {
            self.activate(path);
            return Ok(());
        }
        let disk = match self.store.read(&self.project, path) {
            Ok(disk) => disk,
            Err(error) => {
                self.status = error.to_string();
                return Err(error.to_string());
            }
        };
        let content = self.content_for(path, &disk.text);
        if let TabContent::Source(SourceView {
            message: Some(message),
            ..
        }) = &content
        {
            self.status = format!("{path}: {message}");
        }
        let document = WorkspaceDocument::new(&self.project, path, disk);
        self.tabs.push(Tab {
            document,
            content,
            captured_generation: 0,
        });
        self.activate(path);
        Ok(())
    }

    /// Make `path` the active tab; inactive tabs keep their state and stop receiving frames.
    pub fn activate(&mut self, path: &str) {
        if self.active.as_deref() != Some(path) {
            self.maximized = false;
        }
        self.active = self.tab(path).map(|_| path.to_owned());
        if self.active.is_some() {
            self.selected_asset = Some(path.to_owned());
        }
        self.activation_pending = true;
        if let Some(tab) = self.active_tab_mut()
            && let TabContent::Tool(tool) = &mut tab.content
        {
            tool.request_activation();
        }
        self.remember();
    }

    /// Close a tab after capturing it; unsaved edits need confirmation.
    pub fn close(&mut self, path: &str, confirmed: bool) -> Outcome {
        let Some(index) = self.index_of(path) else {
            return Outcome::Done;
        };
        if self.capture(index, false).is_err() {
            return Outcome::Blocked;
        }
        let tab = &self.tabs[index];
        if tab.document.saving {
            self.status = "Wait for this save to finish before closing.".to_owned();
            return Outcome::Blocked;
        }
        if tab.dirty() && !confirmed {
            return Outcome::Confirm(Confirmation::CloseTab(path.to_owned()));
        }
        self.tabs.remove(index);
        if self.active.as_deref() == Some(path) {
            self.active = None;
            if let Some(last) = self.tabs.last().map(|tab| tab.document.path.clone()) {
                self.activate(&last);
                return Outcome::Done;
            }
        }
        self.remember();
        Outcome::Done
    }

    /// Save the active document.
    pub fn save_active(&mut self) {
        if let Some(path) = self.active.clone() {
            self.save(&path);
        }
    }

    /// Capture and write a document against its disk revision; edits made meanwhile stay in the tab.
    pub fn save(&mut self, path: &str) {
        let Some(index) = self.index_of(path) else {
            return;
        };
        if self.capture(index, false).is_err() {
            return;
        }
        let project = self.project.clone();
        let tab = &mut self.tabs[index];
        if tab.document.saving {
            return;
        }
        if !tab.document.dirty() {
            // A tool may report edits that serialize to the saved text.
            if let TabContent::Tool(tool) = &mut tab.content {
                tool.mark_saved();
            }
            return;
        }
        if tab.document.conflict.is_some() {
            self.status = "Resolve the disk conflict before saving.".to_owned();
            return;
        }
        let snapshot = tab.document.snapshot();
        tab.document.saving = true;
        let result = self.store.save(&project, path, &snapshot.text, &snapshot.revision);
        let tab = &mut self.tabs[index];
        tab.document.saving = false;
        match result {
            Ok(disk) => {
                tab.document.saved(&snapshot, &disk);
                if let TabContent::Tool(tool) = &mut tab.content {
                    tool.mark_saved();
                }
                self.status = format!("Saved {path}");
                if matches!(&self.tabs[index].content, TabContent::Source(view) if view.message.is_some()) {
                    self.retry_editor(index);
                }
                self.notify_assets(path);
            }
            Err(error) => {
                if let ProjectError::Conflict {
                    current,
                    ..
                } = &error
                {
                    self.tabs[index].document.conflict = Some(current.clone());
                }
                self.status = error.to_string();
            }
        }
    }

    /// Reconcile every open document with disk: clean tabs reload external edits, dirty tabs record a conflict.
    pub fn poll(&mut self) {
        for index in 0..self.tabs.len() {
            if self.tabs[index].document.saving || self.capture(index, true).is_err() {
                continue;
            }
            let path = self.tabs[index].document.path.clone();
            let Ok(disk) = self.store.read(&self.project, &path) else {
                continue;
            };
            if self.tabs[index].document.external(&disk) {
                self.load_content(index, &disk.text);
                self.notify_assets(&path);
                self.status = format!("Reloaded external change: {path}");
            }
        }
    }

    /// Resolve the active tab's conflict with the disk version, discarding local edits.
    pub fn reload_disk(&mut self, confirmed: bool) -> Outcome {
        let Some(path) = self.active.clone() else {
            return Outcome::Done;
        };
        let Some(index) = self.index_of(&path) else {
            return Outcome::Done;
        };
        if self.tabs[index].document.conflict.is_none() {
            return Outcome::Done;
        }
        if !confirmed {
            return Outcome::Confirm(Confirmation::ReloadDisk(path));
        }
        self.tabs[index].document.reload_disk();
        let text = self.tabs[index].document.text.clone();
        self.load_content(index, &text);
        self.status = format!("Using the disk version of {path}.");
        Outcome::Done
    }

    /// Keep the active tab's edits and acknowledge the disk version; Save then replaces it.
    pub fn keep_local(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.document.keep_local();
            self.status = "Kept your edits. Save File will replace the reviewed disk version.".to_owned();
        }
    }

    /// Open a referenced source from `owner` (Edit Source) and remember the owner for the return button.
    pub fn open_source(&mut self, owner: &str, path: &str) {
        let Some(index) = self.index_of(owner) else {
            return;
        };
        if self.capture(index, false).is_err() {
            self.status = "Fix invalid edits before opening the source.".to_owned();
            return;
        }
        if self.open(path).is_ok() {
            self.return_path = Some(owner.to_owned());
        }
    }

    /// Return to the document that opened the current source.
    pub fn return_to_owner(&mut self) {
        if let Some(owner) = self.return_path.take()
            && self.tab(&owner).is_some()
        {
            self.activate(&owner);
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Asset lifecycle
    // -----------------------------------------------------------------------------------------------------------------

    /// Create a starter asset named `name` of `kind` (without the dot) and open it; never overwrites.
    pub fn create_asset(&mut self, name: &str, kind: &str) -> Result<String, String> {
        let result = new_asset_path(name, kind).and_then(|path| {
            let text = create_asset_text(&path).ok_or_else(|| "Unsupported asset type.".to_owned())?;
            self.store.create(&self.project, &path, &text).map_err(|error| error.to_string())?;
            Ok(path)
        });
        match result {
            Ok(path) => {
                self.refresh_files();
                let _ = self.open(&path);
                self.status = format!("Created {path}");
                Ok(path)
            }
            Err(message) => {
                self.status = message.clone();
                Err(message)
            }
        }
    }

    /// Import a supported text asset as `assets/<file name>`, keeping its format; never overwrites.
    pub fn import_asset(&mut self, file_name: &str, text: &str) -> Result<String, String> {
        let file_name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
        let path = format!("assets/{file_name}");
        if !is_supported_asset_path(&path) {
            let message = "Choose a supported level, object, FORM or particle asset.".to_owned();
            self.status = message.clone();
            return Err(message);
        }
        match self.store.create(&self.project, &path, text) {
            Ok(_) => {
                self.refresh_files();
                let _ = self.open(&path);
                self.status = format!("Imported {path}");
                Ok(path)
            }
            Err(error) => {
                self.status = error.to_string();
                Err(error.to_string())
            }
        }
    }

    /// Check that an asset can be renamed or deleted: an open document must be saved (or discarded) first.
    pub fn check_modifiable(&mut self, path: &str, action: &str) -> Result<(), String> {
        if let Some(index) = self.index_of(path) {
            self.capture(index, false)?;
            let tab = &self.tabs[index];
            if tab.dirty() || tab.document.saving {
                let message = format!("Save or discard edits before {action} this asset.");
                self.status = message.clone();
                return Err(message);
            }
        }
        Ok(())
    }

    /// Rename an asset to `destination` beneath `assets/`; open documents reopen at the new path, closed ones stay
    /// closed.
    pub fn rename_asset(&mut self, path: &str, destination: &str) -> Result<(), String> {
        self.check_modifiable(path, "renaming")?;
        let destination = destination.trim().trim_start_matches("assets/");
        if destination.is_empty() || Some(destination) == path.strip_prefix("assets/") {
            return Ok(());
        }
        let target = format!("assets/{destination}");
        if let Err(error) = self.store.rename(&self.project, path, &target) {
            self.status = error.to_string();
            return Err(error.to_string());
        }
        let was_open = self.tab(path).is_some();
        if was_open {
            self.close(path, true);
        }
        self.refresh_files();
        if was_open {
            let _ = self.open(&target);
        }
        if self.selected_asset.as_deref() == Some(path) {
            self.selected_asset = Some(target.clone());
        }
        self.status = format!("Renamed to {target}");
        Ok(())
    }

    /// Delete an asset after confirmation; an open document must be saved or discarded first.
    pub fn delete_asset(&mut self, path: &str, confirmed: bool) -> Outcome {
        if self.check_modifiable(path, "deleting").is_err() {
            return Outcome::Blocked;
        }
        if !confirmed {
            return Outcome::Confirm(Confirmation::DeleteAsset(path.to_owned()));
        }
        if let Err(error) = self.store.remove(&self.project, path) {
            self.status = error.to_string();
            return Outcome::Blocked;
        }
        if self.tab(path).is_some() {
            self.close(path, true);
        }
        self.refresh_files();
        self.status = format!("Deleted {path}");
        Outcome::Done
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Frames
    // -----------------------------------------------------------------------------------------------------------------

    /// Run disk polling and periodic rescans at `time_seconds` (the frame clock).
    pub fn tick(&mut self, time_seconds: f64) {
        if time_seconds >= self.next_poll {
            self.next_poll = time_seconds + POLL_SECONDS;
            self.poll();
        }
        if time_seconds >= self.next_refresh {
            self.next_refresh = time_seconds + REFRESH_SECONDS;
            self.refresh_files();
        }
        self.capture_levels();
    }

    /// Bring the active level's presentation up to date and route its requests (Edit Source, new objects).
    pub fn sync_active_level(&mut self) {
        let Some((level, mut access)) = self.active_level() else {
            return;
        };
        level.sync(&mut access);
        let created = !access.created.is_empty();
        if created {
            self.refresh_files();
        }
        self.route_level_requests();
    }

    /// Open the sources the active level asked for (Edit Source), remembering it as the owner.
    pub fn route_level_requests(&mut self) {
        let Some(active) = self.active.clone() else {
            return;
        };
        let requests = match self.active_tab_mut().map(|tab| &mut tab.content) {
            Some(TabContent::Level(level)) => level.take_open_requests(),
            _ => return,
        };
        for path in requests {
            self.open_source(&active, &path);
        }
    }

    /// Start tools that have not started and re-apply the active tab's renderer settings; call from the render phase.
    /// A tool that rejects its document becomes a repair view.
    pub fn prepare_tabs(&mut self, context: &mut StartContext<'_>) {
        let mut started = false;
        for index in 0..self.tabs.len() {
            let TabContent::Tool(tool) = &mut self.tabs[index].content else {
                continue;
            };
            if tool.started() {
                continue;
            }
            started = true;
            if let Err(message) = tool.prepare(context) {
                let path = self.tabs[index].document.path.clone();
                let text = self.tabs[index].document.text.clone();
                self.tabs[index].content = TabContent::Source(SourceView {
                    text,
                    message: Some(message.clone()),
                    retry: AssetEditor::Tool,
                });
                self.status = format!("{path}: {message}");
            }
        }
        let activate = std::mem::take(&mut self.activation_pending) || started;
        if let Some(tab) = self.active_tab_mut() {
            match &mut tab.content {
                TabContent::Tool(tool) => {
                    if activate {
                        // Tools share one renderer: start each activation from the defaults so effects another tab
                        // turned on never leak into a tool that does not set them.
                        context.renderer.set_post_process(joy_engine::rendering::PostProcessSettings::DEFAULT);
                        let _ = context.renderer.set_scene_effects(None);
                        tool.request_activation();
                    }
                    tool.activate(context);
                }
                TabContent::Level(level) if activate => level.activated(context.renderer),
                _ => {}
            }
        }
    }

    /// Persist navigation and layout; unsaved source stays in memory.
    pub fn remember(&mut self) {
        if self.project.is_empty() {
            return;
        }
        self.session.project = self.project.clone();
        self.session.tabs.insert(self.project.clone(), self.tabs.iter().map(|tab| tab.document.path.clone()).collect());
        match &self.active {
            Some(active) => {
                self.session.active.insert(self.project.clone(), active.clone());
            }
            None => {
                self.session.active.remove(&self.project);
            }
        }
        // Disk content stays authoritative when preferences cannot be stored.
        let _ = self.session.save(self.storage.as_ref());
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------------------------------------------------

    fn index_of(&self, path: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.document.path == path)
    }

    /// Copy an editor's current text into its document; passive captures skip pending input and never report.
    fn capture(&mut self, index: usize, passive: bool) -> Result<(), String> {
        let tab = &mut self.tabs[index];
        let name = tab.file_name().to_owned();
        let result = match &mut tab.content {
            TabContent::Level(level) => level.capture(passive).map(Some),
            TabContent::Tool(tool) if passive && !tool.has_unsaved_changes() => Ok(None),
            TabContent::Tool(tool) => tool.capture(&name).map(Some),
            TabContent::Source(view) => Ok(Some(view.text.clone())),
        };
        match result {
            Ok(Some(text)) => {
                tab.document.edit(text);
                if let TabContent::Level(level) = &tab.content {
                    tab.captured_generation = level.generation();
                }
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(message) => {
                if !passive {
                    self.status = format!("{}: {message}", tab.document.path);
                }
                Err(message)
            }
        }
    }

    /// Passively capture level editors whose documents changed, so tab state shows unsaved changes.
    fn capture_levels(&mut self) {
        for index in 0..self.tabs.len() {
            let tab = &self.tabs[index];
            if let TabContent::Level(level) = &tab.content
                && level.generation() != tab.captured_generation
            {
                let _ = self.capture(index, true);
            }
        }
    }

    /// The editor for a document's text.
    fn content_for(&self, path: &str, text: &str) -> TabContent {
        let extension = asset_kind(path).unwrap_or_default();
        match asset_editor(path) {
            AssetEditor::Level => match LevelEditor::load(path, text) {
                Ok(level) => TabContent::Level(Box::new(level)),
                Err(message) => TabContent::Source(SourceView {
                    text: text.to_owned(),
                    message: Some(message),
                    retry: AssetEditor::Level,
                }),
            },
            AssetEditor::Tool => match self.tools.create(&extension) {
                Some(tool) => {
                    let name = path.rsplit('/').next().unwrap_or(path).to_owned();
                    TabContent::Tool(Box::new(ToolTab::new(
                        tool,
                        ToolDocument {
                            name,
                            contents: text.to_owned(),
                        },
                    )))
                }
                None => TabContent::Source(SourceView {
                    text: text.to_owned(),
                    message: Some(format!(
                        "No tool for .{extension} files is available in this build; edit the source here."
                    )),
                    retry: AssetEditor::Tool,
                }),
            },
            AssetEditor::Source => TabContent::Source(SourceView {
                text: text.to_owned(),
                message: None,
                retry: AssetEditor::Source,
            }),
        }
    }

    /// Hand new disk text to a tab's editor; an editor that rejects it becomes a repair view.
    fn load_content(&mut self, index: usize, text: &str) {
        let path = self.tabs[index].document.path.clone();
        let name = self.tabs[index].file_name().to_owned();
        let result = match &mut self.tabs[index].content {
            TabContent::Level(level) => level.reload(text).map_err(|message| (message, AssetEditor::Level)),
            TabContent::Tool(tool) => tool
                .open(ToolDocument {
                    name,
                    contents: text.to_owned(),
                })
                .map_err(|message| (message, AssetEditor::Tool)),
            TabContent::Source(view) => {
                view.text = text.to_owned();
                if view.message.is_some() {
                    self.retry_editor(index);
                }
                Ok(())
            }
        };
        if let Err((message, retry)) = result {
            self.status = format!("{path}: {message}");
            self.tabs[index].content = TabContent::Source(SourceView {
                text: text.to_owned(),
                message: Some(message),
                retry,
            });
        }
        if let TabContent::Level(level) = &self.tabs[index].content {
            self.tabs[index].captured_generation = level.generation();
        }
    }

    /// Try a repair view's editor again with the document text.
    fn retry_editor(&mut self, index: usize) {
        let path = self.tabs[index].document.path.clone();
        let text = self.tabs[index].document.text.clone();
        let TabContent::Source(view) = &self.tabs[index].content else {
            return;
        };
        if view.retry == AssetEditor::Source {
            return;
        }
        match self.content_for(&path, &text) {
            TabContent::Source(SourceView {
                message: Some(message),
                retry,
                ..
            }) => {
                self.tabs[index].content = TabContent::Source(SourceView {
                    text,
                    message: Some(message),
                    retry,
                });
            }
            content => {
                self.tabs[index].content = content;
                self.tabs[index].captured_generation = 0;
                self.activation_pending = true;
            }
        }
    }

    /// Committed source changed: dependent previews re-read their content while keeping their history.
    fn notify_assets(&mut self, changed: &str) {
        for tab in &mut self.tabs {
            if tab.document.path != changed
                && let TabContent::Level(level) = &mut tab.content
            {
                level.assets_changed();
            }
        }
    }
}
