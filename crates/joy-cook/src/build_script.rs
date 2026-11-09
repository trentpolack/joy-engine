// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The cook as a game's build script: decide whether this build cooks, cook what the game names, and write the bundle
//! `joy_engine::cooked_forms!` embeds.
//!
//! Release builds and every browser build cook; native development builds (debug assertions on, not WebAssembly) write
//! an empty bundle instead, so FORM evaluates live and `GameAssets` hot reloads it. The decision follows the same
//! `debug_assertions` and target the game's `source_root!` checks, so a build either hot reloads FORM or embeds cooked
//! geometry, never a stale mix. [`COOK_MODE_VARIABLE`] overrides it (`always` cooks a native development build to try
//! the cooked path; `never` skips cooking). Records are cached under [`CACHE_DIRECTORY_VARIABLE`] when set, otherwise in
//! `joy-cook/` inside Cargo's target directory, which every repository already ignores.
//!
//! Cargo reruns the script when a named folder or file changes, so editing a FORM asset re-cooks only that asset (the
//! cache supplies the rest).

use std::path::{
    Path,
    PathBuf,
};

use joy_form::{
    COOKED_FORMS_FILE,
    write_cooked_bundle,
};

use crate::cook_cache::CookCache;
use crate::form_cook::{
    CookFailure,
    CookReport,
    CookRequest,
    FormCook,
};

/// Environment variable that overrides whether a build cooks: `auto` (the default), `always`, or `never`.
pub const COOK_MODE_VARIABLE: &str = "JOY_COOK_FORMS";

/// Environment variable naming the cook cache folder; by default it lives in Cargo's target directory.
pub const CACHE_DIRECTORY_VARIABLE: &str = "JOY_COOK_CACHE_DIR";

/// Marker file Cargo writes at the root of every target directory; the default cache sits beside it.
const TARGET_DIRECTORY_MARKER: &str = "CACHEDIR.TAG";

/// The calling package's build script, which Cargo should rerun this one for when nothing else is watched.
const BUILD_SCRIPT: &str = "build.rs";

/// Folder of the default cache inside the target directory.
const DEFAULT_CACHE_FOLDER: &str = "joy-cook/forms";

/// Whether a build cooks its FORM assets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CookMode {
    /// Cook release and browser builds; native development builds evaluate live.
    Auto,
    /// Cook every build, including native development builds.
    Always,
    /// Never cook; every build evaluates live.
    Never,
}

impl CookMode {
    /// The mode named by `value` ([`COOK_MODE_VARIABLE`]'s spellings); unknown spellings are `None`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    /// Whether a build with `debug_assertions` for `target_arch` cooks.
    pub fn cooks(self, debug_assertions: bool, target_arch: &str) -> bool {
        match self {
            Self::Auto => !debug_assertions || target_arch == "wasm32",
            Self::Always => true,
            Self::Never => false,
        }
    }
}

/// A game build script's cook: the folders, files, and requests it names. Build it with the chained methods and finish
/// with [`run`](Self::run).
#[derive(Clone, Debug, Default)]
#[must_use = "call run() to cook and write the bundle"]
pub struct BuildCook {
    folders: Vec<String>,
    files: Vec<String>,
    skipped: Vec<String>,
    requests: Vec<CookRequest>,
}

impl BuildCook {
    /// A cook that names nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cook every `.form` and `.forminst` under the project-relative `folder` as authored (see
    /// [`FormCook::add_asset`]).
    pub fn folder(mut self, folder: &str) -> Self {
        self.folders.push(folder.to_owned());
        self
    }

    /// Cook the project-relative asset at `path` as authored.
    pub fn file(mut self, path: &str) -> Self {
        self.files.push(path.to_owned());
        self
    }

    /// Leave `path` out of the folders, for scripts the game only evaluates with run-time inputs (a default copy
    /// would never be read).
    pub fn skip(mut self, path: &str) -> Self {
        self.skipped.push(path.to_owned());
        self
    }

