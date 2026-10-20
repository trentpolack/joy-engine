// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Locating the hosting workspace (the game repository whose `projects/` the editor authors).
//!
//! The native binary accepts, in order: `--workspace=<dir>` (or `--workspace <dir>`), the `JOY_EDITOR_WORKSPACE`
//! environment variable, and the nearest folder at or above the current directory that holds `joy.joyconfig`.
//! Without any of them the editor still opens: Open Folder can add any project folder.

use std::path::{
    Path,
    PathBuf,
};

/// The environment variable naming the hosting workspace.
pub const WORKSPACE_ENVIRONMENT_VARIABLE: &str = "JOY_EDITOR_WORKSPACE";

/// The configuration file that marks a hosting workspace's root.
pub const WORKSPACE_CONFIG: &str = "joy.joyconfig";

/// How the hosting workspace was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceSource {
    /// `--workspace` on the command line.
    Argument,
    /// The `JOY_EDITOR_WORKSPACE` environment variable.
    Environment,
    /// The nearest `joy.joyconfig` at or above the current directory.
    Config,
}

/// A located hosting workspace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostingWorkspace {
    /// Absolute workspace root.
    pub root: PathBuf,
    /// How it was found.
    pub source: WorkspaceSource,
}

/// Find the hosting workspace from command-line `arguments` (without the program name), the value of
/// `JOY_EDITOR_WORKSPACE`, and the current directory. Relative paths resolve against `current_directory`.
///
/// Fails with a message naming every option when nothing identifies a workspace, or when an explicit choice is not a
/// folder.
pub fn locate_workspace(
    arguments: &[String],
    environment: Option<&str>,
    current_directory: &Path,
) -> Result<HostingWorkspace, String> {
    let explicit =
        launch_option(arguments, "workspace").map(|value| (value, WorkspaceSource::Argument)).or_else(|| {
            environment.filter(|value| !value.is_empty()).map(|value| (value.to_owned(), WorkspaceSource::Environment))
        });
    if let Some((value, source)) = explicit {
        let root = current_directory.join(value);
        if !root.is_dir() {
            return Err(format!("The hosting workspace {} is not a folder.", root.display()));
        }
        return Ok(HostingWorkspace {
            root: std::fs::canonicalize(&root).unwrap_or(root),
            source,
        });
    }
    find_workspace_root(current_directory)
        .map(|root| HostingWorkspace { root, source: WorkspaceSource::Config })
        .ok_or_else(|| {
            format!(
                "No hosting workspace found: pass --workspace=<folder>, set {WORKSPACE_ENVIRONMENT_VARIABLE}, or run Joy Editor inside a folder with {WORKSPACE_CONFIG}. Open Folder still opens any project."
            )
        })
}

/// The nearest folder at or above `start` that contains `joy.joyconfig`.
pub fn find_workspace_root(start: &Path) -> Option<PathBuf> {
    let start = std::fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());
    start.ancestors().find(|folder| folder.join(WORKSPACE_CONFIG).is_file()).map(Path::to_path_buf)
}

/// The value of `--name=value` or `--name value` in `arguments`.
pub fn launch_option(arguments: &[String], name: &str) -> Option<String> {
    let flag = format!("--{name}");
    let prefix = format!("--{name}=");
    let mut iterator = arguments.iter();
    while let Some(argument) = iterator.next() {
        if let Some(value) = argument.strip_prefix(&prefix) {
            return Some(value.to_owned());
        }
        if *argument == flag {
            return iterator.next().cloned();
        }
    }
    None
}
