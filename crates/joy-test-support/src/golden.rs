// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Golden files: JSON recordings of deterministic behavior that tests check against and re-record on purpose.
//!
//! A test opens its golden file, rebuilds the recording from the current code (usually by cloning the recording and
//! replacing each case's outputs), and asserts that the two match. Re-recording rewrites the file from the code
//! instead; it keeps every recorded number that still agrees within the tolerance, so the diff shows only the values
//! that really changed, and it keeps the file's layout (compact or indented, with or without a final newline) and
//! JavaScript's number spelling (`0.000001`, `1e-7`, and `2` for an integral value), in which the existing recordings
//! were written.

use std::path::{
    Path,
    PathBuf,
};
use std::sync::Mutex;

use serde_json::Value;

use crate::compare::{
    Tolerance,
    compare,
    number,
};

/// Environment variable that re-records golden files instead of checking them: `JOY_UPDATE_FIXTURES=1 cargo test`.
pub const UPDATE_FIXTURES_VARIABLE: &str = "JOY_UPDATE_FIXTURES";

/// Mismatches printed when a check fails.
const REPORTED_MISMATCHES: usize = 25;

/// Serializes golden-file reads and re-recording within a test process, since several tests may share one file.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Whether this test run re-records golden files (`JOY_UPDATE_FIXTURES` set to anything but empty or `0`).
pub fn update_requested() -> bool {
    std::env::var_os(UPDATE_FIXTURES_VARIABLE).is_some_and(|value| !value.is_empty() && value != "0")
}

/// Open the golden file `name` in the calling package's `tests/fixtures/` directory.
#[macro_export]
macro_rules! golden_file {
    ($name:expr) => {
        $crate::GoldenFile::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/", $name))
    };
}

/// A golden file's text layout, kept when it is re-recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    /// Spaces per indentation level; zero writes everything on one line.
    indent: usize,
    final_newline: bool,
}

/// A parsed golden file. Test helpers panic with the file's path when it cannot be read, parsed, or written.
#[derive(Clone, Debug)]
pub struct GoldenFile {
    path: PathBuf,
    recording: Value,
}

impl GoldenFile {
    /// Read and parse the golden file at `path`.
    pub fn open(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_path_buf();
        // Hold the lock so a test re-recording a shared file on another thread never shows a half-written one.
        let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (recording, _) = read(&path);
        Self {
            path,
            recording,
        }
    }

    /// The parsed recording, for the inputs a test replays.
    pub fn recording(&self) -> &Value {
        &self.recording
    }

    /// Where the file lives.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Assert that `actual` matches the whole recording, or re-record it when [`update_requested`].
    pub fn assert_matches(&self, actual: &Value, tolerance: &Tolerance) {
        self.assert_section_matches("", actual, tolerance);
    }

    /// Assert that `actual` matches the recording's section at the JSON pointer `pointer` (such as `/builtIns`; empty
    /// for the whole recording), or re-record that section when [`update_requested`]. Tests that share one file each
    /// check and re-record only their own sections.
    pub fn assert_section_matches(&self, pointer: &str, actual: &Value, tolerance: &Tolerance) {
        if update_requested() {
            self.rerecord_section(pointer, actual, tolerance);
            return;
        }
        let Some(recorded) = self.recording.pointer(pointer) else {
            panic!(
                "{}: no recorded section {pointer:?}; re-record with {UPDATE_FIXTURES_VARIABLE}=1",
                self.path.display()
            );
        };
        let mismatches = compare(actual, recorded, tolerance);
        if mismatches.is_empty() {
            return;
        }
        let shown: Vec<String> = mismatches.iter().take(REPORTED_MISMATCHES).map(ToString::to_string).collect();
        panic!(
            "{}{}: {} mismatches with the recording (re-record with {UPDATE_FIXTURES_VARIABLE}=1 if the change is \
             intended)\n{}",
            self.path.display(),
            pointer,
            mismatches.len(),
            shown.join("\n")
        );
    }

    /// Replace the section at `pointer` in the file on disk with `actual`, keeping recorded numbers that still agree.
    fn rerecord_section(&self, pointer: &str, actual: &Value, tolerance: &Tolerance) {
        let _guard = WRITE_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        // Read the file again: another test in this process may have re-recorded a different section of it.
        let (mut recording, layout) = read(&self.path);
        // The empty pointer names the whole recording.
        let Some(section) = recording.pointer_mut(pointer) else {
            panic!("{}: no section {pointer:?} to re-record", self.path.display());
        };
        *section = rerecord(actual, section, tolerance);
        write(&self.path, &recording, layout);
    }
}

/// The recording `actual` produces, keeping each number of `recorded` at the same place that still agrees within
/// `tolerance`, so re-recording only rewrites values that changed.
pub fn rerecord(actual: &Value, recorded: &Value, tolerance: &Tolerance) -> Value {
    rerecord_with(actual, Some(recorded), tolerance, tolerance.root())
}

