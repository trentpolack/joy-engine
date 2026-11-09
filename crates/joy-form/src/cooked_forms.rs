// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Cooked FORM bundles: the geometry a game's build evaluated ahead of time, indexed by [`CookKey`], so release and
//! browser builds load meshes instead of evaluating FORM.
//!
//! A game's build script (`joy-cook`) writes one bundle into Cargo's `OUT_DIR` and the game embeds it with
//! [`cooked_forms!`](crate::cooked_forms). Every lookup goes through [`CookedForms::evaluate`]: a key the build cooked
//! decodes its record, and anything else (inputs chosen at run time, edits picked up by hot reload, an empty bundle in
//! native development builds) evaluates live, so cooked and live results can never disagree about which inputs they
//! describe. Cooked results omit named outputs; see [`encode_geometry`](crate::encode_geometry).
//!
//! Layout (little-endian): the magic `JOYFCOOK`, the bundle version (`u32`), the evaluator version's byte length (`u32`)
//! and UTF-8 text, the entry count (`u32`), then each entry's key (16 bytes), record length (`u32`), and record.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use serde_json::Value as Json;

use crate::cook_key::{
    CookKey,
    FORM_EVALUATOR_VERSION,
};
use crate::cooked_geometry::{
    ByteReader,
    ByteWriter,
    CookError,
    decode_geometry,
};
use crate::evaluation::FormEvaluation;
use crate::geometry_data::GeometryData;
use crate::{
    CompiledScript,
    Diagnostic,
};

/// File name of the bundle a game's build script writes to `OUT_DIR` and [`cooked_forms!`](crate::cooked_forms) embeds.
pub const COOKED_FORMS_FILE: &str = crate::__cooked_forms_file!();

/// First bytes of every cooked bundle.
const BUNDLE_MAGIC: &[u8; 8] = b"JOYFCOOK";

/// Current bundle layout.
const BUNDLE_VERSION: u32 = 1;

/// A game's cooked FORM geometry, read-only once built; cheap to clone and share between owners.
///
/// The empty table (`CookedForms::default()`) cooks nothing, so every evaluation runs live; native development builds
/// embed an empty bundle, which keeps FORM edits hot reloading.
#[derive(Clone, Default)]
pub struct CookedForms {
    bundle: Option<Arc<Bundle>>,
}

/// A bundle's bytes (borrowed from the binary when embedded) and each record's byte range.
struct Bundle {
    bytes: Cow<'static, [u8]>,
    records: HashMap<CookKey, Range<usize>>,
}

impl CookedForms {
    /// Read a bundle written by [`write_cooked_bundle`], copying `bytes`. Fails for foreign or damaged bytes, another
    /// bundle layout, or geometry cooked by a different evaluator build (which could disagree with live evaluation).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CookError> {
        let records = index_bundle(bytes)?;
        Ok(Self::with_records(Cow::Owned(bytes.to_vec()), records))
    }

    /// [`from_bytes`](Self::from_bytes) for bytes compiled into the binary, which are borrowed rather than copied.
    pub fn from_static(bytes: &'static [u8]) -> Result<Self, CookError> {
        let records = index_bundle(bytes)?;
        Ok(Self::with_records(Cow::Borrowed(bytes), records))
    }

    /// The bundle a game embeds with [`cooked_forms!`](crate::cooked_forms). A bundle that cannot be read is logged
    /// and treated as empty, so the game still runs, evaluating FORM live.
    pub fn embedded(bytes: &'static [u8]) -> Self {
        match Self::from_static(bytes) {
            Ok(cooked) => cooked,
            Err(error) => {
                log::warn!("Cooked FORM geometry is unusable ({error}); FORM assets will evaluate at run time.");
                Self::default()
            }
        }
    }

    fn with_records(bytes: Cow<'static, [u8]>, records: HashMap<CookKey, Range<usize>>) -> Self {
        let bundle = (!records.is_empty()).then(|| {
            Arc::new(Bundle {
                bytes,
                records,
            })
        });
        Self {
            bundle,
        }
    }

    /// Number of cooked evaluations.
    pub fn len(&self) -> usize {
        self.bundle.as_ref().map_or(0, |bundle| bundle.records.len())
    }

    /// Whether nothing is cooked, so every evaluation runs live.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Whether `key` was cooked.
    pub fn contains(&self, key: CookKey) -> bool {
        self.bundle.as_ref().is_some_and(|bundle| bundle.records.contains_key(&key))
    }

    /// The cooked geometry for `key`: `None` when it was not cooked, an error when its record is damaged.
    pub fn geometry(&self, key: CookKey) -> Option<Result<GeometryData, CookError>> {
        let bundle = self.bundle.as_ref()?;
        let range = bundle.records.get(&key)?;
        Some(decode_geometry(&bundle.bytes[range.clone()]))
    }

    /// The geometry of `evaluation`: its cooked copy when the build cooked these exact inputs, otherwise a live
    /// evaluation (a damaged record is logged and evaluated live too).
    pub fn evaluate(&self, evaluation: &FormEvaluation) -> Result<GeometryData, Diagnostic> {
        if self.is_empty() {
            return evaluation.evaluate();
        }
        self.cooked_or(evaluation.key(), || evaluation.evaluate())
    }

    /// [`evaluate`](Self::evaluate) for a parsed `script` with `overrides` and `materials`.
    pub fn evaluate_script(
        &self,
        script: &CompiledScript,
        overrides: &Json,
        materials: Option<&Json>,
    ) -> Result<GeometryData, Diagnostic> {
        if self.is_empty() {
            return script.evaluate(overrides, materials);
        }
        self.cooked_or(script.cook_key(overrides, materials), || script.evaluate(overrides, materials))
    }

    /// Parse `source` (which may not `use` modules) and evaluate it with cooked lookup: [`crate::compile`] for games
    /// that embed scripts.
    pub fn compile(
        &self,
        source: &str,
        overrides: &Json,
        materials: Option<&Json>,
    ) -> Result<GeometryData, Diagnostic> {
        self.evaluate_script(&CompiledScript::parse(source)?, overrides, materials)
    }

    /// The cooked geometry for `key`, or `live()` when it was not cooked or its record is damaged.
    fn cooked_or(
        &self,
        key: CookKey,
        live: impl FnOnce() -> Result<GeometryData, Diagnostic>,
    ) -> Result<GeometryData, Diagnostic> {
        match self.geometry(key) {
            Some(Ok(geometry)) => Ok(geometry),
            Some(Err(error)) => {
                log::warn!("Cooked FORM geometry {key} is damaged ({error}); evaluating it at run time.");
                live()
            }
            None => live(),
        }
    }
}

