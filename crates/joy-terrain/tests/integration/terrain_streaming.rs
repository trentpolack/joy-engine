// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Streaming: views refine within their budgets, residency stays bounded, collision regions keep the gameplay level
//! resident, craters reach every level and survive eviction, and synthesized detail is deterministic, seamless, and
//! shaped by material.

use joy_terrain::{
    MemoryTileLoader,
    TerrainCrater,
    TerrainDetail,
    TerrainInterest,
    TerrainLayout,
    TerrainStreamer,
    TerrainStreamerOptions,
    TerrainStreamingBudget,
    TerrainSurface,
    TerrainTile,
    TileKey,
    select_terrain_nodes,
};

use crate::{
    rolling_pyramid,
    view,
};

/// A low-resolution projection so the rough test terrain mixes levels (CSS pixels per unit at unit distance).
const COARSE_PROJECTION_SCALE: f64 = 15.0;

/// A streamer over the four-level rolling pyramid whose loads finish after `latency` polls.
fn streamer(options: TerrainStreamerOptions, latency: u32) -> (TerrainStreamer, TerrainLayout, Vec<TerrainTile>) {
    let (layout, tiles) = rolling_pyramid(4);
    let loader = MemoryTileLoader::new(tiles.clone(), latency);
    let streamer = TerrainStreamer::new(layout.clone(), Box::new(loader), options).expect("valid options");
    (streamer, layout, tiles)
}

/// A view interest low over local (`x`, `z`).
fn low_view(x: f64, z: f64) -> TerrainInterest {
    let mut camera = view([x, 8.0, z], [x + 4.0, 0.0, z + 4.0], 0.0);
    camera.projection_scale = COARSE_PROJECTION_SCALE;
    TerrainInterest::View(camera)
}

#[test]
fn views_refine_within_their_loading_budget() {
    let budget = TerrainStreamingBudget {
        maximum_pending_loads: 3,
        ..Default::default()
    };
    let options = TerrainStreamerOptions {
        budget,
        ..Default::default()
    };
    let (mut streamer, _, _) = streamer(options, 2);
    let interest = low_view(-50.0, -50.0);

    // Nothing draws until the roots arrive; then the view refines a few loads at a time until it is satisfied.
    let first = streamer.update(&[interest]).unwrap();
    assert!(streamer.surface().is_none() && first.pending_loads == 4);
    let TerrainInterest::View(camera) = interest else { unreachable!() };
    let mut settled = None;
    for _ in 0..400 {
        let report = streamer.update(&[interest]).unwrap();
        assert!(report.pending_loads <= 4.max(budget.maximum_pending_loads));
        let Some(surface) = streamer.surface() else {
            continue;
        };
        let selection = select_terrain_nodes(surface, &camera).unwrap();
        if selection.wanted.is_empty() && report.pending_loads == 0 {
            settled = Some(selection);
            break;
        }
    }
    let selection = settled.expect("the view never finished refining");
    assert!(selection.nodes.iter().any(|node| node.key.level == 0), "the view reached the finest level near it");
    assert!(selection.nodes.iter().any(|node| node.key.level >= 2), "distant tiles stayed coarse");
}

#[test]
fn residency_stays_within_budget_as_the_view_travels() {
    let budget = TerrainStreamingBudget {
        maximum_resident_tiles: 200,
        ..Default::default()
    };
    let options = TerrainStreamerOptions {
        budget,
        ..Default::default()
    };
    let (mut streamer, layout, _) = streamer(options, 0);
    let (mut largest, mut evicted) = (0, 0);
    for step in 0..120 {
        let x = -60.0 + f64::from(step);
        let report = streamer.update(&[low_view(x, x * 0.5)]).unwrap();
        largest = largest.max(report.resident_tiles);
        evicted += report.evicted;
    }
    let surface = streamer.surface().unwrap();
    assert!(layout.roots().all(|root| surface.tile(root).is_some()), "roots never leave");
    assert!(largest <= budget.maximum_resident_tiles, "resident tiles reached {largest}");
    assert!(evicted > 0, "travel should outgrow the budget and release tiles the view left behind");
}

