// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Content keys for cooked FORM geometry.
//!
//! A [`CookKey`] hashes everything one evaluation reads: the FORM evaluator version, the main script's source, every
//! module it uses (path and source, in load order), the layered parameter overrides, and the material palette. Equal
//! keys mean equal inputs, so cooked geometry can stand in for an evaluation, and an edit to a script, a module, a
//! parameter, or the evaluator misses the cooked copy instead of reusing stale geometry.
//!
//! The hash is FNV-1a over length-prefixed fields, 128 bits wide: stable across platforms, Rust versions, and runs
//! (unlike `std`'s randomized hashers), with no dependency. It guards against accidental collisions between a game's own
//! assets, not adversarial input; cooked bundles come from the game's own build. JSON inputs hash canonically (object
//! keys sorted), so parameter order never decides whether a cooked copy is found.

use serde_json::Value as Json;

/// The FORM evaluator build cooked geometry was produced with: the crate version plus a fingerprint of the crate's
/// library source (computed by its build script), so editing the evaluator invalidates every cooked copy and cache
/// entry without a hand-maintained version number.
pub const FORM_EVALUATOR_VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "+", env!("JOY_FORM_SOURCE_FINGERPRINT"));

/// FNV-1a 128-bit offset basis and prime.
const FNV_OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
const FNV_PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

// Type tags for canonical JSON, so `"1"` and `1` (or `[]` and `{}`) never hash alike.
const TAG_NULL: u8 = 0;
const TAG_FALSE: u8 = 1;
const TAG_TRUE: u8 = 2;
const TAG_NUMBER: u8 = 3;
const TAG_STRING: u8 = 4;
const TAG_ARRAY: u8 = 5;
const TAG_OBJECT: u8 = 6;
const TAG_ABSENT: u8 = 7;

/// Identifies one FORM evaluation's inputs; see the [module documentation](self).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CookKey(u128);

impl CookKey {
    /// Length of [`to_bytes`](Self::to_bytes).
    pub const BYTES: usize = 16;

    /// The key's little-endian bytes, as cooked bundles store it.
    pub fn to_bytes(self) -> [u8; Self::BYTES] {
        self.0.to_le_bytes()
    }

    /// The key stored as `bytes` by [`to_bytes`](Self::to_bytes).
    pub fn from_bytes(bytes: [u8; Self::BYTES]) -> Self {
        Self(u128::from_le_bytes(bytes))
    }

    /// Thirty-two lowercase hexadecimal digits: the key's file name in cook caches.
    pub fn to_hex(self) -> String {
        format!("{:032x}", self.0)
    }
}

impl std::fmt::Display for CookKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:032x}", self.0)
    }
}

/// The source half of a key: a script's main source and every module it loaded, hashed once at parse time.
///
/// The main script's own path is left out, so the same text parsed with [`CompiledScript::parse`](crate::CompiledScript::parse)
/// (games embedding a script) and with a path and loader (levels and the cook) finds the same cooked copy. Module paths
/// are part of it, since they decide which `use` declaration reached which text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SourceFingerprint(u128);

impl SourceFingerprint {
    /// Hash `main_source` and each `(path, source)` module in load order.
    pub(crate) fn new<'a>(main_source: &str, modules: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut hasher = ContentHasher::new();
        hasher.field(main_source.as_bytes());
        for (path, source) in modules {
            hasher.field(path.as_bytes());
            hasher.field(source.as_bytes());
        }
        Self(hasher.finish())
    }
}

/// The key of evaluating a script with `sources` under `overrides` and `materials` with this evaluator build.
pub(crate) fn evaluation_key(sources: SourceFingerprint, overrides: &Json, materials: Option<&Json>) -> CookKey {
    key_for_version(FORM_EVALUATOR_VERSION, sources, overrides, materials)
}

/// [`evaluation_key`] for an explicit evaluator `version`; tests use it to show a new evaluator misses old keys.
fn key_for_version(version: &str, sources: SourceFingerprint, overrides: &Json, materials: Option<&Json>) -> CookKey {
    let mut hasher = ContentHasher::new();
    hasher.field(version.as_bytes());
    hasher.field(&sources.0.to_le_bytes());
    hasher.json(overrides);
    match materials {
        Some(palette) => hasher.json(palette),
        None => hasher.tag(TAG_ABSENT),
    }
    CookKey(hasher.finish())
}

/// FNV-1a 128 over length-prefixed fields and canonical JSON.
struct ContentHasher {
    state: u128,
}

impl ContentHasher {
    fn new() -> Self {
        Self {
            state: FNV_OFFSET,
        }
    }

    /// Hash `bytes` behind their length, so adjacent fields cannot trade bytes and collide.
    fn field(&mut self, bytes: &[u8]) {
        self.raw(&(bytes.len() as u64).to_le_bytes());
        self.raw(bytes);
    }

    fn tag(&mut self, tag: u8) {
        self.raw(&[tag]);
    }

    /// Hash `value` canonically: objects in sorted key order, numbers by their JSON spelling.
    fn json(&mut self, value: &Json) {
        match value {
            Json::Null => self.tag(TAG_NULL),
            Json::Bool(false) => self.tag(TAG_FALSE),
            Json::Bool(true) => self.tag(TAG_TRUE),
            Json::Number(number) => {
                self.tag(TAG_NUMBER);
                self.field(number.to_string().as_bytes());
            }
            Json::String(text) => {
                self.tag(TAG_STRING);
                self.field(text.as_bytes());
            }
            Json::Array(items) => {
                self.tag(TAG_ARRAY);
                self.raw(&(items.len() as u64).to_le_bytes());
                for item in items {
                    self.json(item);
                }
            }
            Json::Object(entries) => {
                self.tag(TAG_OBJECT);
                self.raw(&(entries.len() as u64).to_le_bytes());
                let mut names: Vec<&String> = entries.keys().collect();
                names.sort();
                for name in names {
                    self.field(name.as_bytes());
                    self.json(&entries[name]);
                }
            }
        }
    }

    fn raw(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u128::from(*byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    fn finish(self) -> u128 {
        self.state
    }
}

#[cfg(test)]
#[path = "cook_key.test.rs"]
mod tests;