/// Validate a bundle's header and index its records by key; records decode on lookup.
fn index_bundle(bytes: &[u8]) -> Result<HashMap<CookKey, Range<usize>>, CookError> {
    let mut reader = ByteReader::new(bytes);
    if reader.take(BUNDLE_MAGIC.len())? != BUNDLE_MAGIC {
        return Err(CookError::NotCooked);
    }
    let version = reader.u32()?;
    if version != BUNDLE_VERSION {
        return Err(CookError::UnsupportedVersion {
            found: version,
            expected: BUNDLE_VERSION,
        });
    }
    let length = reader.u32()? as usize;
    let evaluator = String::from_utf8_lossy(reader.take(length)?).into_owned();
    if evaluator != FORM_EVALUATOR_VERSION {
        return Err(CookError::EvaluatorMismatch {
            found: evaluator,
            expected: FORM_EVALUATOR_VERSION.to_owned(),
        });
    }

    // Index each record by key; records decode on lookup.
    let count = reader.u32()? as usize;
    let mut records = HashMap::with_capacity(count.min(bytes.len()));
    for _ in 0..count {
        let key = CookKey::from_bytes(reader.array()?);
        let length = reader.u32()? as usize;
        let start = reader.offset();
        reader.take(length)?;
        records.insert(key, start..(start + length));
    }
    if !reader.is_finished() {
        return Err(CookError::Malformed("bytes follow the last entry".into()));
    }
    Ok(records)
}

impl std::fmt::Debug for CookedForms {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CookedForms").field("records", &self.len()).finish()
    }
}

/// Write a bundle holding `records` (each a key and a record from [`encode_geometry`](crate::encode_geometry)) for this
/// evaluator build, in key order so identical inputs always produce identical bytes.
pub fn write_cooked_bundle(records: impl IntoIterator<Item = (CookKey, Vec<u8>)>) -> Vec<u8> {
    let mut records: Vec<(CookKey, Vec<u8>)> = records.into_iter().collect();
    records.sort_by_key(|(key, _)| *key);
    records.dedup_by_key(|(key, _)| *key);

    let mut writer = ByteWriter::default();
    writer.bytes(BUNDLE_MAGIC);
    writer.u32(BUNDLE_VERSION);
    writer.u32(FORM_EVALUATOR_VERSION.len() as u32);
    writer.bytes(FORM_EVALUATOR_VERSION.as_bytes());
    writer.u32(records.len() as u32);
    for (key, record) in &records {
        writer.bytes(&key.to_bytes());
        writer.u32(record.len() as u32);
        writer.bytes(record);
    }
    writer.finish()
}

/// The bundle file name as a literal, so [`cooked_forms!`](crate::cooked_forms) can `concat!` it.
#[doc(hidden)]
#[macro_export]
macro_rules! __cooked_forms_file {
    () => {
        "joy-cooked-forms.bin"
    };
}

/// The calling game's cooked FORM geometry: the bundle its build script wrote to `OUT_DIR` (see `joy-cook`), as a
/// [`CookedForms`](crate::CookedForms). Native development builds embed an empty bundle, so FORM evaluates live and
/// hot reloads; release and browser builds embed the cooked geometry.
///
/// ```ignore
/// let cooked = joy_engine::cooked_forms!();
/// let hero = compile_cooked_form_mesh(&cooked, HERO_SOURCE, &json!({}), None, ShadingModel::Lit)?;
/// ```
///
/// Build the table once and keep or clone it; each expansion indexes the embedded bundle again.
#[macro_export]
macro_rules! cooked_forms {
    () => {
        $crate::CookedForms::embedded(include_bytes!(concat!(env!("OUT_DIR"), "/", $crate::__cooked_forms_file!())))
    };
}
