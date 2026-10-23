// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Read-only example projects compiled into the editor.
//!
//! A static browser page has no project file service. The browser build opens these bundled examples, so
//! every editor (levels, objects, FORM, JOYFX) can be explored; saves explain that project files need the native
//! editor, and Download hands the current document to the browser.

use joy_engine::project::{
    ProjectManifest,
    parse_project_manifest_text,
};

use super::{
    AssetEntry,
    DiskDocument,
    ProjectError,
    ProjectStore,
    ProjectSummary,
    compare_paths,
    extension_of,
};

/// The id of the bundled example project.
pub const BUNDLED_PROJECT_ID: &str = "examples";

/// The bundled project's manifest.
const MANIFEST: &str = include_str!("../../assets/examples/project.joyproject");

/// The bundled assets: project-relative paths and their contents.
const ASSETS: [(&str, &str); 7] = [
    ("assets/levels/showcase.joylevel", include_str!("../../assets/examples/assets/levels/showcase.joylevel")),
    (
        "assets/objects/lantern-post.joyobject",
        include_str!("../../assets/examples/assets/objects/lantern-post.joyobject"),
    ),
    ("assets/form/cottage.forminst", include_str!("../../../assets/form/structures/cottage.forminst")),
    ("assets/form/building.form", include_str!("../../../assets/form/structures/building.form")),
    ("assets/form/satellite-garden.form", include_str!("../../../assets/examples/satellite-garden.form")),
    ("assets/effects/lantern.joyfx", include_str!("../../../crates/joy-renderer/tests/fixtures/joyfx/lantern.joyfx")),
    (
        "assets/effects/orbiting-lights.joyfx",
        include_str!("../../../tools/joyfx-editor/assets/joyfx/orbiting-lights.joyfx"),
    ),
];

/// The read-only example project store.
pub struct BundledProjects {
    manifest: Option<ProjectManifest>,
}

impl Default for BundledProjects {
    fn default() -> Self {
        Self::new()
    }
}

impl BundledProjects {
    /// The bundled examples.
    pub fn new() -> Self {
        Self {
            manifest: parse_project_manifest_text(MANIFEST).ok(),
        }
    }

    fn check_project(project: &str) -> Result<(), ProjectError> {
        if project == BUNDLED_PROJECT_ID { Ok(()) } else { Err(ProjectError::Rejected("Invalid project.".to_owned())) }
    }
}

impl ProjectStore for BundledProjects {
    fn list_projects(&mut self) -> Result<Vec<ProjectSummary>, ProjectError> {
        Ok(self
            .manifest
            .iter()
            .map(|manifest| ProjectSummary {
                id: BUNDLED_PROJECT_ID.to_owned(),
                name: manifest.name.clone(),
                location: "Bundled with Joy Editor (read-only)".to_owned(),
                external: false,
                manifest: manifest.clone(),
            })
            .collect())
    }

    fn list_files(&mut self, project: &str) -> Result<Vec<AssetEntry>, ProjectError> {
        Self::check_project(project)?;
        let mut files: Vec<AssetEntry> = ASSETS
            .iter()
            .map(|(path, _)| AssetEntry {
                path: (*path).to_owned(),
                kind: extension_of(path).trim_start_matches('.').to_owned(),
            })
            .collect();
        files.sort_by(|left, right| compare_paths(&left.path, &right.path));
        Ok(files)
    }

    fn read(&mut self, project: &str, path: &str) -> Result<DiskDocument, ProjectError> {
        Self::check_project(project)?;
        ASSETS
            .iter()
            .find(|(candidate, _)| *candidate == path)
            .map(|(_, text)| DiskDocument::new(*text))
            .ok_or_else(|| ProjectError::Io(format!("Cannot open {path}: No such file or folder.")))
    }

    fn save(&mut self, project: &str, _path: &str, _text: &str, _revision: &str) -> Result<DiskDocument, ProjectError> {
        Self::check_project(project)?;
        Err(ProjectError::ReadOnly)
    }

    fn create(&mut self, project: &str, _path: &str, _text: &str) -> Result<DiskDocument, ProjectError> {
        Self::check_project(project)?;
        Err(ProjectError::ReadOnly)
    }

    fn rename(&mut self, project: &str, _path: &str, _destination: &str) -> Result<(), ProjectError> {
        Self::check_project(project)?;
        Err(ProjectError::ReadOnly)
    }

    fn remove(&mut self, project: &str, _path: &str) -> Result<(), ProjectError> {
        Self::check_project(project)?;
        Err(ProjectError::ReadOnly)
    }

    fn writable(&self) -> bool {
        false
    }
}
