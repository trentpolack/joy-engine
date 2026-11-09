// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM cook: evaluate a project's FORM assets into one cooked bundle.
//!
//! Each [`CookRequest`] names a project-relative asset and the inputs a game evaluates it with. The cook reads the
//! asset and its modules from the project folder, resolves them through `joy_form::FormEvaluation` (the path
//! `FormAssetLibrary` uses at run time, so keys agree), reuses the [`CookCache`] record for that key or evaluates and
//! caches a new one, and writes every record into a bundle in key order, so identical assets always cook to identical
//! bytes.
//!
//! An asset that fails to resolve or evaluate, has no geometry, or would not decode to exactly what it evaluated to is
//! skipped with its reason rather than failing the build: the game evaluates it live and reports the same diagnostic
//! there. Only an unreadable project file or cache fails the cook.

use std::path::{
    Path,
    PathBuf,
};

use joy_form::{
    CookKey,
    FORM_INSTANCE_EXTENSION,
    FormEvaluation,
    FormInstance,
    GeometryData,
    MODULE_EXTENSION,
    decode_geometry,
    encode_geometry,
    write_cooked_bundle,
};
use serde_json::{
    Map,
    Value as Json,
};

use crate::cook_cache::CookCache;

/// One evaluation to cook: a project-relative `.form` or `.forminst` path and the inputs a game evaluates it with.
#[derive(Clone, Debug, PartialEq)]
pub struct CookRequest {
    /// The asset's path relative to the project folder, with forward slashes (`assets/form/hero.form`).
    pub path: String,
    /// The `.forminst` variant to apply, if any.
    pub variant: Option<String>,
    /// Parameter overrides applied last, as a level component's `parameters` would be.
    pub parameters: Map<String, Json>,
}

impl CookRequest {
    /// `path` at its authored defaults: a script's own defaults, or an instance's overrides.
    pub fn defaults(path: &str) -> Self {
        Self {
            path: path.to_owned(),
            variant: None,
            parameters: Map::new(),
        }
    }

    /// An instance at `path` with its `variant` applied.
    pub fn variant(path: &str, variant: &str) -> Self {
        Self {
            variant: Some(variant.to_owned()),
            ..Self::defaults(path)
        }
    }

    /// A readable label for reports: the path, plus the variant and parameters when present.
    pub fn label(&self) -> String {
        let mut label = self.path.clone();
        if let Some(variant) = &self.variant {
            label.push_str(&format!(" [{variant}]"));
        }
        if !self.parameters.is_empty() {
            label.push_str(&format!(" {}", Json::Object(self.parameters.clone())));
        }
        label
    }
}

/// A request the cook stored in the bundle.
#[derive(Clone, Debug)]
pub struct CookedEntry {
    /// What was cooked.
    pub request: CookRequest,
    /// Its content key.
    pub key: CookKey,
    /// Size of its record in bytes.
    pub record_bytes: usize,
    /// Whether the record came from the cache rather than a fresh evaluation.
    pub from_cache: bool,
}

/// A request the cook left for live evaluation, and why.
#[derive(Clone, Debug)]
pub struct SkippedEntry {
    /// What was skipped.
    pub request: CookRequest,
    /// The resolution or evaluation diagnostic, or why the geometry was not stored.
    pub reason: String,
}

/// The result of one cook.
#[derive(Clone, Debug)]
pub struct CookReport {
    /// The bundle to embed (see `joy_form::CookedForms`).
    pub bundle: Vec<u8>,
    /// Requests stored in the bundle, in request order.
    pub cooked: Vec<CookedEntry>,
    /// Requests left for live evaluation, in request order.
    pub skipped: Vec<SkippedEntry>,
}

impl CookReport {
    /// Requests that were evaluated rather than read from the cache.
    pub fn evaluated(&self) -> usize {
        self.cooked.iter().filter(|entry| !entry.from_cache).count()
    }
}

/// Why the cook could not run: a project file, folder, or the cache could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CookFailure {
    /// A requested path is absolute, leaves the project folder, or uses backslashes.
    #[error("{0} is not a project-relative path")]
    Path(String),
    /// A file or folder could not be read or written.
    #[error("{path}: {message}")]
    Io {
        /// The file or folder.
        path: String,
        /// The operating system's reason.
        message: String,
    },
}

/// Cooks a project's FORM assets. Holds the project folder, an optional cache, and the requests to cook; reading and
/// evaluation happen in [`cook`](Self::cook).
#[derive(Clone, Debug)]
pub struct FormCook {
    project_root: PathBuf,
    cache: Option<CookCache>,
    requests: Vec<CookRequest>,
}

