// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Native project files.
//!
//! A hosting workspace (the game repository) keeps its projects in `projects/<id>/`, each with a `project.joyproject`
//! manifest. Open Folder adds any other folder with a manifest for the session; it never changes the workspace.
//!
//! Every operation validates its project id and path first: assets live beneath the project's `assets/` folder, use a
//! built-in or manifest-declared extension, and are reached through plain path segments without symbolic links. Saves
//! compare revisions, write a temporary sibling, check the revision again, move the replaced file into an ignored
//! `.joy-editor-recovery/` folder beside the asset, and install the new file with a hard link that cannot replace a
//! competing write; a change that lands during the save is preserved and reported as a conflict. Operations run on
//! the editor's thread, so writes are serialized by construction.

use std::collections::BTreeMap;
use std::fs;
use std::io::{
    ErrorKind,
    Write,
};
use std::path::{
    Component,
    Path,
    PathBuf,
};
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use joy_engine::gameplay::{
    parse_level,
    parse_object,
};
use joy_engine::project::{
    JOY_PROJECT_MANIFEST,
    ProjectManifest,
    parse_project_manifest_text,
};

use super::{
    ASSET_DIRECTORY,
    AssetEntry,
    DiskDocument,
    MAXIMUM_ASSET_BYTES,
    ProjectError,
    ProjectStore,
    ProjectSummary,
    compare_paths,
    extension_of,
    is_project_asset_path,
    project_formats,
    revision,
};

/// Folder, relative to the workspace root, whose child folders are authorable projects.
pub const DEFAULT_PROJECTS_DIRECTORY: &str = "projects";

/// Folder beside an asset that keeps the files saves replaced.
pub const RECOVERY_DIRECTORY: &str = ".joy-editor-recovery";

/// Most assets one project may list.
const ASSET_LIMIT: usize = 10_000;

/// Distinguishes temporary and recovery names created in the same instant.
static UNIQUE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A retained pre-save copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryCopy {
    /// Project-relative path inside a `.joy-editor-recovery/` folder.
    pub path: String,
    /// Size in bytes.
    pub bytes: u64,
}

/// Owns access to one hosting workspace's projects and the folders opened this session.
pub struct ProjectFiles {
    // The hosting workspace and its projects folder.
    root: PathBuf,
    projects_root: PathBuf,

    // Folders opened with Open Folder, by generated id; they live only for this session.
    external: BTreeMap<String, PathBuf>,

    // Test seam: runs after a save's temporary file is checked and before the replaced file moves to recovery.
    save_prepared: Option<Box<dyn FnMut()>>,
}

