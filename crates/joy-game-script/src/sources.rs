// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A game's shipped Lua sources and their accepted development edits.
//!
//! Games ship every script embedded (browser and release builds never read files) and hot reload edits in native
//! development builds through `GameAssets`. [`ScriptSources`] is the one owner of that set: each source has an import
//! name (`"rules/combat"`), a project-relative path derived from the game's script folder, and its shipped text.
//! An edit replaces the current text only after the host's validation accepts the whole candidate set, so a broken
//! edit never reaches play and the previous revision keeps running.
//!
//! The set holds text only; hosts compile it into a [`ScriptLibrary`] (or individual scripts) whenever they need one.
//! Running hosts that keep a library alive hot reload it with [`ScriptLibrary::replace_source`] using the names
//! [`ScriptSources::apply_edit`] reports.

use std::collections::BTreeMap;

use crate::ScriptEngine;
use crate::ScriptError;
use crate::ScriptLibrary;

/// File extension of game scripts.
pub const SCRIPT_EXTENSION: &str = ".lua";

/// One shipped script: its import name and embedded text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptSource {
    /// The import name scripts use (`require("data/parts")`), which is also the library name and the path below the
    /// script folder without its extension.
    pub name: &'static str,
    /// The embedded source.
    pub shipped: &'static str,
}

impl ScriptSource {
    /// A shipped source named `name`.
    pub const fn new(name: &'static str, shipped: &'static str) -> Self {
        Self {
            name,
            shipped,
        }
    }
}

/// Every Lua source a game ships, with the latest accepted text of each.
#[derive(Clone, Debug)]
pub struct ScriptSources {
    // Project-relative folder of the scripts, ending in `/` (`assets/scripts/`).
    folder: String,
    shipped: Vec<ScriptSource>,

    // The latest text that passed the host's validation, by name; the shipped text until an edit is accepted.
    current: BTreeMap<String, String>,
}

impl ScriptSources {
    /// The shipped `sources`, found under the project-relative `folder` (a trailing `/` is added when missing).
    pub fn new(folder: &str, sources: &[ScriptSource]) -> Self {
        let folder = if folder.is_empty() || folder.ends_with('/') { folder.to_owned() } else { format!("{folder}/") };
        let current = sources.iter().map(|source| (source.name.to_owned(), source.shipped.to_owned())).collect();
        Self {
            folder,
            shipped: sources.to_vec(),
            current,
        }
    }

    /// The project-relative path of the source `name` (`assets/scripts/rules/combat.lua`).
    pub fn path(&self, name: &str) -> String {
        format!("{}{name}{SCRIPT_EXTENSION}", self.folder)
    }

    /// The source a project-relative path names, if it is one of this set's.
    pub fn name_for_path(&self, path: &str) -> Option<&'static str> {
        self.shipped.iter().find(|source| self.path(source.name) == path).map(|source| source.name)
    }

    /// Every source's project-relative path with its shipped text, for `GameAssets::new`.
    pub fn embedded_assets(&self) -> Vec<(String, &'static str)> {
        self.shipped.iter().map(|source| (self.path(source.name), source.shipped)).collect()
    }

    /// Every source's project-relative path, in shipped order.
    pub fn paths(&self) -> Vec<String> {
        self.shipped.iter().map(|source| self.path(source.name)).collect()
    }

    /// The current text of every source, by name.
    pub fn current(&self) -> &BTreeMap<String, String> {
        &self.current
    }

    /// The current text of one source.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.current.get(name).map(String::as_str)
    }

    /// The shipped text of one source.
    pub fn shipped(&self, name: &str) -> Option<&'static str> {
        self.shipped.iter().find(|source| source.name == name).map(|source| source.shipped)
    }

    /// A library on `engine` (with the host's bindings registered) holding every current source, compiling nothing.
    pub fn library(&self, engine: ScriptEngine) -> ScriptLibrary {
        library_from(engine, &self.current)
    }

    /// Hot reload: offer `text` read from `path`. Paths outside the set return `None`. Otherwise `accept` validates
    /// the candidate set (every current source with this one replaced); on success the edit becomes current and the
    /// source's name is returned, and on failure nothing changes and the error is returned with the path for
    /// diagnostics. Text identical to the current revision is accepted without calling `accept`.
    pub fn apply_edit(
        &mut self,
        path: &str,
        text: &str,
        accept: impl FnOnce(&BTreeMap<String, String>) -> Result<(), ScriptError>,
    ) -> Option<Result<&'static str, ScriptEditError>> {
        let name = self.name_for_path(path)?;
        if self.get(name) == Some(text) {
            return Some(Ok(name));
        }
        let mut candidate = self.current.clone();
        candidate.insert(name.to_owned(), text.to_owned());
        Some(match accept(&candidate) {
            Ok(()) => {
                self.current = candidate;
                Ok(name)
            }
            Err(error) => Err(ScriptEditError {
                path: path.to_owned(),
                error,
            }),
        })
    }
}

/// A rejected development edit: the file and why it did not compile or validate.
#[derive(Debug, thiserror::Error)]
#[error("{path}: {error}")]
pub struct ScriptEditError {
    /// The project-relative path of the edited file.
    pub path: String,
    /// The compile or validation failure.
    pub error: ScriptError,
}

/// A library on `engine` holding `sources` (by name), compiling nothing; for hosts validating a candidate set.
pub fn library_from(engine: ScriptEngine, sources: &BTreeMap<String, String>) -> ScriptLibrary {
    let mut library = ScriptLibrary::new(engine);
    for (name, text) in sources {
        library.add_source(name, text);
    }
    library
}
