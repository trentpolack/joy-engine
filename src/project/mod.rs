// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Content layer: project-level authoring metadata shared by Joy Editor and the desktop host. Mirrors the TypeScript
//! `src/project/` module.

pub mod project_manifest;

pub use project_manifest::{
    JOY_PROJECT_MANIFEST, PROJECT_SCHEMA_VERSION, ProjectCapability, ProjectManifest, ProjectManifestError,
    ProjectPreview, parse_project_manifest, parse_project_manifest_text,
};
