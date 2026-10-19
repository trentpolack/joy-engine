// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Project-relative authored asset references, validated without performing I/O.

use super::error::AssetPathError;

/// Required prefix of every authored asset path.
const ASSET_ROOT: &str = "assets/";

/// Characters that could escape the asset root or change URL meaning.
const FORBIDDEN_CHARACTERS: &[char] = &['\\', '?', '#', '%', ':'];

/// Validate a project-relative asset path and return it unchanged.
///
/// The path must start with `assets/`, contain no `\ ? # % :` or control characters (U+0000 to U+001F), and have no
/// empty, `.`, or `..` segments. When `extensions` is nonempty, the lowercased path must end with one of them (each
/// including its dot, for example `".joyobject"`).
pub fn asset_path<'a>(path: &'a str, extensions: &[&str]) -> Result<&'a str, AssetPathError> {
    let unsafe_character =
        path.chars().any(|character| FORBIDDEN_CHARACTERS.contains(&character) || character <= '\u{1f}');
    let unsafe_segment = path.split('/').any(|part| part.is_empty() || part == "." || part == "..");
    if !path.starts_with(ASSET_ROOT) || unsafe_character || unsafe_segment {
        return Err(AssetPathError::OutsideAssets);
    }

    if !extensions.is_empty() {
        let lowercase = path.to_lowercase();
        if !extensions.iter().any(|extension| lowercase.ends_with(extension)) {
            return Err(AssetPathError::WrongExtension(extensions.join(" or ")));
        }
    }
    Ok(path)
}

#[cfg(test)]
#[path = "../tests/unit/asset_path.rs"]
mod tests;
