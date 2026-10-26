// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Tile loaders and the on-disk terrain package.
//!
//! A streamer asks a [`TerrainTileLoader`] for stored tiles and polls for completions, so loading may be immediate,
//! threaded, or asynchronous without the streamer knowing. [`MemoryTileLoader`] serves a pyramid already in memory,
//! optionally after a delay that exercises streaming. [`PackageTileLoader`] reads a terrain package's tile files
//! through any [`TerrainByteSource`]: the platform layer supplies one that reads files natively and fetches them over
//! HTTP in browsers, so a browser downloads only the tiles a view or collider needs.
//!
//! A package is a folder holding `terrain.json` (format version 2: the layout and the tile folder) and one `.jtile`
//! file per stored tile at `<tiles>/<level>/<x>-<z>.jtile`.

use std::collections::HashMap;
use std::path::Path;

use serde::{
    Deserialize,
    Serialize,
};

use crate::terrain_error::TerrainError;
use crate::terrain_layout::{
    TerrainLayout,
    TileKey,
};
use crate::terrain_tile::{
    TerrainTile,
    decode_tile,
    encode_tile,
};

/// The package manifest's file name inside a package folder.
pub const PACKAGE_MANIFEST_FILE: &str = "terrain.json";
/// The package format version this crate reads and writes.
pub const PACKAGE_FORMAT_VERSION: u32 = 2;
/// The tile folder written by [`write_terrain_package`].
const DEFAULT_TILE_FOLDER: &str = "tiles";
/// The tile file extension.
const TILE_FILE_EXTENSION: &str = "jtile";

/// One finished load: the requested key and the decoded tile or the failure.
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainTileLoad {
    /// The requested tile.
    pub key: TileKey,
    /// The decoded tile, or why it could not be loaded.
    pub result: Result<TerrainTile, TerrainError>,
}

/// Supplies stored tiles (levels zero and up) without blocking the frame.
///
/// The streamer requests each missing tile once and polls every update; a loader reports every request exactly once,
/// as a tile or an error.
pub trait TerrainTileLoader {
    /// Start loading `key`.
    fn request(&mut self, key: TileKey);
    /// Loads finished since the last poll.
    fn poll(&mut self) -> Vec<TerrainTileLoad>;
}

/// Raw file access for [`PackageTileLoader`]: request a package-relative path, then poll for its bytes.
pub trait TerrainByteSource {
    /// Start reading `path` (forward slashes, relative to the package folder or URL).
    fn request(&mut self, path: &str);
    /// Reads finished since the last poll, as `(path, bytes or failure message)`.
    fn poll(&mut self) -> Vec<(String, Result<Vec<u8>, String>)>;
}

/// Serves tiles held in memory, each after `latency_polls` polls (zero serves on the next poll).
pub struct MemoryTileLoader {
    tiles: HashMap<TileKey, TerrainTile>,
    latency_polls: u32,
    pending: Vec<(TileKey, u32)>,
}

impl MemoryTileLoader {
    /// A loader over stored tiles (for example a pyramid from [`crate::build_terrain_pyramid`]).
    pub fn new(tiles: impl IntoIterator<Item = TerrainTile>, latency_polls: u32) -> Self {
        Self {
            tiles: tiles.into_iter().map(|tile| (tile.key(), tile)).collect(),
            latency_polls,
            pending: Vec::new(),
        }
    }
}

impl TerrainTileLoader for MemoryTileLoader {
    fn request(&mut self, key: TileKey) {
        self.pending.push((key, self.latency_polls));
    }

    fn poll(&mut self) -> Vec<TerrainTileLoad> {
        let mut finished = Vec::new();
        self.pending.retain_mut(|(key, remaining)| {
            if *remaining > 0 {
                *remaining -= 1;
                return true;
            }
            let result = self.tiles.get(key).cloned().ok_or(TerrainError::InvalidTile("the pyramid has no such tile"));
            finished.push(TerrainTileLoad {
                key: *key,
                result,
            });
            false
        });
        finished
    }
}

