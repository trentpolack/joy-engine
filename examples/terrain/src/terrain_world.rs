// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The example's terrain systems: a streamer paging Dragon Pit (from the embedded asset or a package), synthesized
//! detail, crater edits, and a physics world whose terrain colliders hold up dropped props.
//!
//! One frame, in order: the camera view and a collision region around the focus and every prop drive the streamer;
//! a newly published surface snapshot replaces the drawn one and resyncs the colliders around the same anchors; then
//! physics steps. Craters and props are gameplay edits, so they always go through the streamer and the physics world,
//! never straight into rendered data. The terrain's local space is the world (the instance is untransformed).

use joy_engine::core::{
    PrimitiveShape,
    SeededRandom,
    random_range,
};
use joy_engine::math::Vec3;
use joy_engine::physics::{
    BodyHandle,
    BodyOptions,
    PhysicsTransform,
    PhysicsWorld,
    PhysicsWorldOptions,
    SdfPrimitive,
    SdfShape,
    TerrainColliderOptions,
    TerrainColliderReport,
    TerrainColliders,
};
use joy_engine::runtime::{
    AppResult,
    AssetFetcher,
};
use joy_engine::terrain::{
    MemoryTileLoader,
    PackageTileLoader,
    TerrainAsset,
    TerrainCrater,
    TerrainInterest,
    TerrainLayout,
    TerrainPackage,
    TerrainStreamer,
    TerrainStreamerOptions,
    TerrainStreamingReport,
    TerrainSurface,
    TerrainTile,
    TerrainTileLoader,
    TerrainView,
    build_pyramid_from_asset,
};

use super::terrain_config::TERRAIN_CONFIG;

/// Where tiles come from.
pub enum TileSource {
    /// The pyramid built from the embedded asset, served from memory after a short delay.
    Embedded(Vec<TerrainTile>),
    /// A package read through the platform fetcher: files natively, HTTP in browsers.
    Package {
        /// The parsed manifest.
        package: TerrainPackage,
        /// The package folder or URL prefix.
        root: String,
    },
}

/// The two prop kinds: SDF crates exercise SDF contacts against terrain, spheres the solver's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropKind {
    /// A box collider built as a signed distance field.
    Crate,
    /// A solver sphere.
    Ball,
}

/// One dropped prop.
#[derive(Clone, Copy, Debug)]
pub struct Prop {
    /// The physics body.
    pub body: BodyHandle,
    /// Which mesh draws it.
    pub kind: PropKind,
}

/// One prop's kind, solver pose, and the gameplay ground under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PropSnapshot {
    /// Which mesh draws it.
    pub kind: PropKind,
    /// The solver's pose.
    pub pose: PhysicsTransform,
    /// The gameplay height (collision level) under its center, when resident.
    pub ground: Option<f64>,
    /// Linear speed in world units per second.
    pub speed: f32,
}

impl PropSnapshot {
    /// Whether the prop has come to rest (moving slower than `speed` world units per second).
    pub fn at_rest(&self, speed: f32) -> bool {
        self.speed < speed
    }
}

/// Copied state for the HUD and tests. Mutating it never changes the world.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldSnapshot {
    /// Whether synthesized detail is on.
    pub detail: bool,
    /// Whether the first surface snapshot has arrived.
    pub ready: bool,
    /// The latest streaming update's counts.
    pub streaming: TerrainStreamingReport,
    /// Terrain colliders after the latest sync.
    pub colliders: usize,
    /// Every crater applied so far.
    pub craters: Vec<TerrainCrater>,
    /// Every prop, in drop order.
    pub props: Vec<PropSnapshot>,
    /// The gameplay height (collision level) under the focus, when resident.
    pub focus_ground: Option<f64>,
}

/// Streaming, edits, and physics for one landscape.
pub struct TerrainWorld {
    // Terrain data: where tiles come from, the stream, and the latest snapshot.
    source: TileSource,
    layout: TerrainLayout,
    detail: bool,
    streamer: TerrainStreamer,
    surface: Option<TerrainSurface>,
    streaming: TerrainStreamingReport,

    // Physics: terrain colliders and the props they hold up.
    physics: PhysicsWorld,
    colliders: TerrainColliders,
    collider_report: TerrainColliderReport,
    crate_shape: SdfShape,
    props: Vec<Prop>,
    scatter: SeededRandom,
}

impl TerrainWorld {
    /// Stream the embedded asset as a pyramid of the configured levels.
    pub fn embedded(asset: &TerrainAsset, detail: bool) -> AppResult<Self> {
        let (layout, tiles) = build_pyramid_from_asset(asset, TERRAIN_CONFIG.streaming.levels)?;
        Self::new(TileSource::Embedded(tiles), layout, detail)
    }

