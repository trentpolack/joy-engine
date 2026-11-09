// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Static shadow caching: which shadow maps keep the depth an earlier frame rendered, and which render again.
//!
//! A shadow map depends only on its light's view and the casters inside that view, so a frame re-renders a map only
//! when one of the two changed:
//! - **Point lights** keep their six atlas tiles while their position, range, and face size stay the same. Their faces
//!   render again when the light is new or moved (a different position or range), its face size changed (its reach
//!   grew or shrank on screen past a power of two, or the atlas filled up), the atlas grew or had to be repacked, or a
//!   caster within its range was added, removed, moved, or changed (its mesh, materials, or shadow policy, or the tint
//!   and opacity of an alpha-tested caster).
//! - **Directional cascades** render again when their matrix changes (they follow the camera, so a moving camera
//!   re-renders them most frames) or a caster inside the cascade changed.
//! - **Transient opaque triangles** are compared with the previous frame's; when they differ, every map their old or
//!   new bounds reach renders again.
//! - **Terrain** selects its level of detail from the camera, so whenever the camera or a terrain instance (or a scene
//!   node above one) changes, every map that reaches shadow-casting terrain renders again.
//! - **Everything** renders again on the first frame, for a different scene, after the shadow maps are reallocated,
//!   after a shading model registers (casters whose materials waited on it start drawing), after a frame fails to
//!   record, and when the scene's caster journal moved past this cache's cursor.
//!
//! Changes outside a light's reach (a crate moved across the level, a tint change on an opaque mesh) leave its maps
//! alone. `RendererDiagnostics::shadow_maps_rendered` counts the maps each frame rendered.

use joy_core::math::{
    Mat4,
    Vec3,
};

use crate::culling::{
    bounds_outside_view,
    sphere_touches_bounds,
};
use crate::frame::ColorVertex;
use crate::mesh::Bounds;
use crate::scene::SceneProxies;
use crate::shadows::{
    AtlasAllocator,
    AtlasTile,
    POINT_FACES,
};

/// What terrain shadow geometry depends on beyond the scene's terrain edits: terrain selects its level of detail from
/// the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TerrainShadowKey {
    /// The scene's terrain revision (`RenderScene::terrain_revision`).
    pub revision: u64,
    /// The stable camera's eye and focus, in world units.
    pub eye: Vec3,
    pub focus: Vec3,
    pub projection: Mat4,
    /// Viewport height in logical pixels, which scales terrain's screen-space error.
    pub logical_height: f32,
}

/// Everything a frame's shadow maps depend on besides the lights.
pub(crate) struct ShadowInputs<'a> {
    /// The scene's identity (`RenderScene::id`).
    pub scene: u64,
    /// The scene's refreshed proxies, for the caster journal.
    pub proxies: &'a SceneProxies,
    /// The frame's complete opaque transient triangles, which cast.
    pub transient: &'a [ColorVertex],
    /// Their world bounds, when there are any.
    pub transient_bounds: Option<Bounds>,
    pub terrain: TerrainShadowKey,
    /// World bounds of each shadow-casting terrain instance this frame.
    pub terrain_bounds: &'a [Bounds],
}

/// One shadowed point light's request: its position and range in world units, and its face size in texels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointShadowRequest {
    pub position: Vec3,
    pub range: f32,
    pub face_size: u32,
}

/// Where one shadowed point light's faces live this frame, and whether they render.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PointShadowPlacement {
    pub tiles: [AtlasTile; POINT_FACES],
    /// Whether the faces render this frame (otherwise they keep an earlier frame's depth).
    pub render: bool,
    /// Caster instances the cached faces hold, while they do not render.
    pub cached_casters: usize,
}

/// One cached shadow map (or a point light's six), for recording what it holds once it renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CachedMap {
    /// A cascade layer, by cascade index.
    Cascade(usize),
    /// A shadowed point light's faces, by its slot this frame.
    PointLight(usize),
}

/// A cascade layer's cached view.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CachedCascade {
    matrix: Mat4,
    /// Caster instances the layer holds.
    casters: usize,
}

/// The scene state cached maps were last brought up to date with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SceneCursor {
    scene: u64,
    revision: u64,
}

/// A point light holding atlas tiles.
#[derive(Clone, Copy, Debug)]
struct CachedLight {
    request: PointShadowRequest,
    tiles: [AtlasTile; POINT_FACES],
    /// Whether the tiles hold this light's depth.
    valid: bool,
    /// Caster instances its faces hold.
    casters: usize,
}

impl CachedLight {
    /// Whether this entry's tiles serve `request`: the same light (bit for bit) at the same face size.
    fn serves(&self, request: &PointShadowRequest) -> bool {
        self.request.face_size == request.face_size
            && self.request.range.to_bits() == request.range.to_bits()
            && self.request.position.to_array().map(f32::to_bits) == request.position.to_array().map(f32::to_bits)
    }
}