#[test]
fn collision_regions_keep_the_gameplay_level_resident() {
    let (mut streamer, _, _) = streamer(TerrainStreamerOptions::default(), 1);
    let region = TerrainInterest::Collision {
        center: [20.0, -30.0],
        radius: 10.0,
    };
    for _ in 0..12 {
        streamer.update(&[region]).unwrap();
    }
    let surface = streamer.surface().unwrap();
    assert!(surface.height_at(20.0, -30.0).is_some());
    assert!(surface.height_at(29.0, -30.0).is_some());
    assert!(surface.height_at(-60.0, 60.0).is_none(), "gameplay height is unknown where nothing is resident");
    assert!(surface.resident_height_at(-60.0, 60.0).is_some(), "the coarse surface still answers placement");
}

#[test]
fn craters_reach_every_level_and_tiles_loaded_later() {
    let options = TerrainStreamerOptions {
        budget: TerrainStreamingBudget {
            maximum_resident_tiles: 24,
            ..Default::default()
        },
        ..Default::default()
    };
    let (mut streamer, _, _) = streamer(options, 0);
    let region = TerrainInterest::Collision {
        center: [16.0, 16.0],
        radius: 6.0,
    };
    for _ in 0..8 {
        streamer.update(&[region]).unwrap();
    }
    let before = streamer.surface().unwrap().height_at(16.0, 16.0).unwrap();
    let crater = TerrainCrater {
        center: [16.0, 16.0],
        radius: 6.0,
        depth: 3.0,
        rim_height: 0.5,
    };
    streamer.add_crater(crater).unwrap();
    streamer.update(&[region]).unwrap();
    let surface = streamer.surface().unwrap().clone();
    let after = surface.height_at(16.0, 16.0).unwrap();
    assert!((before - after - 3.0).abs() < 1e-3, "the bowl sank by its depth: {before} -> {after}");

    // Every level samples the same edited surface at the crater's center (a lattice point of every level).
    for level in 0..=3 {
        let key = surface_key(&surface, level, 16.0, 16.0);
        let tile = surface.tile(key).expect("the chain to the region is resident");
        let origin = surface.layout().tile_origin_samples(key);
        let step = f64::from(1 << level);
        let (x, z) = ((80.0 - origin[0]) / step, (80.0 - origin[1]) / step);
        assert!((f64::from(tile.height(x as i32, z as i32)) - after).abs() < 1e-3, "level {level} missed the crater");
    }

    // Tiles that leave and come back keep the crater.
    let elsewhere = TerrainInterest::Collision {
        center: [-50.0, -50.0],
        radius: 12.0,
    };
    for _ in 0..8 {
        streamer.update(&[elsewhere]).unwrap();
    }
    assert!(streamer.surface().unwrap().height_at(16.0, 16.0).is_none(), "the cratered tile left");
    for _ in 0..8 {
        streamer.update(&[region]).unwrap();
    }
    let returned = streamer.surface().unwrap().height_at(16.0, 16.0).unwrap();
    assert!((returned - after).abs() < 1e-6);
}

