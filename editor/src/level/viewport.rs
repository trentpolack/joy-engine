// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The level viewport: the retained asset presentation, the orbit camera framed into the
//! viewport's region of the window, the editor grid, picking, and the move gizmo.
//!
//! The viewport draws through the engine renderer exactly like games and the tool editors: [`AssetScene`] presents primitives,
//! FORM meshes, lights, and per-entity particle effects in a [`RenderScene`]; the bounded [`EditorGrid`] is drawn as
//! translucent unlit ribbons. Every tab shares one window, so the engine's [`RegionCamera`] maps the camera's image onto
//! the viewport's rectangle (an off-axis projection) and the rest of the window stays covered by editor panels. Positions
//! are world meters, Y-up; screen points are logical pixels with a top-left origin in window coordinates.
//!
//! The level's `postProcess` volumes resolve at the editor camera's eye each frame, so authors see the look a player
//! would at that point; the scene effect preview's bloom choice, once made, layers on top.
//!
//! The viewport borrows authored snapshots and never edits the document: moves report a target position for the owner
//! to commit as one transaction, and gizmo drags preview through [`AssetScene::update`] until they finish.

use joy_engine::development::SceneEffectsControls;
use joy_engine::gameplay::{
    EntityDefinition,
    component_type,
};
use joy_engine::math::{
    Vec2,
    Vec3,
};
use joy_engine::particles::ParticleCameraBasis;
use joy_engine::rendering::{
    AssetScene,
    Camera,
    EditorGrid,
    EditorGridView,
    FrameInputs,
    OrbitCamera,
    PostProcessVolumes,
    RegionCamera,
    RenderScene,
    WorldRay,
    intersect_ray_plane,
    project_to_viewport,
    screen_ray,
};
use joy_engine::runtime::RenderTarget;

use super::constants::{
    CLEAR_COLOR,
    DEFAULT_CAMERA_DISTANCE,
    DEFAULT_CAMERA_PITCH,
    DEFAULT_CAMERA_YAW,
    GIZMO_HIT_PIXELS,
    GIZMO_SCREEN_FRACTION,
    LevelTool,
    VIEWPORT_AMBIENT,
};
use crate::interaction::viewport_input::{
    snap_point,
    snap_value,
};

/// A rectangle of the window in logical pixels with a top-left origin.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ViewRegion {
    /// Top-left corner.
    pub origin: Vec2,
    /// Width and height.
    pub size: Vec2,
}

impl ViewRegion {
    /// Whether `point` lies inside the region.
    pub fn contains(&self, point: Vec2) -> bool {
        point.cmpge(self.origin).all() && point.cmplt(self.origin + self.size).all()
    }

    /// The region's center.
    pub fn center(&self) -> Vec2 {
        self.origin + (self.size * 0.5)
    }
}

/// A move-gizmo handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GizmoHandle {
    /// Along world X.
    X,
    /// Along world Y.
    Y,
    /// Along world Z.
    Z,
    /// In the horizontal plane through the entity.
    Plane,
}

impl GizmoHandle {
    /// The world direction an axis handle moves along; the plane handle has none.
    pub fn axis(self) -> Option<Vec3> {
        match self {
            Self::X => Some(Vec3::X),
            Self::Y => Some(Vec3::Y),
            Self::Z => Some(Vec3::Z),
            Self::Plane => None,
        }
    }
}

/// One projected gizmo handle for drawing, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GizmoLine {
    /// Which handle.
    pub handle: GizmoHandle,
    /// The entity's projected position.
    pub start: Vec2,
    /// The projected handle tip (for the plane handle, the far corner of its square).
    pub end: Vec2,
}

/// A drag in progress on a gizmo handle.
#[derive(Clone, Debug)]
struct GizmoDrag {
    handle: GizmoHandle,
    entity: String,
    start_position: Vec3,
    // Axis parameter or plane point where the drag began.
    anchor: Vec3,
    preview: Vec3,
}

