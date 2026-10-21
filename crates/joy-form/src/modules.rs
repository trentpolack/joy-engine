// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM modules: `use "path.form" as name` loads another script's functions, enums, and constants.
//!
//! Modules are resolved while parsing, so evaluation never reads files and stays deterministic for identical inputs.
//! A host supplies a [`ModuleLoader`] that maps a normalized path to source text (a project folder, an embedded asset
//! table, or an in-memory map). Paths are relative to the importing script's folder and may not leave the loader's
//! root.

use crate::{
    Diagnostic,
    ast::{
        Statement,
        StmtKind,
        Token,
    },
    parser,
};

/// File extension every module path must name.
pub const MODULE_EXTENSION: &str = ".form";

/// Most modules one compiled script may load, counting each file once.
const MODULE_LIMIT: usize = 32;

/// Deepest chain of `use` declarations, counting the main script.
const MODULE_DEPTH_LIMIT: usize = 16;

/// Supplies the source of modules a FORM script names with `use`.
///
/// `path` is normalized and relative to the loader's root: forward slashes, no `.` or `..` segments, ending in
/// `.form`. Return the module's source text, or a short reason the module could not be loaded. Any
/// `Fn(&str) -> Result<String, String>` closure is a loader.
pub trait ModuleLoader {
    /// Load the source of the module at `path`.
    fn load(&self, path: &str) -> Result<String, String>;
}

impl<F> ModuleLoader for F
where
    F: Fn(&str) -> Result<String, String>,
{
    fn load(&self, path: &str) -> Result<String, String> {
        self(path)
    }
}

/// A parsed script and every module it uses; module 0 is the main script.
#[derive(Debug)]
pub(crate) struct Program {
    pub modules: Vec<ProgramModule>,
}

/// One parsed source file in a [`Program`].
#[derive(Debug)]
pub(crate) struct ProgramModule {
    /// The normalized path the module was loaded from; `None` for a main script parsed from text alone.
    pub path: Option<String>,
    pub statements: Vec<Statement>,
}

impl ProgramModule {
    /// How diagnostics name this module.
    pub fn display_path(&self) -> &str {
        self.path.as_deref().unwrap_or("script")
    }
}

/// Parse `source` and, through `loader`, every module it uses.
///
/// `path` locates the main script for relative `use` paths; `None` resolves them from the loader's root. Without a
/// loader any `use` declaration is a diagnostic. Module failures are reported at the importing `use` declaration with
/// the module path, line, and column in the message.
pub(crate) fn build(
    source: &str,
    path: Option<&str>,
    loader: Option<&dyn ModuleLoader>,
) -> Result<Program, Diagnostic> {
    let main_path = path.map(normalize_main_path).transpose()?;
    let mut builder = ProgramBuilder {
        loader,
        modules: Vec::new(),
        loading: Vec::new(),
    };
    let statements = parser::parse(source)?;
    builder.modules.push(ProgramModule {
        path: main_path.clone(),
        statements: Vec::new(),
    });
    builder.loading.push(main_path.clone().unwrap_or_default());
    let statements = builder.resolve(statements, main_path.as_deref())?;
    builder.modules[0].statements = statements;
    Ok(Program {
        modules: builder.modules,
    })
}

struct ProgramBuilder<'a> {
    loader: Option<&'a dyn ModuleLoader>,
    modules: Vec<ProgramModule>,
    // Paths of the modules currently being resolved, outermost first, so cycles are reported where they close.
    loading: Vec<String>,
}

