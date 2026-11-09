// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shadow planning: the shadow views a frame uses, which of them render (see [`super::cache`]), the casters each
//! rendering view draws, and the receiver uniforms.
//!
//! The key light casts cascades when it is lit and set to cast. Selected point lights set to cast take shadow slots in
//! importance order, up to the configured budget, when their range touches a caster (a light whose reach holds no
//! caster would see empty faces). Their faces are sized by how much of the view each light reaches and placed in the
//! atlas, shrinking the least important lights' faces when the atlas is full. Casters come from the scene's bounds
//! tree: each cascade queries its own volume, and each point light queries its sphere once, then culls only those
//! casters against each of its six faces. Views that keep a cached map query nothing.

use joy_core::math::{
    Mat4,
    Vec3,
};
use joy_render_types::{
    DirectionalLight,
    PointLight,
};

use crate::camera::CameraFrame;
use crate::culling::{
    ClipPlanes,
    Overlap,
    bounds_outside_view,
    sphere_touches_bounds,
};
use crate::draw_items::DrawItem;
use crate::mesh::Bounds;
use crate::scene::SceneProxies;
use crate::scene_batcher::casts_into_view;
use crate::shadows::cache::{
    CachedMap,
    PointShadowPlacement,
    PointShadowRequest,
    ShadowCache,
    ShadowInputs,
};
use crate::shadows::{
    AtlasTile,
    CASCADE_BLEND_FRACTION,
    CASCADE_TEXEL_BIAS,
    CascadeRaw,
    CasterVolumes,
    FAR_FADE_FRACTION,
    MIN_POINT_FACE_SIZE,
    POINT_FACES,
    POINT_SOURCE_RADIUS,
    PointShadowRaw,
    ShadowMaps,
    ShadowOptions,
    ShadowUniforms,
    cascade_splits,
    fit_cascades,
    fit_face_sizes,
    point_face_size,
    point_face_views,
    point_near_plane,
};

/// Where one shadow view renders.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShadowTarget {
    /// A directional cascade's layer.
    Cascade(usize),
    /// A point-light face's tile in the atlas.
    PointFace(AtlasTile),
}

/// One shadow view that renders this frame, and what it draws.
#[derive(Clone, Debug)]
pub(crate) struct ShadowView {
    /// World to the view's clip space.
    pub matrix: Mat4,
    pub target: ShadowTarget,
    /// World size of one map texel for a directional cascade; zero for point-light faces, whose texels vary.
    pub texel: f32,
    /// Slots of the scene proxies that cast into the view, ascending.
    pub casters: Vec<u32>,
    /// Whether the transient opaque triangles reach the view.
    pub transient: bool,
    /// The cached map this view renders into.
    pub map: CachedMap,
    /// The view's draws in order (caster batches, transient triangles, then terrain), filled once materials are
    /// prepared.
    pub items: Vec<DrawItem>,
}

/// Shadow views and receiver data prepared for one frame.
#[derive(Debug, Default)]
pub(crate) struct ShadowPlan {
    /// Views that render this frame, in pass order: cascades, then point-light faces.
    pub views: Vec<ShadowView>,
    /// Views receivers read this frame, rendered or cached: cascades plus six faces per shadowed point light.
    pub views_in_use: usize,
    pub cascades: usize,
    pub point_lights: usize,
    /// Per selected point light, its shadow slot (`Shadows.points` in shadow-receiver.wgsl), if it casts this frame.
    pub light_slots: Vec<Option<usize>>,
    /// Whether every face in use renders, so the atlas pass may clear the whole atlas instead of each tile it renders.
    pub clear_atlas: bool,
    /// Caster instances the maps kept from earlier frames hold.
    pub cached_casters: usize,
}

impl ShadowPlan {
    /// Whether any point-light face renders this frame.
    pub(crate) fn renders_point_faces(&self) -> bool {
        self.views.iter().any(|view| matches!(view.target, ShadowTarget::PointFace(_)))
    }
}

