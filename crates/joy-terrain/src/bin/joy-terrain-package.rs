// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Convert a version 1 terrain asset into a streamable version 2 package.
//!
//!     joy-terrain-package <terrain.json> <output folder> [--levels <count>]
//!
//! Reads the asset's manifest and height file, builds a pyramid of `--levels` stored levels (default: as many as the
//! asset's leaves allow, down to one root per side), and writes `terrain.json` plus one `.jtile` file per tile into the
//! output folder. Level-0 bounds and errors come from the asset unchanged. Prints a one-line JSON summary on success
//! and a message on standard error with exit status 1 on failure.

use std::path::PathBuf;
use std::process::ExitCode;

use joy_terrain::{
    build_pyramid_from_asset,
    load_terrain,
    write_terrain_package,
};

/// The optional flag that sets the stored level count.
const LEVELS_FLAG: &str = "--levels";
const USAGE: &str = "usage: joy-terrain-package <terrain.json> <output folder> [--levels <count>]";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// Parse arguments, convert, and describe the written package.
fn run(arguments: Vec<String>) -> Result<String, String> {
    let (manifest, output, levels) = parse_arguments(&arguments)?;
    let asset = load_terrain(&manifest).map_err(|error| error.to_string())?;

    // By default, halve the leaves per side until a single root (or an odd count) remains.
    let levels = levels.unwrap_or_else(|| asset.tiles_per_axis().trailing_zeros() + 1);
    let (layout, tiles) = build_pyramid_from_asset(&asset, levels).map_err(|error| error.to_string())?;
    let package = write_terrain_package(&output, &layout, &tiles).map_err(|error| error.to_string())?;
    let summary = serde_json::json!({
        "package": output.display().to_string(),
        "levels": package.layout.levels,
        "rootTiles": package.layout.root_tiles,
        "tiles": tiles.len(),
    });
    Ok(summary.to_string())
}

/// `(manifest, output folder, levels)` from the command line.
fn parse_arguments(arguments: &[String]) -> Result<(PathBuf, PathBuf, Option<u32>), String> {
    let mut positional = Vec::new();
    let mut levels = None;
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index] == LEVELS_FLAG {
            let value = arguments.get(index + 1).ok_or(USAGE)?;
            levels = Some(value.parse::<u32>().map_err(|_| format!("{LEVELS_FLAG} needs a whole number"))?);
            index += 2;
            continue;
        }
        positional.push(PathBuf::from(&arguments[index]));
        index += 1;
    }
    match positional.as_slice() {
        [manifest, output] => Ok((manifest.clone(), output.clone(), levels)),
        _ => Err(USAGE.into()),
    }
}