/// The version 2 package manifest: a layout and where its tiles live.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerrainPackage {
    /// Format version; must be 2.
    pub version: u32,
    /// The pyramid geometry every tile belongs to.
    pub layout: TerrainLayout,
    /// Tile folder relative to the package, with forward slashes.
    pub tiles: String,
}

impl TerrainPackage {
    /// Parse and validate manifest JSON.
    pub fn parse(json: &str) -> Result<Self, TerrainError> {
        let package: Self =
            serde_json::from_str(json).map_err(|error| TerrainError::InvalidManifest(error.to_string()))?;
        if package.version != PACKAGE_FORMAT_VERSION {
            return Err(TerrainError::InvalidManifest(format!("unsupported package version {}", package.version)));
        }
        let escapes = package.tiles.split('/').any(|part| part == "..") || package.tiles.starts_with('/');
        if package.tiles.is_empty() || escapes {
            return Err(TerrainError::InvalidManifest("tiles must be a relative folder inside the package".into()));
        }
        package.layout.validate()?;
        Ok(package)
    }

    /// The package-relative path of a stored tile's file.
    pub fn tile_path(&self, key: TileKey) -> String {
        format!("{}/{}/{}-{}.{TILE_FILE_EXTENSION}", self.tiles, key.level, key.x, key.z)
    }
}

/// Loads a package's tiles through a byte source, decoding and checking each file.
pub struct PackageTileLoader<S: TerrainByteSource> {
    package: TerrainPackage,
    source: S,
    pending: HashMap<String, TileKey>,
}

impl<S: TerrainByteSource> PackageTileLoader<S> {
    /// A loader for `package` reading through `source`.
    pub fn new(package: TerrainPackage, source: S) -> Self {
        Self {
            package,
            source,
            pending: HashMap::new(),
        }
    }

    /// The package this loader reads.
    pub fn package(&self) -> &TerrainPackage {
        &self.package
    }
}

impl<S: TerrainByteSource> TerrainTileLoader for PackageTileLoader<S> {
    fn request(&mut self, key: TileKey) {
        let path = self.package.tile_path(key);
        self.source.request(&path);
        self.pending.insert(path, key);
    }

    fn poll(&mut self) -> Vec<TerrainTileLoad> {
        let mut finished = Vec::new();
        for (path, bytes) in self.source.poll() {
            let Some(key) = self.pending.remove(&path) else {
                continue;
            };
            let result = match bytes {
                Ok(bytes) => decode_tile(&self.package.layout, key, &bytes),
                Err(message) => Err(TerrainError::Io(format!("{path}: {message}"))),
            };
            finished.push(TerrainTileLoad {
                key,
                result,
            });
        }
        finished
    }
}

/// Write a package folder: `terrain.json` and every stored tile's file. Creates folders as needed and replaces files
/// already there.
pub fn write_terrain_package(
    folder: impl AsRef<Path>,
    layout: &TerrainLayout,
    tiles: &[TerrainTile],
) -> Result<TerrainPackage, TerrainError> {
    layout.validate()?;
    let folder = folder.as_ref();
    let package = TerrainPackage {
        version: PACKAGE_FORMAT_VERSION,
        layout: layout.clone(),
        tiles: DEFAULT_TILE_FOLDER.into(),
    };
    let io = |error: std::io::Error| TerrainError::Io(error.to_string());
    for tile in tiles {
        let path = folder.join(package.tile_path(tile.key()));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        std::fs::write(&path, encode_tile(layout, tile)?).map_err(io)?;
    }
    let manifest = serde_json::to_string_pretty(&package).map_err(|error| TerrainError::Io(error.to_string()))?;
    std::fs::write(folder.join(PACKAGE_MANIFEST_FILE), manifest + "\n").map_err(io)?;
    Ok(package)
}