impl FormCook {
    /// A cook for the project in `project_root` (asset and module paths are relative to it), without a cache.
    pub fn new(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            cache: None,
            requests: Vec::new(),
        }
    }

    /// Reuse and store records in `cache`.
    pub fn with_cache(mut self, cache: CookCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Cook `request` as given.
    pub fn add(&mut self, request: CookRequest) -> &mut Self {
        self.requests.push(request);
        self
    }

    /// Cook the asset at `path` as authored: a `.form` at its defaults, or a `.forminst` at its own overrides and with
    /// each of its variants.
    pub fn add_asset(&mut self, path: &str) -> Result<&mut Self, CookFailure> {
        check_relative(path)?;
        self.requests.push(CookRequest::defaults(path));
        if path.ends_with(FORM_INSTANCE_EXTENSION) {
            // A malformed instance cooks its default request only, which reports the document's error when cooked.
            let text = self.read(path)?;
            if let Ok(instance) = FormInstance::read(&text) {
                for variant in instance.variants.iter().flatten() {
                    self.requests.push(CookRequest::variant(path, &variant.name));
                }
            }
        }
        Ok(self)
    }

    /// [`add_asset`](Self::add_asset) for every `.form` and `.forminst` under `folder` (recursively, in sorted path
    /// order) except the paths in `skipped`.
    pub fn add_folder(&mut self, folder: &str, skipped: &[&str]) -> Result<&mut Self, CookFailure> {
        check_relative(folder)?;
        let mut paths = Vec::new();
        collect_assets(&self.project_root, folder.trim_end_matches('/'), &mut paths)?;
        paths.sort();
        for path in paths.iter().filter(|path| !skipped.contains(&path.as_str())) {
            self.add_asset(path)?;
        }
        Ok(self)
    }

    /// The requests added so far, in order.
    pub fn requests(&self) -> &[CookRequest] {
        &self.requests
    }

    /// Cook every request into a bundle.
    pub fn cook(&self) -> Result<CookReport, CookFailure> {
        let mut records = Vec::new();
        let mut cooked = Vec::new();
        let mut skipped = Vec::new();
        for request in &self.requests {
            match self.cook_request(request)? {
                Ok(cooked_record) => {
                    cooked.push(CookedEntry {
                        request: request.clone(),
                        key: cooked_record.key,
                        record_bytes: cooked_record.record.len(),
                        from_cache: cooked_record.from_cache,
                    });
                    records.push((cooked_record.key, cooked_record.record));
                }
                Err(reason) => skipped.push(SkippedEntry {
                    request: request.clone(),
                    reason,
                }),
            }
        }
        Ok(CookReport {
            bundle: write_cooked_bundle(records),
            cooked,
            skipped,
        })
    }

    /// One request's record, or the reason it is left for live evaluation. The outer error is an I/O failure that stops
    /// the cook.
    fn cook_request(&self, request: &CookRequest) -> Result<RequestOutcome, CookFailure> {
        check_relative(&request.path)?;
        let text = self.read(&request.path)?;
        let loader = |module: &str| -> Result<String, String> { self.read(module).map_err(|error| error.to_string()) };
        let evaluation = match FormEvaluation::resolve(
            &request.path,
            &text,
            &loader,
            request.variant.as_deref(),
            &request.parameters,
        ) {
            Ok(evaluation) => evaluation,
            Err(error) => return Ok(Err(error.to_string())),
        };
        let key = evaluation.key();
        if let Some(record) = self.cache.as_ref().and_then(|cache| cache.load(key)) {
            return Ok(Ok(CookedRecord {
                key,
                record,
                from_cache: true,
            }));
        }

        let geometry = match evaluation.evaluate() {
            Ok(geometry) => geometry,
            Err(diagnostic) => return Ok(Err(diagnostic.to_string())),
        };
        let record = match cooked_record(&geometry) {
            Ok(record) => record,
            Err(reason) => return Ok(Err(reason)),
        };
        if let Some(cache) = &self.cache {
            cache.store(key, &record).map_err(|error| io_failure(&cache.record_path(key), &error))?;
        }
        Ok(Ok(CookedRecord {
            key,
            record,
            from_cache: false,
        }))
    }

    /// The text of the project file at `path`.
    fn read(&self, path: &str) -> Result<String, CookFailure> {
        let file = self.project_root.join(path);
        std::fs::read_to_string(&file).map_err(|error| io_failure(&file, &error))
    }
}

/// A request's cooked record, keyed, and whether the cache supplied it.
struct CookedRecord {
    key: CookKey,
    record: Vec<u8>,
    from_cache: bool,
}

/// A request's record, or why it is left for live evaluation.
type RequestOutcome = Result<CookedRecord, String>;

/// `geometry`'s record, when it has something to draw and decodes to exactly the evaluated geometry (outputs aside,
/// which records leave out). Values JSON cannot spell, such as a non-finite attribute, fail the second check.
fn cooked_record(geometry: &GeometryData) -> Result<Vec<u8>, String> {
    if geometry.positions.is_empty() {
        return Err("the evaluation has no geometry".into());
    }
    let record = encode_geometry(geometry);
    let expected = GeometryData {
        outputs: None,
        ..geometry.clone()
    };
    match decode_geometry(&record) {
        Ok(decoded) if decoded == expected => Ok(record),
        Ok(_) => Err("the geometry does not survive cooking exactly".into()),
        Err(error) => Err(error.to_string()),
    }
}

/// Refuse absolute paths, parent segments, and backslashes, so a request can only name files inside the project.
fn check_relative(path: &str) -> Result<(), CookFailure> {
    let inside = !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && !path.contains(':')
        && path.split('/').all(|segment| segment != "..");
    if inside { Ok(()) } else { Err(CookFailure::Path(path.to_owned())) }
}

/// Every FORM script and instance under `folder` (project-relative), recursively.
fn collect_assets(root: &Path, folder: &str, paths: &mut Vec<String>) -> Result<(), CookFailure> {
    let directory = root.join(folder);
    let entries = std::fs::read_dir(&directory).map_err(|error| io_failure(&directory, &error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_failure(&directory, &error))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{folder}/{name}");
        if entry.path().is_dir() {
            collect_assets(root, &path, paths)?;
        } else if name.ends_with(MODULE_EXTENSION) || name.ends_with(FORM_INSTANCE_EXTENSION) {
            paths.push(path);
        }
    }
    Ok(())
}

fn io_failure(path: &Path, error: &std::io::Error) -> CookFailure {
    CookFailure::Io {
        path: path.display().to_string(),
        message: error.to_string(),
    }
}