/// Owns the viewport's scene, presentation, camera, and grid; borrows authored snapshots.
pub struct LevelViewport {
    // Retained presentation and its scene. The asset scene's instances live in `scene`.
    assets: AssetScene,
    scene: RenderScene,
    post_process: PostProcessVolumes,

    // Camera and view aids.
    /// The orbit camera.
    pub camera: OrbitCamera,
    grid: EditorGrid,
    /// Whether the grid is drawn (editor state only).
    pub show_grid: bool,
    /// Grid snapping for placement and moves in meters; zero disables it.
    pub snap: f32,
    /// The active manipulation tool.
    pub tool: LevelTool,
    /// Temporary scene effect previews (never saved), applied when the viewport renders.
    pub effects: SceneEffectsControls,

    // Window placement, refreshed by the UI every frame.
    /// The viewport's rectangle in the window.
    pub region: ViewRegion,
    /// The window size in logical pixels.
    pub viewport_size: Vec2,

    // Interaction and diagnostics.
    drag: Option<GizmoDrag>,
    selected: Option<String>,
    grid_spacing: Option<f64>,
}

impl Default for LevelViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl LevelViewport {
    /// An empty viewport with the editor's initial camera.
    pub fn new() -> Self {
        Self {
            assets: AssetScene::new(),
            scene: RenderScene::new(),
            post_process: PostProcessVolumes::new(),
            camera: initial_camera(),
            grid: EditorGrid::new(),
            show_grid: true,
            snap: 1.0,
            tool: LevelTool::Select,
            effects: SceneEffectsControls::new(),
            region: ViewRegion::default(),
            viewport_size: Vec2::ZERO,
            drag: None,
            selected: None,
            grid_spacing: None,
        }
    }

    /// Present `entities` (authored, with template links) with `selected` highlighted, reading linked objects, FORM
    /// meshes, and effects through `read`. A failure keeps the previous presentation and returns its message; an
    /// invalid post-process volume keeps the previous volumes.
    pub fn sync(
        &mut self,
        entities: &[EntityDefinition],
        selected: Option<&str>,
        read: impl FnMut(&str) -> Result<String, String>,
    ) -> Result<(), String> {
        self.drag = None;
        self.selected = selected.map(str::to_owned);
        self.assets.sync(&mut self.scene, entities, selected, read).map_err(|error| error.to_string())?;
        self.post_process.sync(self.assets.entities()).map_err(|error| error.to_string())
    }

    /// Re-tint the displayed presentation for a new selection without reading assets.
    pub fn select(&mut self, selected: Option<&str>) {
        self.selected = selected.map(str::to_owned);
        let entities = self.assets.entities().to_vec();
        let _ = self.assets.update(&mut self.scene, &entities, selected);
    }

    /// The displayed entities with linked objects resolved.
    pub fn entities(&self) -> &[EntityDefinition] {
        self.assets.entities()
    }

    /// The presentation, for meshes, effects, and lights.
    pub fn assets(&self) -> &AssetScene {
        &self.assets
    }

    /// Number of drawn mesh instances.
    pub fn instance_count(&self) -> usize {
        self.scene.len()
    }

    /// Minor grid spacing in meters from the last frame, or `None` when the grid is hidden.
    pub fn grid_spacing(&self) -> Option<f64> {
        self.grid_spacing
    }

    /// Advance attached effects; failures are reported through [`effect_error`](Self::effect_error).
    pub fn step(&mut self, seconds: f64) {
        self.assets.step(seconds);
    }

    /// The most recent effect failure since the last sync.
    pub fn effect_error(&self) -> Option<&str> {
        Some(self.assets.error()).filter(|error| !error.is_empty())
    }

    /// Frame the selected entity, or every entity without a selection.
    pub fn focus(&mut self, selected: Option<&str>) {
        let entities = self.assets.entities();
        let selection: Vec<&EntityDefinition> =
            match entities.iter().find(|entity| Some(entity.id.as_str()) == selected) {
                Some(entity) => vec![entity],
                None => entities.iter().collect(),
            };
        let mut points = Vec::with_capacity(selection.len() * 2);
        for entity in selection {
            let size = dimensions(entity, self.bounds(&entity.id));
            for sign in [-1.0, 1.0] {
                points.push(entity.transform.position + (size * sign * 0.5));
            }
        }
        self.camera.frame_positions(&points);
        self.camera.distance = self.camera.distance.max(6.0);
    }