impl ProgramBuilder<'_> {
    /// Load the modules `statements` use and record their indices in each `use` declaration.
    fn resolve(
        &mut self,
        mut statements: Vec<Statement>,
        importer: Option<&str>,
    ) -> Result<Vec<Statement>, Diagnostic> {
        for statement in &mut statements {
            let StmtKind::Use {
                path,
                module,
                ..
            } = &mut statement.kind
            else {
                continue;
            };
            *module = self.load(path, importer, &statement.token)?;
        }
        Ok(statements)
    }

    fn load(&mut self, requested: &str, importer: Option<&str>, token: &Token) -> Result<usize, Diagnostic> {
        let Some(loader) = self.loader else {
            return Err(Diagnostic::at(
                "Modules need a host that loads files; open this script from a project or library folder to use modules.",
                token,
            ));
        };
        let path = resolve_path(requested, importer).map_err(|message| Diagnostic::at(message, token))?;
        if self.loading.contains(&path) {
            return Err(Diagnostic::at(
                format!("Module '{path}' uses itself through a cycle of 'use' declarations."),
                token,
            ));
        }
        if let Some(index) = self.modules.iter().position(|module| module.path.as_deref() == Some(path.as_str())) {
            return Ok(index);
        }
        if self.modules.len() >= MODULE_LIMIT {
            return Err(Diagnostic::at(format!("Module limit exceeded ({MODULE_LIMIT} files per script)."), token));
        }
        if self.loading.len() >= MODULE_DEPTH_LIMIT {
            return Err(Diagnostic::at(format!("Module nesting limit exceeded ({MODULE_DEPTH_LIMIT} levels)."), token));
        }

        let source = loader
            .load(&path)
            .map_err(|reason| Diagnostic::at(format!("Could not load module '{path}': {reason}"), token))?;
        let in_module = |error: Diagnostic| {
            Diagnostic::at(format!("{path} ({}:{}): {}", error.line, error.column, error.message), token)
        };
        let statements = parser::parse(&source).map_err(in_module)?;
        validate_module(&statements).map_err(in_module)?;

        // Reserve the index first so modules that share this dependency find it, then resolve its own uses.
        let index = self.modules.len();
        self.modules.push(ProgramModule {
            path: Some(path.clone()),
            statements: Vec::new(),
        });
        self.loading.push(path.clone());
        let resolved = self.resolve(statements, Some(&path));
        self.loading.pop();
        self.modules[index].statements = resolved.map_err(in_module)?;
        Ok(index)
    }
}

/// Modules contribute declarations only; anything that would produce geometry belongs to the script that uses them.
fn validate_module(statements: &[Statement]) -> Result<(), Diagnostic> {
    for statement in statements {
        if !matches!(
            statement.kind,
            StmtKind::Function(_) | StmtKind::Enum { .. } | StmtKind::Use { .. } | StmtKind::Let(..)
        ) {
            return Err(Diagnostic::at(
                "Modules may only declare fn, enum, let, and use; move other statements into the script that uses this module.",
                &statement.token,
            ));
        }
    }
    Ok(())
}

/// Resolve `requested` against the folder of `importer` (or the loader root) into a normalized module path.
pub(crate) fn resolve_path(requested: &str, importer: Option<&str>) -> Result<String, String> {
    if requested.contains('\\') || requested.starts_with('/') || requested.contains(':') {
        return Err(format!("Module path '{requested}' must be relative and use forward slashes."));
    }
    if !requested.ends_with(MODULE_EXTENSION) {
        return Err(format!("Module path '{requested}' must name a {MODULE_EXTENSION} file."));
    }
    let mut segments: Vec<&str> = match importer {
        Some(importer) => {
            let mut folder: Vec<&str> = importer.split('/').collect();
            folder.pop();
            folder
        }
        None => Vec::new(),
    };
    for segment in requested.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err(format!("Module path '{requested}' leaves the project folder."));
                }
            }
            other => segments.push(other),
        }
    }
    Ok(segments.join("/"))
}

/// Normalize the main script's own path the same way module paths are normalized.
fn normalize_main_path(path: &str) -> Result<String, Diagnostic> {
    let normalized = path.replace('\\', "/");
    let mut segments: Vec<&str> = Vec::new();
    for segment in normalized.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    if segments.is_empty() {
        return Err(Diagnostic {
            message: "Script path must name a file.".into(),
            line: 1,
            column: 1,
        });
    }
    Ok(segments.join("/"))
}

#[cfg(test)]
#[path = "modules.test.rs"]
mod tests;
