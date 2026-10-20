// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Test-only helpers shared by Joy Engine crates and the games built on it: golden files that record deterministic
//! behavior.
//!
//! A golden file is a JSON recording of what the code produced when it was last recorded on purpose: the inputs a test
//! replays and the outputs it observed. Tests rebuild the recording from the current code and check it with
//! [`GoldenFile::assert_matches`] (or [`GoldenFile::assert_section_matches`] for one part of a shared file). When
//! behavior changes on purpose, re-record and review the fixture diff like any other change:
//!
//! ```text
//! JOY_UPDATE_FIXTURES=1 cargo test -p <package>
//! ```
//!
//! Engine crates take this crate as a dev-dependency; games reach it as `joy_engine::testing` by enabling the
//! facade's `test-support` feature in their `[dev-dependencies]`.

mod compare;
mod golden;

pub use compare::{
    Mismatch,
    Tolerance,
    compare,
};
pub use golden::{
    GoldenFile,
    UPDATE_FIXTURES_VARIABLE,
    rerecord,
    update_requested,
};

/// A single-precision value as its shortest round-tripping decimal (`0.3`, not `0.30000001192092896`), so recordings of
/// `f32` data stay readable.
pub fn json_f32(value: f32) -> serde_json::Value {
    let shortest: f64 = value.to_string().parse().expect("f32 text parses as f64");
    serde_json::Value::from(shortest)
}

/// Single-precision values as an array of [`json_f32`] numbers.
pub fn json_f32s(values: &[f32]) -> serde_json::Value {
    serde_json::Value::Array(values.iter().copied().map(json_f32).collect())
}