    /// Restore the initial camera without changing the document.
    pub fn reset_camera(&mut self) {
        let snap = self.snap;
        self.camera = initial_camera();
        self.snap = snap;
    }

    /// The camera mapped onto the viewport region.
    pub fn region_camera(&self) -> RegionCamera<'_, OrbitCamera> {
        RegionCamera::new(&self.camera, self.viewport_size, self.region.origin, self.region.size)
    }

    /// The world ray through a window point.
    pub fn screen_ray(&self, point: Vec2) -> Option<WorldRay> {
        let aspect = if self.viewport_size.y > 0.0 { self.viewport_size.x / self.viewport_size.y } else { 1.0 };
        let view_projection = self.region_camera().camera_frame(aspect).view_projection();
        screen_ray(point, view_projection, self.viewport_size)
    }

    /// Project a world position to window coordinates.
    pub fn project(&self, position: Vec3) -> Option<Vec2> {
        let aspect = if self.viewport_size.y > 0.0 { self.viewport_size.x / self.viewport_size.y } else { 1.0 };
        let view_projection = self.region_camera().camera_frame(aspect).view_projection();
        project_to_viewport(position, view_projection, self.viewport_size)
    }

    /// The nearest displayed entity under a window point.
    pub fn pick(&self, point: Vec2) -> Option<String> {
        let ray = self.screen_ray(point)?;
        pick_entity(self.assets.entities(), |id| self.bounds(id), &ray)
    }

    /// Where a window point's ray meets the ground (Y = 0), snapped to the grid.
    pub fn pick_ground(&self, point: Vec2) -> Option<Vec3> {
        let hit = intersect_ray_plane(self.screen_ray(point)?, Vec3::Y, 0.0)?;
        Some(snap_point(hit, self.snap))
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Move gizmo
    // -----------------------------------------------------------------------------------------------------------------

    /// The selected entity's projected gizmo handles, in move mode; empty otherwise.
    pub fn gizmo_lines(&self) -> Vec<GizmoLine> {
        if self.tool != LevelTool::Move {
            return Vec::new();
        }
        let Some(position) = self.gizmo_position() else {
            return Vec::new();
        };
        let length = self.camera.distance * GIZMO_SCREEN_FRACTION;
        let Some(start) = self.project(position) else {
            return Vec::new();
        };
        let mut lines = Vec::with_capacity(4);
        for handle in [GizmoHandle::X, GizmoHandle::Y, GizmoHandle::Z] {
            let axis = handle.axis().unwrap_or(Vec3::X);
            if let Some(end) = self.project(position + (axis * length)) {
                lines.push(GizmoLine {
                    handle,
                    start,
                    end,
                });
            }
        }
        if let Some(end) = self.project(position + (Vec3::new(1.0, 0.0, 1.0) * length * 0.3)) {
            lines.push(GizmoLine {
                handle: GizmoHandle::Plane,
                start,
                end,
            });
        }
        lines
    }

    /// The handle under a window point, if any.
    pub fn gizmo_hit(&self, point: Vec2) -> Option<GizmoHandle> {
        let lines = self.gizmo_lines();
        // The plane square sits at the origin of the axes, so test it first.
        if let Some(plane) = lines.iter().find(|line| line.handle == GizmoHandle::Plane) {
            let minimum = plane.start.min(plane.end);
            let maximum = plane.start.max(plane.end);
            if point.cmpge(minimum).all() && point.cmple(maximum).all() {
                return Some(GizmoHandle::Plane);
            }
        }
        lines
            .iter()
            .filter(|line| line.handle != GizmoHandle::Plane)
            .map(|line| (line.handle, distance_to_segment(point, line.start, line.end)))
            .filter(|(_, distance)| *distance <= GIZMO_HIT_PIXELS)
            .min_by(|left, right| left.1.total_cmp(&right.1))
            .map(|(handle, _)| handle)
    }

    /// Start dragging `handle` of the selected entity from a window point; returns whether a drag began.
    pub fn begin_gizmo(&mut self, handle: GizmoHandle, point: Vec2) -> bool {
        let (Some(entity), Some(position)) = (self.selected.clone(), self.gizmo_position()) else {
            return false;
        };
        let Some(anchor) = self.drag_anchor(handle, position, point) else {
            return false;
        };
        self.drag = Some(GizmoDrag {
            handle,
            entity,
            start_position: position,
            anchor,
            preview: position,
        });
        true
    }

    /// Continue a drag to a window point and preview the moved entity; returns its snapped position.
    pub fn drag_gizmo(&mut self, point: Vec2) -> Option<Vec3> {
        let drag = self.drag.clone()?;
        let current = self.drag_anchor(drag.handle, drag.start_position, point)?;
        let mut position = drag.start_position;
        match drag.handle.axis() {
            Some(axis) => {
                let index = axis.to_array().iter().position(|value| *value == 1.0).unwrap_or(0);
                let moved = drag.start_position[index] + (current.x - drag.anchor.x);
                position[index] = snap_value(moved, self.snap);
            }
            None => {
                let moved = drag.start_position + (current - drag.anchor);
                position.x = snap_value(moved.x, self.snap);
                position.z = snap_value(moved.z, self.snap);
            }
        }
        if let Some(active) = self.drag.as_mut() {
            active.preview = position;
        }
        let mut entities = self.assets.entities().to_vec();
        if let Some(entity) = entities.iter_mut().find(|entity| entity.id == drag.entity) {
            entity.transform.position = position;
        }
        let selected = self.selected.clone();
        let _ = self.assets.update(&mut self.scene, &entities, selected.as_deref());
        Some(position)
    }

    /// Finish a drag: the dragged entity and its final position when it moved.
    pub fn end_gizmo(&mut self) -> Option<(String, Vec3)> {
        let drag = self.drag.take()?;
        (drag.preview != drag.start_position).then_some((drag.entity, drag.preview))
    }

    /// Abandon a drag and restore the displayed position.
    pub fn cancel_gizmo(&mut self) {
        if self.drag.take().is_some() {
            let selected = self.selected.clone();
            self.select(selected.as_deref());
        }
    }

    /// Whether a gizmo drag is in progress.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The selected entity's displayed position, while it exists.
    fn gizmo_position(&self) -> Option<Vec3> {
        let selected = self.selected.as_deref()?;
        if let Some(drag) = &self.drag {
            return Some(drag.preview);
        }
        self.assets.entities().iter().find(|entity| entity.id == selected).map(|entity| entity.transform.position)
    }

    /// For an axis handle, the parameter along the axis nearest the point's ray (in `x`); for the plane handle, where
    /// the ray meets the horizontal plane through the entity.
    fn drag_anchor(&self, handle: GizmoHandle, position: Vec3, point: Vec2) -> Option<Vec3> {
        let ray = self.screen_ray(point)?;
        match handle.axis() {
            Some(axis) => closest_axis_parameter(position, axis, &ray).map(|parameter| Vec3::new(parameter, 0.0, 0.0)),
            None => intersect_ray_plane(ray, Vec3::Y, -position.y),
        }
    }

    /// The compiled mesh bounds presenting `entity_id`, if any.
    fn bounds(&self, entity_id: &str) -> Option<(Vec3, Vec3)> {
        self.assets.mesh(entity_id).map(|mesh| {
            let bounds = mesh.bounds();
            (bounds.min, bounds.max)
        })
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Rendering
    // -----------------------------------------------------------------------------------------------------------------

    /// Draw the presentation, grid, and particles into the frame target.
    pub fn render(&mut self, target: &mut RenderTarget<'_>) -> Result<(), String> {
        self.effects.apply(target.renderer());
        let basis = self.camera.basis();
        let post_process = self.effects.with_bloom_choice(self.post_process.resolve(basis.eye));
        target.renderer().set_post_process(post_process);
        let grid: Vec<_> = if self.show_grid && self.region.size.y > 0.0 {
            let view = EditorGridView {
                target: self.camera.target.to_array().map(f64::from),
                backward: basis.backward.to_array().map(f64::from),
                distance: f64::from(self.camera.distance),
                viewport_height_pixels: f64::from(self.region.size.y.max(1.0)),
                height: 0.0,
            };
            match self.grid.frame(&view) {
                Ok(frame) => {
                    self.grid_spacing = Some(frame.spacing);
                    frame.triangles.clone()
                }
                Err(_) => Vec::new(),
            }
        } else {
            self.grid_spacing = None;
            Vec::new()
        };
        let particle_basis = ParticleCameraBasis {
            right: basis.right.to_array().map(f64::from),
            up: basis.up.to_array().map(f64::from),
            backward: basis.backward.to_array().map(f64::from),
        };
        let batches = self.assets.particle_batches(&particle_basis).unwrap_or_default();
        let draws = self.assets.transparent_draws(&batches).unwrap_or_default();
        let lights = self.assets.lights();
        let inputs = FrameInputs {
            lights: &lights,
            ambient: Some(VIEWPORT_AMBIENT),
            clear_color: CLEAR_COLOR,
            transparent_triangles: &grid,
            transparent_draws: &draws,
            ..Default::default()
        };
        let camera = RegionCamera::new(&self.camera, self.viewport_size, self.region.origin, self.region.size);
        target.draw(&self.scene, &camera, &inputs).map_err(|error| error.to_string())
    }
}

/// The editor's initial camera: 26 meters out at a 0.7 radian pitch.
fn initial_camera() -> OrbitCamera {
    OrbitCamera {
        target: Vec3::ZERO,
        yaw: DEFAULT_CAMERA_YAW,
        pitch: DEFAULT_CAMERA_PITCH,
        radius: 10.0,
        distance: DEFAULT_CAMERA_DISTANCE,
        ..Default::default()
    }
}

/// Pick the nearest box or sphere primitive (or mesh bounds) hit by `ray`, testing in each entity's local frame.
///
/// `bounds` returns a compiled mesh's local bounds for asset-backed renders. Entities without a `render` component
/// are pickable as spheres when they carry an `effect`.
pub fn pick_entity(
    entities: &[EntityDefinition],
    bounds: impl Fn(&str) -> Option<(Vec3, Vec3)>,
    ray: &WorldRay,
) -> Option<String> {
    let mut closest = f32::INFINITY;
    let mut selected = None;
    for entity in entities {
        let render = entity.components.get(component_type::RENDER);
        let has_asset = render
            .and_then(|record| record.get("asset"))
            .is_some_and(|asset| asset.as_str().is_some_and(|path| !path.is_empty()));
        let shape = if has_asset {
            Some("box")
        } else {
            match render.and_then(|record| record.get("shape")).and_then(|shape| shape.as_str()) {
                Some(shape) => Some(shape),
                None if entity.components.contains_key(component_type::EFFECT) => Some("sphere"),
                None => None,
            }
        };
        let sphere = match shape {
            Some("box") => false,
            Some("sphere") => true,
            _ => continue,
        };
        let mesh_bounds = bounds(&entity.id);
        let size = dimensions(entity, mesh_bounds);
        if size.cmpeq(Vec3::ZERO).any() {
            continue;
        }
        let (minimum, maximum) = mesh_bounds.unwrap_or((Vec3::ZERO, Vec3::ZERO));
        let center = (minimum + maximum) * 0.5;
        let render_size = render.and_then(|record| read_vector(record.get("size"))).unwrap_or(Vec3::ONE);
        let transform = &entity.transform;
        let local_origin = inverse_rotate(ray.origin - transform.position, transform.rotation);
        let origin = (local_origin - (center * transform.scale * render_size)) / size;
        let direction = inverse_rotate(ray.direction, transform.rotation) / size;

        let mut distance = f32::INFINITY;
        if sphere {
            let a = direction.dot(direction);
            let b = origin.dot(direction);
            let c = origin.dot(origin) - 0.25;
            let discriminant = (b * b) - (a * c);
            if discriminant >= 0.0 {
                distance = (-b - discriminant.sqrt()) / a;
                if distance < 0.0 {
                    distance = (-b + discriminant.sqrt()) / a;
                }
            }
        } else {
            let mut near = 0.0_f32;
            let mut far = f32::INFINITY;
            for axis in 0..3 {
                if direction[axis].abs() < 1e-9 {
                    if origin[axis].abs() > 0.5 {
                        far = -1.0;
                    }
                    continue;
                }
                let first = (-0.5 - origin[axis]) / direction[axis];
                let second = (0.5 - origin[axis]) / direction[axis];
                near = near.max(first.min(second));
                far = far.min(first.max(second));
            }
            if far >= near {
                distance = near;
            }
        }
        if distance >= 0.0 && distance < closest {
            closest = distance;
            selected = Some(entity.id.clone());
        }
    }
    selected
}

/// Full primitive dimensions after entity scale, in world meters; mesh bounds multiply the render size.
pub fn dimensions(entity: &EntityDefinition, bounds: Option<(Vec3, Vec3)>) -> Vec3 {
    let render = entity.components.get(component_type::RENDER);
    let size = render.and_then(|record| read_vector(record.get("size"))).unwrap_or(Vec3::ONE);
    let (minimum, maximum) = bounds.unwrap_or((Vec3::ZERO, Vec3::ONE));
    entity.transform.scale * size * (maximum - minimum).max(Vec3::splat(0.01))
}

/// Undo an XYZ Euler rotation (applied X, then Y, then Z), preserving ray parameter distance.
fn inverse_rotate(vector: Vec3, rotation: Vec3) -> Vec3 {
    let (mut x, mut y, mut z) = (vector.x, vector.y, vector.z);
    let (rx, ry, rz) = (rotation.x, rotation.y, rotation.z);
    (x, y) = ((x * rz.cos()) + (y * rz.sin()), (-x * rz.sin()) + (y * rz.cos()));
    (x, z) = ((x * ry.cos()) - (z * ry.sin()), (x * ry.sin()) + (z * ry.cos()));
    (y, z) = ((y * rx.cos()) + (z * rx.sin()), (-y * rx.sin()) + (z * rx.cos()));
    Vec3::new(x, y, z)
}

/// Three finite numbers as a vector.
fn read_vector(value: Option<&joy_engine::serde_json::Value>) -> Option<Vec3> {
    let items = value?.as_array()?;
    if items.len() != 3 {
        return None;
    }
    let numbers: Vec<f32> = items.iter().filter_map(|item| item.as_f64()).map(|item| item as f32).collect();
    (numbers.len() == 3 && numbers.iter().all(|number| number.is_finite()))
        .then(|| Vec3::new(numbers[0], numbers[1], numbers[2]))
}

/// The parameter `t` of the point on the line `origin + t * axis` nearest `ray`, or `None` when they are parallel.
fn closest_axis_parameter(origin: Vec3, axis: Vec3, ray: &WorldRay) -> Option<f32> {
    let offset = origin - ray.origin;
    let a = axis.dot(axis);
    let b = axis.dot(ray.direction);
    let c = ray.direction.dot(ray.direction);
    let d = axis.dot(offset);
    let e = ray.direction.dot(offset);
    let denominator = (a * c) - (b * b);
    if denominator.abs() < 1e-6 {
        return None;
    }
    Some(((b * e) - (c * d)) / denominator)
}

/// Distance in pixels from `point` to the segment `start`–`end`.
fn distance_to_segment(point: Vec2, start: Vec2, end: Vec2) -> f32 {
    let segment = end - start;
    let length = segment.length_squared();
    if length <= f32::EPSILON {
        return point.distance(start);
    }
    let t = ((point - start).dot(segment) / length).clamp(0.0, 1.0);
    point.distance(start + (segment * t))
}