impl ProjectFiles {
    /// Projects beneath `root/projects`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_projects_directory(root, DEFAULT_PROJECTS_DIRECTORY)
    }

    /// Projects beneath `root/projects_directory`, for hosting workspaces with another layout.
    pub fn with_projects_directory(root: impl Into<PathBuf>, projects_directory: &str) -> Self {
        let root = absolute(root.into());
        let projects_root = root.join(projects_directory);
        Self { root, projects_root, external: BTreeMap::new(), save_prepared: None }
    }

    /// The hosting workspace root.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Run `hook` inside each save after the second revision check, just before the replaced file moves to recovery.
    ///
    /// Tests use it to simulate another editor writing during a save.
    #[doc(hidden)]
    pub fn on_save_prepared(&mut self, hook: impl FnMut() + 'static) {
        self.save_prepared = Some(Box::new(hook));
    }

    /// Retained pre-save copies in the project, without exposing other hidden files.
    pub fn list_recovery(&mut self, project: &str) -> Result<Vec<RecoveryCopy>, ProjectError> {
        let root = self.project_root(project)?;
        let mut copies = Vec::new();
        visit_recovery(&root.join(ASSET_DIRECTORY), ASSET_DIRECTORY, &mut copies);
        copies.sort_by(|left, right| compare_paths(&left.path, &right.path));
        Ok(copies)
    }

    /// Remove one recovery copy by its exact path, or every copy in the project when `path` is `None`; returns how many
    /// were removed.
    pub fn remove_recovery(&mut self, project: &str, path: Option<&str>) -> Result<usize, ProjectError> {
        let root = self.project_root(project)?;
        let copies = self.list_recovery(project)?;
        let selected: Vec<&RecoveryCopy> =
            copies.iter().filter(|copy| path.is_none_or(|path| copy.path == path)).collect();
        if path.is_some() && selected.len() != 1 {
            return Err(ProjectError::Rejected("Recovery copy does not exist.".to_owned()));
        }
        for copy in &selected {
            let _ = fs::remove_file(root.join(&copy.path));
        }
        Ok(selected.len())
    }

    /// Resolve a project directory: a validated id inside the workspace's projects folder, or an opened folder.
    fn project_root(&self, project: &str) -> Result<PathBuf, ProjectError> {
        let valid = !project.is_empty()
            && project.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
        if !valid {
            return Err(ProjectError::Rejected("Invalid project.".to_owned()));
        }
        if let Some(external) = self.external.get(project) {
            return Ok(external.clone());
        }
        let root = self.projects_root.join(project);
        let metadata =
            fs::symlink_metadata(&root).map_err(|_| ProjectError::Rejected("Invalid project.".to_owned()))?;
        let inside_workspace =
            canonical(&self.root).and_then(|workspace| Ok(inside(&workspace, &canonical(&root)?))).unwrap_or(false);
        if metadata.file_type().is_symlink() || !metadata.is_dir() || !inside_workspace {
            return Err(ProjectError::Rejected("Project must be inside this checkout.".to_owned()));
        }
        Ok(root)
    }

    /// Resolve an existing asset after validating every path component.
    fn resolve_file(&self, project: &str, relative: &str) -> Result<PathBuf, ProjectError> {
        let root = self.project_root(project)?;
        if !is_project_asset_path(relative, &formats_of(&root)) {
            return Err(ProjectError::Rejected(
                "Only project FORM, particle and level assets can be opened.".to_owned(),
            ));
        }
        let mut file = root.clone();
        for part in relative.split('/') {
            file.push(part);
            let metadata = fs::symlink_metadata(&file)
                .map_err(|error| ProjectError::Io(format!("Cannot open {relative}: {}", describe_io(&error))))?;
            if metadata.file_type().is_symlink() {
                return Err(ProjectError::Rejected("Symbolic links are not editable project assets.".to_owned()));
            }
        }
        let contained = inside(&canonical(&root)?, &canonical(&file)?);
        if !contained || !fs::symlink_metadata(&file).is_ok_and(|metadata| metadata.is_file()) {
            return Err(ProjectError::Rejected("File is outside the project.".to_owned()));
        }
        Ok(file)
    }

    /// Resolve the location for a new asset, creating its folders, and reject folders that leave the project.
    fn resolve_new_file(&self, project: &str, relative: &str) -> Result<PathBuf, ProjectError> {
        let root = self.project_root(project)?;
        if !is_project_asset_path(relative, &formats_of(&root)) {
            return Err(ProjectError::Rejected(
                "Asset paths must stay beneath assets/ and use a project-supported extension.".to_owned(),
            ));
        }
        let file = root.join(relative);
        let parent = file.parent().map(Path::to_path_buf).unwrap_or_else(|| root.clone());
        fs::create_dir_all(&parent).map_err(|error| ProjectError::Io(describe_io(&error)))?;
        let linked = fs::symlink_metadata(&parent).map(|metadata| metadata.file_type().is_symlink()).unwrap_or(true);
        if linked || !inside(&canonical(&root)?, &canonical(&parent)?) {
            return Err(ProjectError::Rejected("Asset directories must not be symbolic links.".to_owned()));
        }
        Ok(file)
    }

    /// Describe a project folder from its manifest.
    fn describe(id: &str, root: &Path, manifest: ProjectManifest, external: bool) -> ProjectSummary {
        ProjectSummary {
            id: id.to_owned(),
            name: manifest.name.clone(),
            location: root.display().to_string(),
            external,
            manifest,
        }
    }

    /// The save steps after the first revision check; `temporary` is removed by the caller either way.
    fn install(
        &mut self,
        project: &str,
        relative: &str,
        file: &Path,
        temporary: &Path,
        text: &str,
        expected_revision: &str,
    ) -> Result<(), ProjectError> {
        let permissions = fs::metadata(file).map(|metadata| metadata.permissions()).ok();
        let mut handle = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(temporary)
            .map_err(|error| ProjectError::Io(describe_io(&error)))?;
        handle.write_all(text.as_bytes()).map_err(|error| ProjectError::Io(describe_io(&error)))?;
        handle.sync_all().map_err(|error| ProjectError::Io(describe_io(&error)))?;
        drop(handle);
        if let Some(permissions) = permissions {
            let _ = fs::set_permissions(temporary, permissions);
        }

        // Check again after preparing the temporary file to catch external editor writes.
        let latest = self.read(project, relative)?;
        if latest.revision != expected_revision {
            return Err(ProjectError::Conflict { current: latest, during_save: false });
        }
        if let Some(hook) = self.save_prepared.as_mut() {
            hook();
        }

        // Move the exact replaced file into recovery, then install exclusively. A competing external atomic save wins;
        // its data is never overwritten by the rename.
        let directory = file.parent().map(|parent| parent.join(RECOVERY_DIRECTORY));
        let Some(directory) = directory else {
            return Err(ProjectError::Io("Asset has no folder.".to_owned()));
        };
        fs::create_dir_all(&directory).map_err(|error| ProjectError::Io(describe_io(&error)))?;
        if fs::symlink_metadata(&directory).map(|metadata| metadata.file_type().is_symlink()).unwrap_or(true) {
            return Err(ProjectError::Rejected("Recovery directory must not be a symbolic link.".to_owned()));
        }
        let name = file.file_name().map_or_else(|| "asset".to_owned(), |name| name.to_string_lossy().into_owned());
        let backup = directory.join(format!("{name}.{}", unique_suffix()));
        fs::rename(file, &backup).map_err(|error| ProjectError::Io(describe_io(&error)))?;
        let replaced = fs::read_to_string(&backup).unwrap_or_default();
        if revision(&replaced) != expected_revision {
            restore(&backup, file)?;
            return Err(ProjectError::Conflict { current: self.read(project, relative)?, during_save: true });
        }
        if let Err(error) = fs::hard_link(temporary, file) {
            // The old file stays in recovery, including late in-place writes.
            let _ = fs::hard_link(&backup, file);
            if error.kind() == ErrorKind::AlreadyExists {
                return Err(ProjectError::Conflict { current: self.read(project, relative)?, during_save: true });
            }
            return Err(ProjectError::Io(describe_io(&error)));
        }
        Ok(())
    }
}

