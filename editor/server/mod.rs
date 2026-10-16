// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Project file access for Joy Editor, in process.
//!
//! The TypeScript workspace reached project files through a Vite middleware (`plugin.mjs`) backed by
//! `project-files.mjs`. The Rust editor owns its files directly: [`ProjectStore`] is the operation set the middleware
//! served (projects, assets, read, save, create, rename, delete), [`ProjectFiles`](project_files::ProjectFiles) is the
//! native implementation with the same path policy (manifest-supported assets beneath `assets/`, no symbolic links, no
//! traversal, revision-checked and serialized saves, recovery copies), and [`BundledProjects`] is the read-only store
//! the browser build opens, since a static page has no project file service.
//!
//! Paths are project-relative with forward slashes (`assets/levels/track.joylevel`). Revisions are content hashes: a
//! save succeeds only when the file still has the revision the editor read.

mod bundled_projects;
#[cfg(not(target_arch = "wasm32"))]
pub mod project_files;

use std::fmt;

use joy_engine::project::ProjectManifest;

pub use bundled_projects::{BUNDLED_PROJECT_ID, BundledProjects};

/// Largest asset the editor reads or writes, in bytes (the TypeScript service's 6 MB bound).
pub const MAXIMUM_ASSET_BYTES: usize = 6_000_000;

/// Asset formats every project supports, as lowercase dot extensions; manifests may add more.
pub const BUILT_IN_FORMATS: [&str; 5] = [".form", ".formlab", ".joyfx", ".joylevel", ".joyobject"];

/// Folder beneath a project that holds every authorable asset.
pub const ASSET_DIRECTORY: &str = "assets";

/// One project the editor can open.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSummary {
    /// Stable identifier: the folder name beneath the workspace's `projects/`, or `external-…` for an opened folder.
    pub id: String,
    /// Display name from the manifest.
    pub name: String,
    /// Where the project lives, for Project Details.
    pub location: String,
    /// Whether the folder was opened with Open Folder rather than discovered in the hosting workspace.
    pub external: bool,
    /// The validated manifest.
    pub manifest: ProjectManifest,
}

/// One supported asset in a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    /// Project-relative path beginning with `assets/`.
    pub path: String,
    /// Extension without the dot, for example `joylevel`.
    pub kind: String,
}

impl AssetEntry {
    /// The file name without folders.
    pub fn file_name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The containing folder, for example `assets/levels`.
    pub fn folder(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(folder, _)| folder)
    }
}

/// A file's text and the revision it was read at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiskDocument {
    /// UTF-8 contents.
    pub text: String,
    /// Content revision from [`revision`].
    pub revision: String,
}

impl DiskDocument {
    /// A document for `text` with its computed revision.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let revision = revision(&text);
        Self { text, revision }
    }
}

/// Why a project operation did not happen. Messages match the TypeScript service where it had one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectError {
    /// The file changed since it was read; `current` is what is on disk now. Nothing was written.
    Conflict {
        /// The disk version the editor should offer.
        current: DiskDocument,
        /// Whether the change landed while the save was installing (the external version was preserved).
        during_save: bool,
    },
    /// A file already exists at the destination of a create, import, or rename.
    AlreadyExists,
    /// The path or project failed the path policy; the message explains which rule.
    Rejected(String),
    /// The store cannot write (the browser build's bundled examples).
    ReadOnly,
    /// A file system operation failed.
    Io(String),
}

impl fmt::Display for ProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict { during_save: false, .. } => {
                formatter.write_str("This file changed on disk. Review the conflict before saving.")
            }
            Self::Conflict { during_save: true, .. } => {
                formatter.write_str("This file changed during save. The external version was preserved.")
            }
            Self::AlreadyExists => formatter.write_str("A file with this name already exists."),
            Self::Rejected(message) | Self::Io(message) => formatter.write_str(message),
            Self::ReadOnly => formatter.write_str(
                "These bundled examples are read-only. Run the native Joy Editor to save project files; Download keeps a copy.",
            ),
        }
    }
}

impl std::error::Error for ProjectError {}

/// The project file operations the editor needs; the in-process replacement for the workspace file service.
///
/// Implementations enforce their own boundary: callers pass untrusted project ids and paths.
pub trait ProjectStore {
    /// Projects available to open, sorted by name.
    fn list_projects(&mut self) -> Result<Vec<ProjectSummary>, ProjectError>;

    /// Supported assets beneath the project's `assets/` folder, sorted by path.
    fn list_files(&mut self, project: &str) -> Result<Vec<AssetEntry>, ProjectError>;

    /// Read an asset and its revision.
    fn read(&mut self, project: &str, path: &str) -> Result<DiskDocument, ProjectError>;

    /// Replace an existing asset provided it is still at `expected_revision`; returns the written document.
    fn save(
        &mut self,
        project: &str,
        path: &str,
        text: &str,
        expected_revision: &str,
    ) -> Result<DiskDocument, ProjectError>;

    /// Create a new asset; never replaces an existing file.
    fn create(&mut self, project: &str, path: &str, text: &str) -> Result<DiskDocument, ProjectError>;

    /// Rename an asset; never replaces an existing file.
    fn rename(&mut self, project: &str, path: &str, destination: &str) -> Result<(), ProjectError>;

    /// Delete an asset.
    fn remove(&mut self, project: &str, path: &str) -> Result<(), ProjectError>;

    /// Whether saves, creation, renames, and deletion can succeed.
    fn writable(&self) -> bool;

    /// Add a compatible folder (one with a `project.joyproject` manifest) for this session and describe it.
    fn open_folder(&mut self, folder: &str) -> Result<ProjectSummary, ProjectError> {
        let _ = folder;
        Err(ProjectError::Rejected("Open Folder needs the native Joy Editor.".to_owned()))
    }
}

/// A content revision for `text`: a 64-bit FNV-1a hash and the byte length, in hexadecimal.
///
/// Revisions only need to tell versions of one file apart within an editing session; the TypeScript service used a
/// SHA-256 digest for the same purpose.
pub fn revision(text: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}{:08x}", text.len())
}

/// The lowercase extension of a path's final component including the dot (Node's `path.extname`), or an empty string.
pub fn extension_of(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(index) if index > 0 => name[index..].to_lowercase(),
        _ => String::new(),
    }
}

/// Whether `path` names an asset beneath `assets/` with one of `formats`, using only plain path segments.
pub fn is_project_asset_path(path: &str, formats: &[String]) -> bool {
    path.starts_with("assets/")
        && !path.contains('\\')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
        && formats.contains(&extension_of(path))
}

/// The built-in formats plus a manifest's capability extensions, without duplicates.
pub fn project_formats(manifest: Option<&ProjectManifest>) -> Vec<String> {
    let mut formats: Vec<String> = BUILT_IN_FORMATS.iter().map(|format| (*format).to_owned()).collect();
    for capability in manifest.map(|manifest| manifest.capabilities.as_slice()).unwrap_or_default() {
        for extension in &capability.extensions {
            if !formats.contains(extension) {
                formats.push(extension.clone());
            }
        }
    }
    formats
}

/// Order asset paths like JavaScript's `localeCompare` for plain ASCII names: case-insensitively, then exactly.
pub(crate) fn compare_paths(left: &str, right: &str) -> std::cmp::Ordering {
    left.to_lowercase().cmp(&right.to_lowercase()).then_with(|| left.cmp(right))
}