/// Where casters changed since the cached maps rendered.
#[derive(Debug, Default)]
struct StaleRegions {
    everything: bool,
    regions: Vec<Bounds>,
}

impl StaleRegions {
    fn clear(&mut self) {
        self.everything = false;
        self.regions.clear();
    }

    fn reaches_view(&self, view_projection: Mat4) -> bool {
        self.everything || self.regions.iter().any(|bounds| !bounds_outside_view(*bounds, view_projection))
    }

    fn reaches_sphere(&self, center: Vec3, radius: f32) -> bool {
        self.everything || self.regions.iter().any(|bounds| sphere_touches_bounds(center, radius, *bounds))
    }
}

/// The renderer's shadow-map cache: what each cascade layer and atlas tile holds, and where casters changed since.
#[derive(Debug)]
pub(crate) struct ShadowCache {
    cursor: Option<SceneCursor>,
    /// The view each cascade layer holds depth for, when valid.
    cascades: Vec<Option<CachedCascade>>,
    /// Shadowed point lights holding tiles, in last frame's importance order.
    lights: Vec<CachedLight>,
    allocator: AtlasAllocator,
    /// Last frame's opaque transient triangle positions and bounds.
    transient: Vec<[f32; 3]>,
    transient_bounds: Option<Bounds>,
    /// Last frame's terrain shadow inputs.
    terrain: Option<TerrainShadowKey>,
    terrain_bounds: Vec<Bounds>,
    /// This frame's stale regions, from [`begin_frame`](Self::begin_frame).
    stale: StaleRegions,
}

impl Default for ShadowCache {
    fn default() -> Self {
        Self {
            cursor: None,
            cascades: Vec::new(),
            lights: Vec::new(),
            allocator: AtlasAllocator::new(0),
            transient: Vec::new(),
            transient_bounds: None,
            terrain: None,
            terrain_bounds: Vec::new(),
            stale: StaleRegions::default(),
        }
    }
}

impl ShadowCache {
    /// Forget every cached map, so the next frame renders all of them: after the maps are reallocated or a frame fails
    /// after planning.
    pub(crate) fn invalidate(&mut self) {
        self.cursor = None;
        self.cascades.clear();
        self.lights.clear();
        self.allocator.reset(self.allocator.size());
    }

    /// Collect where casters changed since the previous frame: the scene's caster journal, transient triangles, and
    /// terrain. Call once per frame before asking which maps render.
    pub(crate) fn begin_frame(&mut self, inputs: &ShadowInputs<'_>) {
        self.stale.clear();

        // Scene casters, through the journal from this cache's cursor.
        let cursor = SceneCursor {
            scene: inputs.scene,
            revision: inputs.proxies.revision(),
        };
        let journal = self
            .cursor
            .filter(|previous| previous.scene == inputs.scene)
            .and_then(|previous| inputs.proxies.caster_changes_since(previous.revision));
        match journal {
            Some(changes) => {
                for change in changes {
                    self.stale.regions.extend(change.before);
                    self.stale.regions.extend(change.after);
                }
            }
            None => self.stale.everything = true,
        }
        self.cursor = Some(cursor);

        // Transient opaque triangles, compared position by position with last frame's.
        let same_transient = self.transient.len() == inputs.transient.len()
            && self.transient.iter().zip(inputs.transient).all(|(previous, vertex)| *previous == vertex.position);
        if !same_transient {
            self.stale.regions.extend(self.transient_bounds);
            self.stale.regions.extend(inputs.transient_bounds);
            self.transient.clear();
            self.transient.extend(inputs.transient.iter().map(|vertex| vertex.position));
            self.transient_bounds = inputs.transient_bounds;
        }

        // Terrain, whose shadow geometry follows the camera and its own edits.
        let terrain_changed =
            self.terrain != Some(inputs.terrain) || self.terrain_bounds.as_slice() != inputs.terrain_bounds;
        if terrain_changed {
            self.stale.regions.extend(self.terrain_bounds.iter().copied());
            self.stale.regions.extend(inputs.terrain_bounds.iter().copied());
            self.terrain = Some(inputs.terrain);
            self.terrain_bounds.clear();
            self.terrain_bounds.extend_from_slice(inputs.terrain_bounds);
        }
    }

    /// Whether cascade `index`, viewing through `matrix` this frame, renders: its layer holds another view's depth, or
    /// a change reached its volume. Records that the layer will hold this view.
    pub(crate) fn cascade_renders(&mut self, index: usize, matrix: Mat4) -> bool {
        if self.cascades.len() <= index {
            self.cascades.resize(index + 1, None);
        }
        let held = self.cascades[index].filter(|cascade| cascade.matrix == matrix);
        let renders = held.is_none() || self.stale.reaches_view(matrix);
        self.cascades[index] = Some(CachedCascade {
            matrix,
            casters: if renders { 0 } else { held.map_or(0, |cascade| cascade.casters) },
        });
        renders
    }