impl ProjectStore for ProjectFiles {
    fn list_projects(&mut self) -> Result<Vec<ProjectSummary>, ProjectError> {
        let mut projects = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.projects_root) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_directory = entry.file_type().is_ok_and(|kind| kind.is_dir());
                let valid_name =
                    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte));
                if !is_directory || !valid_name {
                    continue;
                }
                // A folder without a valid manifest is not an authorable project.
                if let Ok(manifest) = read_manifest(&entry.path()) {
                    projects.push(Self::describe(&name, &entry.path(), manifest, false));
                }
            }
        }
        for (id, root) in &self.external {
            if let Ok(manifest) = read_manifest(root) {
                projects.push(Self::describe(id, root, manifest, true));
            }
        }
        projects.sort_by(|left, right| compare_paths(&left.name, &right.name));
        Ok(projects)
    }

    fn list_files(&mut self, project: &str) -> Result<Vec<AssetEntry>, ProjectError> {
        let root = self.project_root(project)?;
        let formats = formats_of(&root);
        let assets = root.join(ASSET_DIRECTORY);
        match fs::symlink_metadata(&assets) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(ProjectError::Rejected("Asset directories must not be symbolic links.".to_owned()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(ProjectError::Io(describe_io(&error))),
        }
        let mut files = Vec::new();
        visit_assets(&assets, ASSET_DIRECTORY, &formats, &mut files)?;
        files.sort_by(|left, right| compare_paths(&left.path, &right.path));
        Ok(files)
    }

    fn read(&mut self, project: &str, path: &str) -> Result<DiskDocument, ProjectError> {
        let file = self.resolve_file(project, path)?;
        let metadata = fs::symlink_metadata(&file).map_err(|error| ProjectError::Io(describe_io(&error)))?;
        if metadata.len() > MAXIMUM_ASSET_BYTES as u64 {
            return Err(ProjectError::Rejected("Assets must be 6 MB or smaller.".to_owned()));
        }
        let bytes = fs::read(&file).map_err(|error| ProjectError::Io(describe_io(&error)))?;
        let text =
            String::from_utf8(bytes).map_err(|_| ProjectError::Rejected(format!("{path} is not UTF-8 text.")))?;
        Ok(DiskDocument::new(text))
    }

    fn save(
        &mut self,
        project: &str,
        path: &str,
        text: &str,
        expected_revision: &str,
    ) -> Result<DiskDocument, ProjectError> {
        if text.len() > MAXIMUM_ASSET_BYTES {
            return Err(ProjectError::Rejected("Invalid asset save; maximum size is 6 MB.".to_owned()));
        }
        let file = self.resolve_file(project, path)?;
        let current = self.read(project, path)?;
        if current.revision != expected_revision {
            return Err(ProjectError::Conflict { current, during_save: false });
        }
        let name = file.file_name().map_or_else(|| "asset".to_owned(), |name| name.to_string_lossy().into_owned());
        let temporary = file.with_file_name(format!("{name}.{}.tmp", unique_suffix()));
        let result = self.install(project, path, &file, &temporary, text, expected_revision);
        let _ = fs::remove_file(&temporary);
        result.map(|()| DiskDocument::new(text))
    }

    fn create(&mut self, project: &str, path: &str, text: &str) -> Result<DiskDocument, ProjectError> {
        if text.len() > MAXIMUM_ASSET_BYTES {
            return Err(ProjectError::Rejected("Invalid asset creation; maximum size is 6 MB.".to_owned()));
        }
        // Structured documents must be valid before they exist on disk.
        match extension_of(path).as_str() {
            ".joyobject" => {
                parse_object(text).map_err(|error| ProjectError::Rejected(error.to_string()))?;
            }
            ".joylevel" => {
                parse_level(text).map_err(|error| ProjectError::Rejected(error.to_string()))?;
            }
            _ => {}
        }
        let file = self.resolve_new_file(project, path)?;
        let mut handle = match fs::OpenOptions::new().write(true).create_new(true).open(&file) {
            Ok(handle) => handle,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => return Err(ProjectError::AlreadyExists),
            Err(error) => return Err(ProjectError::Io(describe_io(&error))),
        };
        handle.write_all(text.as_bytes()).map_err(|error| ProjectError::Io(describe_io(&error)))?;
        Ok(DiskDocument::new(text))
    }

    fn rename(&mut self, project: &str, path: &str, destination: &str) -> Result<(), ProjectError> {
        let source = self.resolve_file(project, path)?;
        let target = self.resolve_new_file(project, destination)?;
        match fs::hard_link(&source, &target) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::AlreadyExists => return Err(ProjectError::AlreadyExists),
            Err(error) => return Err(ProjectError::Io(describe_io(&error))),
        }
        fs::remove_file(&source).map_err(|error| ProjectError::Io(describe_io(&error)))
    }

    fn remove(&mut self, project: &str, path: &str) -> Result<(), ProjectError> {
        let file = self.resolve_file(project, path)?;
        fs::remove_file(file).map_err(|error| ProjectError::Io(describe_io(&error)))
    }

    fn writable(&self) -> bool {
        true
    }

    fn open_folder(&mut self, folder: &str) -> Result<ProjectSummary, ProjectError> {
        let root = canonical(Path::new(folder))?;
        let manifest = read_manifest(&root).map_err(|message| {
            ProjectError::Rejected(format!("This is not a compatible Joy project ({JOY_PROJECT_MANIFEST}): {message}"))
        })?;
        let id = format!("external-{}", &revision(&root.display().to_string())[..12]);
        self.external.insert(id.clone(), root.clone());
        Ok(Self::describe(&id, &root, manifest, true))
    }
}