/// Everything that can cast into a shadow view this frame.
pub(crate) struct FrameCasters<'a> {
    /// The scene's refreshed proxies and their bounds tree.
    pub proxies: &'a SceneProxies,
    /// World bounds of the transient opaque triangles, if any.
    pub transient: Option<Bounds>,
    /// World bounds of each shadow-casting terrain instance.
    pub terrain: &'a [Bounds],
}

impl FrameCasters<'_> {
    fn is_empty(&self) -> bool {
        self.proxies.caster_count() == 0 && self.transient.is_none() && self.terrain.is_empty()
    }

    /// Whether any caster's bounds touch the sphere at `center` with `radius`.
    fn touch_sphere(&self, center: Vec3, radius: f32) -> bool {
        let touches = |bounds: &Bounds| sphere_touches_bounds(center, radius, *bounds);
        let mut found = self.transient.as_ref().is_some_and(touches) || self.terrain.iter().any(touches);
        if !found {
            let classify = |bounds: &Bounds| Overlap::reaching(touches(bounds));
            self.proxies.query(classify, |_, proxy, _| found |= proxy.casts && touches(&proxy.bounds));
        }
        found
    }

    /// Slots of the mesh casters whose bounds pass `reaches` exactly, ascending, into `slots`; `classify` prunes the
    /// search conservatively.
    fn mesh_casters(
        &self,
        classify: impl Fn(&Bounds) -> Overlap,
        reaches: impl Fn(&Bounds) -> bool,
        slots: &mut Vec<u32>,
    ) {
        slots.clear();
        self.proxies.query(classify, |index, proxy, overlap| {
            if proxy.casts && (overlap == Overlap::Inside || reaches(&proxy.bounds)) {
                slots.push(index);
            }
        });
        slots.sort_unstable();
    }
}

impl CasterVolumes for FrameCasters<'_> {
    fn visit(&self, reaches: &dyn Fn(&Bounds) -> bool, visit: &mut dyn FnMut(&Bounds)) {
        self.proxies.query(
            |bounds| Overlap::reaching(reaches(bounds)),
            |_, proxy, _| {
                if proxy.casts {
                    visit(&proxy.bounds);
                }
            },
        );
        for bounds in self.transient.iter().chain(self.terrain) {
            visit(bounds);
        }
    }
}

/// What one frame's shadow planning reads.
pub(crate) struct ShadowFrame<'a> {
    pub options: Option<ShadowOptions>,
    pub max_texture_dimension: u32,
    /// The directional light that lights and shadows surfaces, when valid.
    pub key: Option<DirectionalLight>,
    /// The frame's selected point lights, strongest first.
    pub points: &'a [PointLight],
    /// The stable (unjittered) camera.
    pub camera: &'a CameraFrame,
    pub render_size: (u32, u32),
    pub casters: FrameCasters<'a>,
    /// What the cache compares to decide which maps went stale.
    pub changes: ShadowInputs<'a>,
}

/// The renderer's shadow planner: the shadow-map cache and scratch lists reused every frame.
#[derive(Debug, Default)]
pub(crate) struct ShadowPlanner {
    cache: ShadowCache,
    requests: Vec<PointShadowRequest>,
    placements: Vec<PointShadowPlacement>,
    reached: Vec<u32>,
}

impl ShadowPlanner {
    /// Render every shadow map on the next frame (after the maps are reallocated, or a frame failed after planning).
    pub(crate) fn invalidate(&mut self) {
        self.cache.invalidate();
    }

    /// Record the caster instances each rendering view of `plan` drew (`casters`, in view order), and return the caster
    /// instances every map in use holds, rendered this frame or kept.
    pub(crate) fn record_casters(&mut self, plan: &ShadowPlan, casters: &[usize]) -> usize {
        for (view, count) in plan.views.iter().zip(casters) {
            self.cache.add_casters(view.map, *count);
        }
        plan.cached_casters + casters.iter().sum::<usize>()
    }