fn rerecord_with(actual: &Value, recorded: Option<&Value>, tolerance: &Tolerance, relative: f64) -> Value {
    match (actual, recorded) {
        (Value::Number(produced), Some(Value::Number(kept)))
            if Tolerance::agrees(relative, number(produced), number(kept)) =>
        {
            Value::Number(kept.clone())
        }
        (Value::Number(produced), _) => Value::Number(produced.clone()),
        (Value::Array(produced), Some(Value::Array(kept))) if produced.len() == kept.len() => Value::Array(
            produced
                .iter()
                .zip(kept)
                .map(|(produced, kept)| rerecord_with(produced, Some(kept), tolerance, relative))
                .collect(),
        ),
        (Value::Array(produced), _) => {
            Value::Array(produced.iter().map(|produced| rerecord_with(produced, None, tolerance, relative)).collect())
        }
        (Value::Object(produced), recorded) => {
            // Recorded keys keep their recorded order and new keys follow, so a re-record never reorders a file.
            let kept = recorded.and_then(Value::as_object);
            let recorded_keys = kept.into_iter().flat_map(|kept| kept.keys()).filter(|key| produced.contains_key(*key));
            let new_keys = produced.keys().filter(|key| kept.is_none_or(|kept| !kept.contains_key(*key)));
            let entries = recorded_keys.chain(new_keys).map(|key| {
                let relative = tolerance.for_key(key, relative);
                let recorded = kept.and_then(|kept| kept.get(key));
                (key.clone(), rerecord_with(&produced[key], recorded, tolerance, relative))
            });
            Value::Object(entries.collect())
        }
        (other, _) => other.clone(),
    }
}

/// Read and parse a golden file and detect its layout.
fn read(path: &Path) -> (Value, Layout) {
    let text = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let recording = serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    (recording, layout(&text))
}

/// The layout of a golden file's text: the indentation of its second line, or one line when it has no other.
fn layout(text: &str) -> Layout {
    let body = text.trim_end_matches('\n');
    let indent = body.lines().nth(1).map_or(0, |line| line.len() - line.trim_start_matches(' ').len());
    Layout {
        indent,
        final_newline: text.ends_with('\n'),
    }
}

/// Write a recording with `layout`.
fn write(path: &Path, recording: &Value, layout: Layout) {
    let indent = " ".repeat(layout.indent);
    let mut bytes = Vec::new();
    let serialized = if layout.indent == 0 {
        let formatter = JavaScriptNumbers(serde_json::ser::CompactFormatter);
        serde::Serialize::serialize(recording, &mut serde_json::Serializer::with_formatter(&mut bytes, formatter))
    } else {
        let formatter = JavaScriptNumbers(serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes()));
        serde::Serialize::serialize(recording, &mut serde_json::Serializer::with_formatter(&mut bytes, formatter))
    };
    serialized.expect("JSON values serialize");
    if layout.final_newline {
        bytes.push(b'\n');
    }

    // Write beside the file and rename over it, so a reader never sees a partly written recording.
    let mut staging = path.as_os_str().to_owned();
    staging.push(".recording");
    let staging = PathBuf::from(staging);
    std::fs::write(&staging, bytes).unwrap_or_else(|error| panic!("{}: {error}", staging.display()));
    std::fs::rename(&staging, path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

/// The spelling JavaScript gives a number: the shortest decimal that round-trips (ryu's digits, as serde_json writes
/// them), laid out fixed for magnitudes from `1e-6` up to `1e21` and exponential (`1e-7`, `1e+21`) outside them,
/// integral values without a fraction, and `-0` as `0`.
fn javascript_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let Some(shortest) = serde_json::Number::from_f64(value).map(|number| number.to_string()) else {
        return "null".to_owned();
    };
    let (sign, unsigned) = match shortest.strip_prefix('-') {
        Some(unsigned) => ("-", unsigned),
        None => ("", shortest.as_str()),
    };
    let (mantissa, exponent) = unsigned.split_once(['e', 'E']).unwrap_or((unsigned, "0"));
    let exponent: i32 = exponent.parse().expect("ryu writes integral exponents");
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));

    // The value is 0.DIGITS x 10^point, with no leading or trailing zeros in DIGITS.
    let all = format!("{whole}{fraction}");
    let leading = all.len() - all.trim_start_matches('0').len();
    let digits = all.trim_start_matches('0').trim_end_matches('0');
    let point = whole.len() as i32 + exponent - leading as i32;
    let count = digits.len() as i32;

    let body = if count <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - count) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let power = point - 1;
        let sign = if power < 0 { "-" } else { "+" };
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() { String::new() } else { format!(".{rest}") };
        format!("{first}{rest}e{sign}{}", power.abs())
    };
    format!("{sign}{body}")
}

/// A JSON formatter that spells floating-point numbers as JavaScript does and otherwise defers to `F`.
struct JavaScriptNumbers<F>(F);

impl<F: serde_json::ser::Formatter> serde_json::ser::Formatter for JavaScriptNumbers<F> {
    fn write_f64<W: ?Sized + std::io::Write>(&mut self, writer: &mut W, value: f64) -> std::io::Result<()> {
        writer.write_all(javascript_number(value).as_bytes())
    }

    fn begin_array<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.0.begin_array(writer)
    }

    fn end_array<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.0.end_array(writer)
    }

    fn begin_array_value<W: ?Sized + std::io::Write>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()> {
        self.0.begin_array_value(writer, first)
    }

    fn end_array_value<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.0.end_array_value(writer)
    }

    fn begin_object<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.0.begin_object(writer)
    }

    fn end_object<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.0.end_object(writer)
    }

    fn begin_object_key<W: ?Sized + std::io::Write>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()> {
        self.0.begin_object_key(writer, first)
    }

    fn end_object_key<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.0.end_object_key(writer)
    }

    fn begin_object_value<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.0.begin_object_value(writer)
    }

    fn end_object_value<W: ?Sized + std::io::Write>(&mut self, writer: &mut W) -> std::io::Result<()> {
        self.0.end_object_value(writer)
    }
}

#[cfg(test)]
#[path = "golden.test.rs"]
mod tests;