/// Read and validate a folder's manifest.
fn read_manifest(root: &Path) -> Result<ProjectManifest, String> {
    let text = fs::read_to_string(root.join(JOY_PROJECT_MANIFEST)).map_err(|error| describe_io(&error))?;
    parse_project_manifest_text(&text).map_err(|error| error.to_string())
}

/// Built-in formats plus the project manifest's extensions; an unreadable manifest leaves the built-ins.
fn formats_of(root: &Path) -> Vec<String> {
    project_formats(read_manifest(root).ok().as_ref())
}

/// Collect supported files beneath `directory`, skipping hidden folders and every symbolic link.
fn visit_assets(
    directory: &Path,
    relative: &str,
    formats: &[String],
    files: &mut Vec<AssetEntry>,
) -> Result<(), ProjectError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(ProjectError::Io(describe_io(&error))),
    };
    let mut entries: Vec<fs::DirEntry> = entries.flatten().collect();
    entries.sort_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{relative}/{name}");
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && !name.starts_with('.') {
            visit_assets(&entry.path(), &path, formats, files)?;
        } else if kind.is_file() && formats.contains(&extension_of(&name)) {
            let kind = extension_of(&name).trim_start_matches('.').to_owned();
            files.push(AssetEntry { path, kind });
        }
        if files.len() > ASSET_LIMIT {
            return Err(ProjectError::Rejected("Project exceeds the 10,000 asset browsing limit.".to_owned()));
        }
    }
    Ok(())
}

