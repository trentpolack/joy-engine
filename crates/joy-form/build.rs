// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Fingerprint the evaluator's source for `FORM_EVALUATOR_VERSION`, so cooked FORM geometry and its cache entries miss
//! whenever the evaluator could produce different output.
//!
//! The fingerprint hashes every library source file under `src/` (unit tests and binaries excluded) in sorted path
//! order, with line endings normalized so checkouts on every platform agree. A comment edit also changes it; the cost
//! is one re-cook, which is cheaper than trusting a hand-maintained version number to be bumped.

use std::path::{
    Path,
    PathBuf,
};

/// FNV-1a 64-bit parameters; the fingerprint only needs to change when the source does.
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Source files that cannot change evaluation: unit tests and command-line binaries.
const EXCLUDED_SUFFIX: &str = ".test.rs";
const EXCLUDED_FOLDER: &str = "bin";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let source_folder = manifest.join("src");
    println!("cargo::rerun-if-changed={}", source_folder.display());

    let mut files = Vec::new();
    collect_sources(&source_folder, &mut files)?;
    files.sort();

    let mut hash = FNV_OFFSET;
    for file in &files {
        let relative = file.strip_prefix(&source_folder)?.to_string_lossy().replace('\\', "/");
        let text = std::fs::read_to_string(file)?.replace("\r\n", "\n");
        for bytes in [relative.as_bytes(), text.as_bytes()] {
            // Length prefixes keep a byte moving between a path and its text from producing the same stream.
            for byte in (bytes.len() as u64).to_le_bytes().iter().chain(bytes) {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(FNV_PRIME);
            }
        }
    }
    println!("cargo::rustc-env=JOY_FORM_SOURCE_FINGERPRINT={hash:016x}");
    Ok(())
}

/// Every `.rs` file under `folder`, skipping unit tests and binaries.
fn collect_sources(folder: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let path = entry?.path();
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if path.is_dir() {
            if name != EXCLUDED_FOLDER {
                collect_sources(&path, files)?;
            }
        } else if name.ends_with(".rs") && !name.ends_with(EXCLUDED_SUFFIX) {
            files.push(path);
        }
    }
    Ok(())
}