    /// Stream a package from `root` (a folder natively, a URL prefix in browsers).
    pub fn package(package: TerrainPackage, root: String, detail: bool) -> AppResult<Self> {
        let layout = package.layout.clone();
        Self::new(
            TileSource::Package {
                package,
                root,
            },
            layout,
            detail,
        )
    }

    fn new(source: TileSource, layout: TerrainLayout, detail: bool) -> AppResult<Self> {
        let streamer = create_streamer(&source, &layout, detail)?;
        let props = TERRAIN_CONFIG.props;
        Ok(Self {
            source,
            layout,
            detail,
            streamer,
            surface: None,
            streaming: TerrainStreamingReport::default(),
            physics: PhysicsWorld::new(PhysicsWorldOptions::default())?,
            colliders: TerrainColliders::new(TerrainColliderOptions {
                radius: TERRAIN_CONFIG.streaming.collision_radius as f32,
                ..Default::default()
            })?,
            collider_report: TerrainColliderReport::default(),
            crate_shape: SdfShape::primitive(SdfPrimitive::Box {
                half_extents: Vec3::splat(props.crate_half_extent),
            })?,
            props: Vec::new(),
            scatter: SeededRandom::new(props.scatter_seed),
        })
    }

    /// Switch synthesized detail on or off. Restarts the stream (a new lineage, so the renderer rebuilds its tile
    /// pool) and replays every crater onto it.
    pub fn set_detail(&mut self, enabled: bool) -> AppResult {
        if enabled == self.detail {
            return Ok(());
        }
        let craters = self.streamer.craters().to_vec();
        let mut streamer = create_streamer(&self.source, &self.layout, enabled)?;
        for crater in craters {
            streamer.add_crater(crater)?;
        }
        self.streamer = streamer;
        self.detail = enabled;
        self.surface = None;
        Ok(())
    }

    /// The latest surface snapshot, once the roots have arrived.
    pub fn surface(&self) -> Option<&TerrainSurface> {
        self.surface.as_ref()
    }

    /// Stream for `view` and a collision region around `focus` and every prop; resync colliders when a new snapshot
    /// is published. Returns the new snapshot, if any.
    pub fn update_streaming(&mut self, view: TerrainView, focus: Vec3) -> AppResult<Option<TerrainSurface>> {
        let anchors = self.anchors(focus);
        let radius = TERRAIN_CONFIG.streaming.collision_radius;
        let mut interests = vec![TerrainInterest::View(view)];
        interests.extend(anchors.iter().map(|anchor| TerrainInterest::Collision {
            center: [f64::from(anchor.x), f64::from(anchor.z)],
            radius,
        }));
        self.streaming = self.streamer.update(&interests)?;
        let Some(published) = self.streaming.published.then(|| self.streamer.surface().cloned()).flatten() else {
            return Ok(None);
        };
        self.collider_report = self.colliders.sync(&mut self.physics, &published, &anchors)?;
        self.surface = Some(published.clone());
        Ok(Some(published))
    }

    /// Advance physics by `seconds`, keeping colliders under props that moved since the last snapshot.
    pub fn step_physics(&mut self, seconds: f32, focus: Vec3) -> AppResult {
        if let Some(surface) = &self.surface {
            let anchors = self.anchors(focus);
            self.collider_report = self.colliders.sync(&mut self.physics, surface, &anchors)?;
        }
        self.physics.step(seconds)?;
        Ok(())
    }

    /// Place a crater where a world ray first meets the surface; returns the crater center, or `None` on a miss.
    pub fn place_crater(&mut self, origin: Vec3, direction: Vec3) -> AppResult<Option<Vec3>> {
        let Some(surface) = &self.surface else {
            return Ok(None);
        };
        let reach = TERRAIN_CONFIG.crater.maximum_pick_distance;
        let Some(hit) = surface.raycast(origin.as_dvec3(), direction.as_dvec3(), reach) else {
            return Ok(None);
        };
        self.streamer.add_crater(TerrainCrater {
            center: [hit.point.x, hit.point.z],
            ..TERRAIN_CONFIG.crater.shape
        })?;
        Ok(Some(hit.point.as_vec3()))
    }