    /// Cook `request` too, for parameter sets the game evaluates that its assets do not author.
    pub fn request(mut self, request: CookRequest) -> Self {
        self.requests.push(request);
        self
    }

    /// Cook (or not; see the [module documentation](self)) and write the bundle to `OUT_DIR`. A build script cannot
    /// recover from a missing project file or an unwritable output, so those stop the build with a message; assets that
    /// fail to evaluate are reported as Cargo warnings and evaluate live in the game.
    pub fn run(self) {
        if let Err(message) = self.run_in_environment() {
            eprintln!("joy-cook: {message}");
            std::process::exit(1);
        }
    }

    fn run_in_environment(&self) -> Result<(), String> {
        let out_directory = PathBuf::from(required_variable("OUT_DIR")?);
        let project_root = PathBuf::from(required_variable("CARGO_MANIFEST_DIR")?);
        println!("cargo::rerun-if-changed={BUILD_SCRIPT}");
        println!("cargo::rerun-if-env-changed={COOK_MODE_VARIABLE}");
        println!("cargo::rerun-if-env-changed={CACHE_DIRECTORY_VARIABLE}");

        let mode_text = std::env::var(COOK_MODE_VARIABLE).unwrap_or_default();
        let mode = CookMode::parse(&mode_text)
            .ok_or_else(|| format!("{COOK_MODE_VARIABLE}={mode_text} is not auto, always, or never"))?;
        let debug_assertions = std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some();
        let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();

        let bundle = if mode.cooks(debug_assertions, &target_arch) {
            // Only cooking builds depend on the assets; live builds re-read them through hot reload instead.
            for path in self.folders.iter().chain(&self.files) {
                println!("cargo::rerun-if-changed={path}");
            }
            let cache = CookCache::new(cache_directory(&out_directory));
            let report = self.cook(&project_root, cache).map_err(|failure| failure.to_string())?;
            for skipped in &report.skipped {
                // Cargo reads one directive per line, so multi-line diagnostics fold onto one.
                let reason = skipped.reason.replace('\n', " ");
                println!(
                    "cargo::warning=FORM asset {} was not cooked and evaluates at run time: {reason}",
                    skipped.request.label()
                );
            }
            report.bundle
        } else {
            write_cooked_bundle([])
        };
        let output = out_directory.join(COOKED_FORMS_FILE);
        std::fs::write(&output, bundle).map_err(|error| format!("{}: {error}", output.display()))
    }

    /// Cook the named assets of the project in `project_root` with `cache`.
    fn cook(&self, project_root: &Path, cache: CookCache) -> Result<CookReport, CookFailure> {
        let mut cook = FormCook::new(project_root).with_cache(cache);
        let skipped: Vec<&str> = self.skipped.iter().map(String::as_str).collect();
        for folder in &self.folders {
            cook.add_folder(folder, &skipped)?;
        }
        for file in &self.files {
            cook.add_asset(file)?;
        }
        for request in &self.requests {
            cook.add(request.clone());
        }
        cook.cook()
    }
}

fn required_variable(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("{name} is not set; BuildCook::run belongs in a build script"))
}

/// [`CACHE_DIRECTORY_VARIABLE`] when set; otherwise `joy-cook/forms` in the target directory holding `out_directory`
/// (found by Cargo's marker file), so every profile and target shares one cache; otherwise a folder in `out_directory`.
fn cache_directory(out_directory: &Path) -> PathBuf {
    if let Some(directory) = std::env::var_os(CACHE_DIRECTORY_VARIABLE).filter(|value| !value.is_empty()) {
        return PathBuf::from(directory);
    }
    out_directory
        .ancestors()
        .find(|ancestor| ancestor.join(TARGET_DIRECTORY_MARKER).is_file())
        .map_or_else(|| out_directory.join(DEFAULT_CACHE_FOLDER), |target| target.join(DEFAULT_CACHE_FOLDER))
}
