// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Hand a generated text or binary file to the player: browsers download it, and native builds write it into the
//! working directory. Tools and examples use it for one-off exports such as baked
//! meshes and editable recipes that the player takes elsewhere. Values the game reads back later (settings, progress)
//! belong in persisted storage (`runtime::Storage`) instead, and authored documents that must not be overwritten use the
//! development editors' guarded saves.

/// Why a generated file was not saved.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SaveFileError {
    /// The name was empty, a folder reference, or contained a path separator.
    #[error("Saved files need a plain file name without folders: \"{0}\".")]
    InvalidName(String),
    /// The browser or file system refused the file.
    #[error("Could not save {name}: {message}")]
    Failed {
        /// The requested file name.
        name: String,
        /// The platform's explanation.
        message: String,
    },
}

/// Save `contents` as `file_name` and return where it went: the written path natively, or the downloaded file name in
/// browsers. Native builds replace an existing file of that name in the working directory.
pub fn save_text_file(file_name: &str, contents: &str) -> Result<String, SaveFileError> {
    save_file(file_name, contents.as_bytes(), true)
}

/// Save binary `bytes` (GLB exports, images) as `file_name`, like [`save_text_file`]; browsers download them as
/// `application/octet-stream` unless the extension names a known type (`.glb`, `.png`).
pub fn save_binary_file(file_name: &str, bytes: &[u8]) -> Result<String, SaveFileError> {
    save_file(file_name, bytes, false)
}

fn save_file(file_name: &str, bytes: &[u8], text: bool) -> Result<String, SaveFileError> {
    if file_name.is_empty() || file_name == "." || file_name == ".." || file_name.contains(['/', '\\']) {
        return Err(SaveFileError::InvalidName(file_name.to_owned()));
    }
    let failed = |message: String| SaveFileError::Failed { name: file_name.to_owned(), message };

    #[cfg(target_arch = "wasm32")]
    {
        let result = if text {
            crate::browser::dom::download_text(file_name, std::str::from_utf8(bytes).unwrap_or_default())
        } else {
            crate::browser::dom::download_bytes(file_name, bytes, media_type(file_name))
        };
        result.map_err(failed)?;
        Ok(file_name.to_owned())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = text;
        let directory = std::env::current_dir().map_err(|error| failed(error.to_string()))?;
        write_file(&directory, file_name, bytes).map_err(|error| failed(error.to_string()))
    }
}

/// The download media type for a binary file name.
#[cfg(target_arch = "wasm32")]
fn media_type(file_name: &str) -> &'static str {
    let lower = file_name.to_lowercase();
    if lower.ends_with(".glb") {
        "model/gltf-binary"
    } else if lower.ends_with(".png") {
        "image/png"
    } else {
        "application/octet-stream"
    }
}

/// Write `bytes` into `directory`, returning the written path for display.
#[cfg(not(target_arch = "wasm32"))]
fn write_file(directory: &std::path::Path, file_name: &str, bytes: &[u8]) -> std::io::Result<String> {
    let path = directory.join(file_name);
    std::fs::write(&path, bytes)?;
    Ok(path.display().to_string())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn files_save_by_plain_name_only() {
        for name in ["", ".", "..", "nested/recipe.json", "..\\recipe.json"] {
            assert_eq!(save_text_file(name, "{}"), Err(SaveFileError::InvalidName(name.to_owned())));
            assert_eq!(save_binary_file(name, &[1]), Err(SaveFileError::InvalidName(name.to_owned())));
        }
        let directory = std::env::temp_dir().join(format!("joy-save-file-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = write_file(&directory, "recipe.formlab", b"{\"version\": 1}\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"version\": 1}\n");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
