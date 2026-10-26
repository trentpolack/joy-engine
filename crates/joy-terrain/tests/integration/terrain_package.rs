// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Tile files and packages: tiles survive a file round trip within quantization, malformed files are refused, and the
//! packaging tool writes a package a streamer loads tile by tile.

use std::path::PathBuf;
use std::process::Command;

use joy_terrain::{
    PACKAGE_MANIFEST_FILE,
    PackageTileLoader,
    TerrainByteSource,
    TerrainError,
    TerrainInterest,
    TerrainPackage,
    TerrainStreamer,
    TerrainStreamerOptions,
    TileKey,
    build_pyramid_from_asset,
    decode_tile,
    encode_tile,
};

use crate::{
    dragon_pit,
    view,
};

/// Reads package files synchronously from a folder, answering on the next poll.
struct FolderSource {
    folder: PathBuf,
    requested: Vec<String>,
}

impl TerrainByteSource for FolderSource {
    fn request(&mut self, path: &str) {
        self.requested.push(path.to_owned());
    }

    fn poll(&mut self) -> Vec<(String, Result<Vec<u8>, String>)> {
        self.requested
            .drain(..)
            .map(|path| {
                let bytes = std::fs::read(self.folder.join(&path)).map_err(|error| error.to_string());
                (path, bytes)
            })
            .collect()
    }
}

#[test]
fn tile_files_round_trip_and_refuse_mismatches() {
    let (layout, tiles) = build_pyramid_from_asset(&dragon_pit(), 3).expect("three levels");
    let tile = tiles.iter().find(|tile| tile.key() == TileKey::new(1, 3, 2)).expect("the tile exists");
    let bytes = encode_tile(&layout, tile).expect("stored tiles encode");
    let decoded = decode_tile(&layout, tile.key(), &bytes).expect("the file decodes");

    // Samples come back within half a quantization step; bounds and errors are exact.
    let step = layout.height_scale / 65535.0;
    for (original, restored) in tile.heights().iter().zip(decoded.heights()) {
        assert!(f64::from((original - restored).abs()) <= (step * 0.5) + 1e-3);
    }
    assert_eq!((decoded.minimum(), decoded.maximum()), (tile.minimum(), tile.maximum()));
    assert_eq!(decoded.errors(), tile.errors());

    assert!(matches!(decode_tile(&layout, TileKey::new(1, 3, 3), &bytes), Err(TerrainError::InvalidTile(_))));
    assert!(matches!(decode_tile(&layout, tile.key(), &bytes[..bytes.len() - 2]), Err(TerrainError::InvalidTile(_))));
    assert!(matches!(decode_tile(&layout, tile.key(), b"JTIX"), Err(TerrainError::InvalidTile(_))));
}

#[test]
fn the_packaging_tool_writes_a_package_a_streamer_loads() {
    let folder = std::env::temp_dir().join(format!("joy-terrain-package-{}", std::process::id()));
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/terrain/assets/dragon-pit/terrain.json");
    let output = Command::new(env!("CARGO_BIN_EXE_joy-terrain-package"))
        .args([manifest, folder.to_str().expect("a UTF-8 temporary folder"), "--levels", "3"])
        .output()
        .expect("the packaging tool runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let package = TerrainPackage::parse(&std::fs::read_to_string(folder.join(PACKAGE_MANIFEST_FILE)).unwrap())
        .expect("the written manifest is valid");
    assert_eq!(package.layout.root_tiles, [4, 4]);
    let layout = package.layout.clone();
    let source = FolderSource {
        folder: folder.clone(),
        requested: Vec::new(),
    };
    let loader = PackageTileLoader::new(package, source);
    let mut streamer = TerrainStreamer::new(layout, Box::new(loader), TerrainStreamerOptions::default()).unwrap();

    // A camera low over one corner pulls level-0 tiles there through the files.
    let interest = TerrainInterest::View(view([-900.0, 330.0, -900.0], [-950.0, 300.0, -950.0], 0.0));
    for _ in 0..8 {
        streamer.update(&[interest]).expect("valid interests");
    }
    let surface = streamer.surface().expect("the roots arrived");
    assert!(surface.tile(TileKey::new(0, 0, 0)).is_some(), "the near corner streamed to the finest level");
    assert!(surface.tile(TileKey::new(0, 15, 15)).is_none(), "the far corner stayed coarse");
    std::fs::remove_dir_all(&folder).expect("the temporary package is removed");
}