#[test]
fn detail_is_deterministic_seamless_and_shaped_by_material() {
    let detail = TerrainDetail::default();
    let options = TerrainStreamerOptions {
        detail: Some(detail),
        ..Default::default()
    };
    let region = TerrainInterest::Collision {
        center: [-9.0, 7.0],
        radius: 6.0,
    };
    let run = || {
        let (mut streamer, _, _) = streamer(options, 0);
        for _ in 0..16 {
            streamer.update(&[region]).unwrap();
        }
        streamer.surface().unwrap().clone()
    };
    let (first, second) = (run(), run());
    assert_eq!(first.collision_level(), -2);

    // Two independent streams synthesize identical tiles, bit for bit.
    let mut synthesized = 0;
    for tile in first.tiles().filter(|tile| tile.key().level < 0) {
        let twin = second.tile(tile.key()).expect("both streams synthesized the same tiles");
        assert_eq!(tile.heights(), twin.heights(), "{:?} differs between runs", tile.key());
        synthesized += 1;
    }
    assert!(synthesized >= 4, "the region needs several detail tiles");

    // Neighboring detail tiles share their edge samples exactly.
    let layout = first.layout();
    let cells = layout.tile_cells as i32;
    for tile in first.tiles().filter(|tile| tile.key().level == -2) {
        let right = TileKey::new(-2, tile.key().x + 1, tile.key().z);
        if let Some(neighbor) = first.tile(right) {
            for z in 0..=cells {
                assert_eq!(tile.height(cells, z), neighbor.height(0, z));
            }
        }
    }

    // Detail vanishes on the authored lattice: a level -1 tile's even samples are its level-0 parent's, so the two draw
    // the same edges side by side and the same surface at the moment one replaces the other.
    let mut compared = 0;
    for tile in first.tiles().filter(|tile| tile.key().level == -1) {
        let key = tile.key();
        let parent = first.tile(key.ancestor(0)).expect("detail tiles keep their level-0 parent resident");
        let half = cells / 2;
        let (offset_x, offset_z) = ((key.x % 2) as i32 * half, (key.z % 2) as i32 * half);
        for z in (0..=cells).step_by(2) {
            for x in (0..=cells).step_by(2) {
                assert_eq!(
                    tile.height(x, z),
                    parent.height(offset_x + (x / 2), offset_z + (z / 2)),
                    "{key:?} at {x},{z}"
                );
                compared += 1;
            }
        }
    }
    assert!(compared > 0, "the region needs level -1 tiles");

    // Detail adds relief between the authored samples, and the gameplay height follows it.
    let authored = TerrainSurface::from_tiles(layout.clone(), rolling_pyramid(4).1, Default::default()).unwrap();
    let mut largest: f64 = 0.0;
    for step in 0..40 {
        let (x, z) = (-12.0 + (f64::from(step) * 0.15), 7.0 + 0.37);
        let detailed = first.height_at(x, z).unwrap();
        largest = largest.max((detailed - authored.height_at(x, z).unwrap()).abs());
    }
    assert!(largest > 0.05, "synthesized detail should move the surface off the authored triangles");
    assert!(largest < detail.amplitude_bound() + 2.0, "detail stays near its amplitude bound ({largest})");
}

/// The key of the tile of `level` containing local (`x`, `z`).
fn surface_key(surface: &TerrainSurface, level: i32, x: f64, z: f64) -> TileKey {
    let layout = surface.layout();
    let sample = layout.local_to_sample(x, z);
    let span = layout.tile_span_samples(level);
    TileKey::new(level, (sample[0] / span) as u32, (sample[1] / span) as u32)
}

#[test]
fn oversubscribed_views_settle_without_churning_tiles() {
    // A low view over detailed terrain wants far more tiles than the budget holds.
    let options = TerrainStreamerOptions {
        detail: Some(TerrainDetail::default()),
        budget: TerrainStreamingBudget {
            maximum_resident_tiles: 60,
            ..Default::default()
        },
        ..Default::default()
    };
    let (mut streamer, _, _) = streamer(options, 0);
    let interest = low_view(-20.0, -20.0);
    let mut settled_work = 0;
    for update in 0..200 {
        let report = streamer.update(&[interest]).unwrap();
        assert!(report.resident_tiles <= 60 + 4, "residency stays near its budget: {report:?}");
        if update >= 150 {
            settled_work += report.loaded + report.synthesized + report.evicted;
        }
        if update == 199 {
            assert!(report.waiting > 0, "the view still wants more than the budget allows");
        }
    }
    // Only the occasional retry after a backoff remains (without it, every update fetched and released tiles).
    assert!(settled_work < 25, "a saturated stream churned {settled_work} tiles over 50 updates");
}