    /// Caster instances cascade `index` holds.
    pub(crate) fn cascade_casters(&self, index: usize) -> usize {
        self.cascades.get(index).copied().flatten().map_or(0, |cascade| cascade.casters)
    }

    /// Record that a map rendered this frame drew `casters` more caster instances, so frames that keep it can report
    /// what it holds.
    pub(crate) fn add_casters(&mut self, map: CachedMap, casters: usize) {
        let held = match map {
            CachedMap::Cascade(index) => {
                self.cascades.get_mut(index).and_then(Option::as_mut).map(|cascade| &mut cascade.casters)
            }
            CachedMap::PointLight(slot) => self.lights.get_mut(slot).map(|light| &mut light.casters),
        };
        if let Some(held) = held {
            *held += casters;
        }
    }

    /// Forget cascade layers from `count` on, which this frame does not render (fewer cascades, or none).
    pub(crate) fn retire_cascades(&mut self, count: usize) {
        self.cascades.truncate(count);
    }

    /// Place this frame's shadowed point lights (importance order) in an atlas of `atlas_size` texels, keeping the
    /// tiles of every light whose cached faces still serve it, and fill `placements` in request order. Lights that are
    /// new, resized, or reached by a change render; when the atlas grew, or new lights do not fit around the kept ones,
    /// every light is placed again and renders. Returns `false` (with `placements` empty) only if the requests do not
    /// fit even an empty atlas, which fitted face sizes always do.
    pub(crate) fn place_point_lights(
        &mut self,
        requests: &[PointShadowRequest],
        atlas_size: u32,
        placements: &mut Vec<PointShadowPlacement>,
    ) -> bool {
        placements.clear();
        if self.allocator.size() != atlas_size {
            self.lights.clear();
            self.allocator.reset(atlas_size);
        }

        // Keep each cached light that still serves a request; release the tiles of the rest.
        let mut kept: Vec<Option<CachedLight>> = vec![None; requests.len()];
        for cached in self.lights.drain(..) {
            let claim = requests
                .iter()
                .enumerate()
                .position(|(index, request)| kept[index].is_none() && cached.serves(request));
            match claim {
                Some(index) => kept[index] = Some(cached),
                None => {
                    for tile in cached.tiles {
                        self.allocator.release(tile);
                    }
                }
            }
        }

        // Place the new lights largest first around the kept ones; repack everything when one does not fit.
        let mut order: Vec<usize> = (0..requests.len()).filter(|index| kept[*index].is_none()).collect();
        order.sort_by(|left, right| requests[*right].face_size.cmp(&requests[*left].face_size));
        let mut repack = false;
        for index in order {
            match self.allocate_faces(requests[index].face_size) {
                Some(tiles) => {
                    kept[index] = Some(CachedLight {
                        request: requests[index],
                        tiles,
                        valid: false,
                        casters: 0,
                    });
                }
                None => {
                    repack = true;
                    break;
                }
            }
        }
        if repack {
            self.allocator.reset(atlas_size);
            let sizes: Vec<u32> = requests.iter().map(|request| request.face_size).collect();
            let Some(placed) = self.allocator.place_all(&sizes) else {
                self.allocator.reset(atlas_size);
                return false;
            };
            for (index, tiles) in placed.into_iter().enumerate() {
                kept[index] = Some(CachedLight {
                    request: requests[index],
                    tiles,
                    valid: false,
                    casters: 0,
                });
            }
        }

        // A light renders when its tiles hold nothing valid or a change reached its range.
        for cached in kept.into_iter().flatten() {
            let request = cached.request;
            let render = !cached.valid || self.stale.reaches_sphere(request.position, request.range);
            let casters = if render { 0 } else { cached.casters };
            placements.push(PointShadowPlacement {
                tiles: cached.tiles,
                render,
                cached_casters: casters,
            });
            self.lights.push(CachedLight {
                valid: true,
                casters,
                ..cached
            });
        }
        true
    }

    /// Six tiles of `face_size`, or `None` (releasing any taken) when they do not all fit.
    fn allocate_faces(&mut self, face_size: u32) -> Option<[AtlasTile; POINT_FACES]> {
        let mut tiles = [AtlasTile::default(); POINT_FACES];
        for face in 0..POINT_FACES {
            match self.allocator.allocate(face_size) {
                Some(tile) => tiles[face] = tile,
                None => {
                    for taken in &tiles[..face] {
                        self.allocator.release(*taken);
                    }
                    return None;
                }
            }
        }
        Some(tiles)
    }
}