    /// Choose this frame's shadow views, decide which render, find the casters of each rendering view, and upload the
    /// rendering views' matrices and the receiver uniforms into `maps`.
    pub(crate) fn plan(
        &mut self,
        frame: &ShadowFrame<'_>,
        maps: &mut ShadowMaps,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> ShadowPlan {
        let mut plan = ShadowPlan {
            light_slots: vec![None; frame.points.len()],
            ..ShadowPlan::default()
        };
        self.cache.begin_frame(&frame.changes);
        let mut uniforms = ShadowUniforms::disabled();
        let Some(options) = frame.options.filter(|_| !frame.casters.is_empty()) else {
            self.cache.retire_cascades(0);
            self.cache.place_point_lights(&[], maps.allocated_atlas_size(), &mut self.placements);
            maps.write_uniforms(queue, &uniforms);
            return plan;
        };
        let quality = options.quality;
        uniforms.counts = [0.0, quality.blocker_sample_count() as f32, quality.filter_sample_count() as f32, 0.0];
        uniforms.parameters = [FAR_FADE_FRACTION, 0.0, 0.0, 0.0];

        self.plan_cascades(frame, &options, &mut plan, &mut uniforms);
        self.plan_point_lights(frame, &options, maps, device, &mut plan, &mut uniforms);

        let matrices: Vec<Mat4> = plan.views.iter().map(|view| view.matrix).collect();
        maps.write_casters(device, queue, &matrices);
        maps.write_uniforms(queue, &uniforms);
        plan
    }

    /// Directional cascades. Each cascade's volume reaches toward the light only as far as the casters above it, so
    /// casters outside every cascade (including those beyond the shadow distance) are never drawn.
    fn plan_cascades(
        &mut self,
        frame: &ShadowFrame<'_>,
        options: &ShadowOptions,
        plan: &mut ShadowPlan,
        uniforms: &mut ShadowUniforms,
    ) {
        let camera = frame.camera;
        let Some(light) = frame.key else {
            self.cache.retire_cascades(0);
            return;
        };
        let distance = options.shadow_distance.min(camera.far).max(camera.near);
        if !(light.casts_shadow && light.is_lit() && distance > camera.near) {
            self.cache.retire_cascades(0);
            return;
        }

        let count = options.quality.cascade_count();
        let splits = cascade_splits(camera.near, distance, count, options.cascade_split_lambda);
        let map_size = options.directional_map_size(frame.max_texture_dimension);
        let fits = fit_cascades(camera, light.direction, &splits, count, map_size, distance, &frame.casters);
        for (index, fit) in fits.iter().enumerate() {
            uniforms.cascades[index] = CascadeRaw {
                matrix: fit.matrix.to_cols_array_2d(),
                extent: [
                    splits[index],
                    fit.texel,
                    fit.depth_range,
                    options.normal_bias.max(fit.texel * CASCADE_TEXEL_BIAS),
                ],
            };
            if self.cache.cascade_renders(index, fit.matrix) {
                let matrix = fit.matrix;
                let mut casters = Vec::new();
                let planes = ClipPlanes::new(matrix);
                let classify = |bounds: &Bounds| planes.classify(bounds);
                frame.casters.mesh_casters(classify, |bounds| !bounds_outside_view(*bounds, matrix), &mut casters);
                let target = ShadowTarget::Cascade(index);
                plan.views.push(shadow_view(
                    matrix,
                    target,
                    CachedMap::Cascade(index),
                    fit.texel,
                    casters,
                    &frame.casters,
                ));
            } else {
                plan.cached_casters += self.cache.cascade_casters(index);
            }
        }
        self.cache.retire_cascades(fits.len());
        uniforms.counts[0] = fits.len() as f32;
        uniforms.directional = [distance, 1.0, light.angular_radius.tan(), CASCADE_BLEND_FRACTION];
        plan.cascades = fits.len();
        plan.views_in_use += fits.len();
    }

    /// Point lights: the most important casting lights that reach a caster, their face sizes fitted to the atlas, which
    /// grows (up to the quality's size) to what the frame asks for.
    fn plan_point_lights(
        &mut self,
        frame: &ShadowFrame<'_>,
        options: &ShadowOptions,
        maps: &mut ShadowMaps,
        device: &wgpu::Device,
        plan: &mut ShadowPlan,
        uniforms: &mut ShadowUniforms,
    ) {
        let largest_face = options.point_map_size(frame.max_texture_dimension);
        let mut shadowed: Vec<usize> = Vec::new();
        let mut face_sizes: Vec<u32> = Vec::new();
        for (index, light) in frame.points.iter().enumerate() {
            if shadowed.len() == options.point_light_budget() {
                break;
            }
            if light.casts_shadow && frame.casters.touch_sphere(light.position, light.range) {
                shadowed.push(index);
                face_sizes.push(point_face_size(
                    light.position,
                    light.range,
                    frame.camera,
                    frame.render_size.1,
                    largest_face,
                ));
            }
        }
        if !shadowed.is_empty() {
            maps.reserve_atlas(device, &face_sizes);
        }
        let atlas_size = maps.allocated_atlas_size();
        fit_face_sizes(&mut face_sizes, atlas_size, MIN_POINT_FACE_SIZE);
        shadowed.truncate(face_sizes.len());

        // Keep the tiles of lights whose cached faces still serve them; the rest are placed and render.
        self.requests.clear();
        self.requests.extend(shadowed.iter().zip(&face_sizes).map(|(index, face_size)| PointShadowRequest {
            position: frame.points[*index].position,
            range: frame.points[*index].range,
            face_size: *face_size,
        }));
        if !self.cache.place_point_lights(&self.requests, atlas_size, &mut self.placements) {
            shadowed.clear();
        }

        let texel = 1.0 / atlas_size as f32;
        for (slot, (light_index, placement)) in shadowed.iter().zip(&self.placements).enumerate() {
            let light = &frame.points[*light_index];
            let faces = placement.tiles;
            let origin = |face: usize| [faces[face].x as f32 * texel, faces[face].y as f32 * texel];
            let pair = |first: usize| {
                let [x0, y0] = origin(first);
                let [x1, y1] = origin(first + 1);
                [x0, y0, x1, y1]
            };
            let face_size = faces[0].size as f32;
            uniforms.points[slot] = PointShadowRaw {
                position_range: light.position.extend(light.range).to_array(),
                parameters: [point_near_plane(light.range), POINT_SOURCE_RADIUS, options.normal_bias, 1.0],
                tiles: [pair(0), pair(2), pair(4)],
                tile: [face_size * texel, face_size, 0.0, 0.0],
            };
            plan.light_slots[*light_index] = Some(slot);
            if !placement.render {
                plan.cached_casters += placement.cached_casters;
                continue;
            }

            // One sphere query finds what the light reaches; each of its six faces then culls only those.
            let (position, range) = (light.position, light.range);
            let touches = |bounds: &Bounds| sphere_touches_bounds(position, range, *bounds);
            frame.casters.mesh_casters(|bounds| Overlap::reaching(touches(bounds)), touches, &mut self.reached);
            for (face, matrix) in point_face_views(position, range).into_iter().enumerate() {
                let casters: Vec<u32> = self
                    .reached
                    .iter()
                    .copied()
                    .filter(|index| {
                        frame.casters.proxies.get(*index).is_some_and(|proxy| casts_into_view(proxy, matrix))
                    })
                    .collect();
                let target = ShadowTarget::PointFace(faces[face]);
                plan.views.push(shadow_view(matrix, target, CachedMap::PointLight(slot), 0.0, casters, &frame.casters));
            }
        }
        plan.point_lights = shadowed.len();
        plan.views_in_use += POINT_FACES * shadowed.len();
        plan.clear_atlas = self.placements.iter().all(|placement| placement.render);
        uniforms.counts[3] = plan.point_lights as f32;
    }
}

/// A rendering shadow view over `casters`, noting whether the transient triangles reach it.
fn shadow_view(
    matrix: Mat4,
    target: ShadowTarget,
    map: CachedMap,
    texel: f32,
    casters: Vec<u32>,
    frame_casters: &FrameCasters<'_>,
) -> ShadowView {
    ShadowView {
        matrix,
        target,
        texel,
        casters,
        transient: frame_casters.transient.is_some_and(|bounds| !bounds_outside_view(bounds, matrix)),
        map,
        items: Vec::new(),
    }
}