    /// Drop a burst of props above `focus`; the oldest props leave past the configured maximum.
    pub fn drop_props(&mut self, focus: Vec3) -> AppResult {
        let config = TERRAIN_CONFIG.props;
        let ground = self.ground_height(focus).unwrap_or(f64::from(focus.y)) as f32;
        for index in 0..config.burst {
            let spread = f64::from(config.spread);
            let offset_x = random_range(-spread, spread, &mut self.scatter) as f32;
            let offset_z = random_range(-spread, spread, &mut self.scatter) as f32;
            let lift = config.drop_height + (index as f32 * (config.crate_half_extent * 1.5));
            let position = Vec3::new(focus.x + offset_x, ground + lift, focus.z + offset_z);
            let options = BodyOptions {
                mass: config.mass,
                radius: config.ball_radius,
                position,
                rotation: Vec3::new(0.3 * index as f32, 0.7 * index as f32, 0.0),
                friction: 0.9,
                linear_damping: 0.1,
                angular_damping: 0.8,
                ..BodyOptions::new(PrimitiveShape::Sphere)
            };
            let prop = if index % 2 == 0 {
                Prop {
                    body: self.physics.create_sdf_body(&self.crate_shape, options)?,
                    kind: PropKind::Crate,
                }
            } else {
                Prop {
                    body: self.physics.create_body(options)?,
                    kind: PropKind::Ball,
                }
            };
            self.props.push(prop);
        }
        while self.props.len() > config.maximum {
            let oldest = self.props.remove(0);
            self.physics.remove_body(oldest.body);
        }
        Ok(())
    }

    /// The props in drop order.
    pub fn props(&self) -> &[Prop] {
        &self.props
    }

    /// A prop's presentation pose, blended between fixed steps.
    pub fn prop_transform(&self, prop: &Prop) -> Option<PhysicsTransform> {
        self.physics.interpolated_body_transform(prop.body)
    }

    /// Copy the state the HUD and tests read, with the gameplay ground under `focus`.
    pub fn snapshot(&self, focus: Vec3) -> WorldSnapshot {
        let props = self.props.iter().filter_map(|prop| self.prop_snapshot(prop)).collect();
        WorldSnapshot {
            detail: self.detail,
            ready: self.surface.is_some(),
            streaming: self.streaming,
            colliders: self.collider_report.colliders,
            craters: self.streamer.craters().to_vec(),
            props,
            focus_ground: self
                .surface
                .as_ref()
                .and_then(|surface| surface.height_at(f64::from(focus.x), f64::from(focus.z))),
        }
    }

    /// One prop's pose, speed, and the gameplay ground under it.
    fn prop_snapshot(&self, prop: &Prop) -> Option<PropSnapshot> {
        let pose = self.physics.body_transform(prop.body)?;
        let position = pose.position;
        Some(PropSnapshot {
            kind: prop.kind,
            pose,
            ground: self
                .surface
                .as_ref()
                .and_then(|surface| surface.height_at(f64::from(position.x), f64::from(position.z))),
            speed: self.physics.body_velocity(prop.body).map(|velocity| velocity.length()).unwrap_or(0.0),
        })
    }

    /// The finest resident terrain height under a point, for cameras and placement.
    pub fn ground_height(&self, point: Vec3) -> Option<f64> {
        self.surface.as_ref()?.resident_height_at(f64::from(point.x), f64::from(point.z))
    }

    /// Where the terrain must be solid: the focus and every prop.
    fn anchors(&self, focus: Vec3) -> Vec<Vec3> {
        let mut anchors = vec![focus];
        anchors.extend(
            self.props.iter().filter_map(|prop| self.physics.body_transform(prop.body)).map(|pose| pose.position),
        );
        anchors
    }
}

/// A streamer over `source` with or without detail.
fn create_streamer(source: &TileSource, layout: &TerrainLayout, detail: bool) -> AppResult<TerrainStreamer> {
    let streaming = TERRAIN_CONFIG.streaming;
    let loader: Box<dyn TerrainTileLoader> = match source {
        TileSource::Embedded(tiles) => Box::new(MemoryTileLoader::new(tiles.clone(), streaming.memory_latency_polls)),
        TileSource::Package {
            package,
            root,
        } => Box::new(PackageTileLoader::new(package.clone(), AssetFetcher::new(root.clone()))),
    };
    let options = TerrainStreamerOptions {
        detail: detail.then_some(streaming.detail),
        budget: streaming.budget,
        ..Default::default()
    };
    Ok(TerrainStreamer::new(layout.clone(), loader, options)?)
}

/// The view a camera at `eye` looking at `focus` selects with, in CSS pixels.
pub fn terrain_view(eye: Vec3, focus: Vec3, logical_height: f32, tolerance: f32, target_radius: f32) -> TerrainView {
    let half_fov = f64::from(TERRAIN_CONFIG.camera_controls.vertical_fov_radians) * 0.5;
    TerrainView {
        eye: eye.as_dvec3(),
        projection_scale: f64::from(logical_height.max(1.0)) / (2.0 * joy_engine::math::deterministic::tan(half_fov)),
        tolerance: f64::from(tolerance),
        target: focus.as_dvec3(),
        target_radius: f64::from(target_radius),
    }
}