/// Collect files inside `.joy-editor-recovery/` folders beneath `directory`.
fn visit_recovery(directory: &Path, relative: &str, copies: &mut Vec<RecoveryCopy>) {
    let Ok(entries) = fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() && name == RECOVERY_DIRECTORY {
            for copy in fs::read_dir(entry.path()).into_iter().flatten().flatten() {
                if copy.file_type().is_ok_and(|kind| kind.is_file()) {
                    let bytes = copy.metadata().map(|metadata| metadata.len()).unwrap_or(0);
                    let copy_name = copy.file_name().to_string_lossy().into_owned();
                    copies.push(RecoveryCopy { path: format!("{relative}/{name}/{copy_name}"), bytes });
                }
            }
        } else if kind.is_dir() && !name.starts_with('.') {
            visit_recovery(&entry.path(), &format!("{relative}/{name}"), copies);
        }
    }
}

/// Put a recovery copy back at `file` unless something already took the name.
fn restore(backup: &Path, file: &Path) -> Result<(), ProjectError> {
    match fs::hard_link(backup, file) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(ProjectError::Io(describe_io(&error))),
    }
}

/// A suffix unique to this process and moment, for temporary and recovery names.
fn unique_suffix() -> String {
    let nanos =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
    let count = UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{nanos:x}-{count}", std::process::id())
}

/// Resolve symbolic links and `..` segments.
fn canonical(path: &Path) -> Result<PathBuf, ProjectError> {
    fs::canonicalize(path).map_err(|error| ProjectError::Io(format!("{}: {}", path.display(), describe_io(&error))))
}

/// Whether `file` lies inside `root` (or is `root`), after both were made canonical.
fn inside(root: &Path, file: &Path) -> bool {
    file.strip_prefix(root).is_ok_and(|relative| !relative.components().any(|part| part == Component::ParentDir))
}

/// An absolute form of `path` without touching the file system.
fn absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    std::env::current_dir().map(|directory| directory.join(&path)).unwrap_or(path)
}

/// A short description of an I/O failure.
fn describe_io(error: &std::io::Error) -> String {
    match error.kind() {
        ErrorKind::NotFound => "No such file or folder.".to_owned(),
        ErrorKind::PermissionDenied => "Permission denied.".to_owned(),
        _ => error.to_string(),
    }
}
