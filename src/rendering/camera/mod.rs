// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Cameras, projection, and picking.
//!
//! Matrices are column-major, right-handed, and use zero-to-one clip depth on every backend (wgpu normalizes WebGL's
//! clip space), so the renderer never needs a WebGPU/WebGL flag. Viewport coordinates are logical (CSS) pixels with a
//! top-left origin; display density changes only drawing-buffer resolution, never the view.

mod top_down_camera;

pub use top_down_camera::{TopDownCamera, sort_top_down_transparent_triangles, top_down_orthographic};

use glam::{Mat4, Vec2, Vec3, Vec4};

/// Smallest automatic near plane in world units.
const MINIMUM_NEAR_PLANE: f32 = 0.001;

/// Immutable camera matrices for one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFrame {
    /// World-to-view matrix (camera looks along -Z).
    pub view: Mat4,
    /// View-to-clip perspective matrix with zero-to-one depth.
    pub projection: Mat4,
    /// Eye position in world units.
    pub eye: Vec3,
    /// Point the camera is framing; shadow fitting centers on it.
    pub focus: Vec3,
    /// Near clip distance in world units.
    pub near: f32,
    /// Far clip distance in world units.
    pub far: f32,
}

impl CameraFrame {
    /// Combined world-to-clip matrix.
    pub fn view_projection(&self) -> Mat4 {
        self.projection * self.view
    }
}

/// Any camera the renderer can draw from.
pub trait Camera {
    /// Produce this frame's matrices for a logical viewport aspect ratio (width / height).
    fn camera_frame(&self, aspect: f32) -> CameraFrame;
}

/// Orthonormal basis and eye position of an orbit camera, in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraBasis {
    /// Unit vector from the target toward the eye.
    pub backward: Vec3,
    /// Unit screen-right vector.
    pub right: Vec3,
    /// Unit screen-up vector.
    pub up: Vec3,
    /// Eye position in world units.
    pub eye: Vec3,
}

/// Y-up orbit camera matching the TypeScript `OrbitCamera`. Distances are world units; angles are radians.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrbitCamera {
    /// Point the camera orbits and looks at.
    pub target: Vec3,
    /// Eye distance from the target.
    pub distance: f32,
    /// Framed content radius; drives the automatic far plane.
    pub radius: f32,
    /// Heading around +Y; zero places the eye on +Z.
    pub yaw: f32,
    /// Elevation above the horizon.
    pub pitch: f32,
    /// Vertical perspective field of view.
    pub vertical_fov_radians: f32,
    /// Authored near clip; `None` derives it from the distance.
    pub near_clip: Option<f32>,
    /// Authored far clip; `None` derives it from distance and radius.
    pub far_clip: Option<f32>,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 32.0,
            radius: 10.0,
            yaw: 0.75,
            pitch: 0.55,
            vertical_fov_radians: std::f32::consts::FRAC_PI_4,
            near_clip: None,
            far_clip: None,
        }
    }
}

impl OrbitCamera {
    /// World-space basis vectors and eye position for the current orbit.
    pub fn basis(&self) -> CameraBasis {
        let backward =
            Vec3::new(self.pitch.cos() * self.yaw.sin(), self.pitch.sin(), self.pitch.cos() * self.yaw.cos());
        let right = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let up = backward.cross(right);
        CameraBasis { backward, right, up, eye: self.target + (backward * self.distance) }
    }

    /// Near clip distance in world units.
    pub fn near_plane(&self) -> f32 {
        self.near_clip.unwrap_or_else(|| MINIMUM_NEAR_PLANE.max(self.distance / 1000.0))
    }

    /// Far clip distance in world units.
    pub fn far_plane(&self) -> f32 {
        self.far_clip.unwrap_or(self.distance + (self.radius * 100.0))
    }

    /// World-to-view matrix built from the yaw-derived basis, which stays well-defined at any pitch.
    pub fn view_matrix(&self) -> Mat4 {
        let basis = self.basis();
        Mat4::from_cols(
            Vec4::new(basis.right.x, basis.up.x, basis.backward.x, 0.0),
            Vec4::new(basis.right.y, basis.up.y, basis.backward.y, 0.0),
            Vec4::new(basis.right.z, basis.up.z, basis.backward.z, 0.0),
            Vec4::new(-basis.right.dot(basis.eye), -basis.up.dot(basis.eye), -basis.backward.dot(basis.eye), 1.0),
        )
    }

    /// Perspective projection with zero-to-one depth for a logical aspect ratio.
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        perspective(self.vertical_fov_radians, aspect.max(0.0001), self.near_plane(), self.far_plane())
    }

    /// Fit packed world positions: centers the target, sets the radius, and places the eye at 2.8 radii.
    /// An empty slice frames a default ten-unit volume.
    pub fn frame_positions(&mut self, positions: &[Vec3]) {
        let (minimum, maximum) = if positions.is_empty() {
            (Vec3::splat(-5.0), Vec3::splat(5.0))
        } else {
            positions.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |bounds, point| {
                (bounds.0.min(*point), bounds.1.max(*point))
            })
        };
        self.target = (minimum + maximum) * 0.5;
        self.radius = ((maximum - minimum) * 0.5).length().max(0.1);
        self.distance = self.radius * 2.8;
    }
}

impl Camera for OrbitCamera {
    fn camera_frame(&self, aspect: f32) -> CameraFrame {
        let basis = self.basis();
        CameraFrame {
            view: self.view_matrix(),
            projection: self.projection_matrix(aspect),
            eye: basis.eye,
            focus: self.target,
            near: self.near_plane(),
            far: self.far_plane(),
        }
    }
}

/// Any camera with its image re-centered by a lens shift (an off-axis projection), so a scene framed for part of the
/// viewport stays centered there, for example beside a side panel, without changing perspective or field of view.
///
/// Culling, shadows, picking ([`screen_ray`] through the shifted matrices), and scene effects all read the shifted
/// projection, so drawing through this camera matches rendering into the uncovered part of the canvas alone.
#[derive(Clone, Copy, Debug)]
pub struct ShiftedCamera<'a, C: Camera> {
    /// The camera whose image is shifted; borrowed for the frame.
    pub camera: &'a C,
    /// Image-center offset in normalized device units (+X right, +Y up); `-0.5` moves the center a quarter of the
    /// viewport's width to the left.
    pub shift: Vec2,
}

impl<'a, C: Camera> ShiftedCamera<'a, C> {
    /// Center `camera`'s image between interface insets at the left and right edges of a viewport, all in logical
    /// pixels. An empty viewport leaves the image unshifted.
    pub fn between_insets(camera: &'a C, viewport_width: f32, left_inset: f32, right_inset: f32) -> Self {
        let shift = if viewport_width > 0.0 { (left_inset - right_inset) / viewport_width } else { 0.0 };
        Self { camera, shift: Vec2::new(shift, 0.0) }
    }
}

impl<C: Camera> Camera for ShiftedCamera<'_, C> {
    fn camera_frame(&self, aspect: f32) -> CameraFrame {
        let frame = self.camera.camera_frame(aspect);
        // Translating clip space by `shift * w` moves every projected point by `shift` in normalized device units.
        let lens = Mat4::from_translation(self.shift.extend(0.0));
        CameraFrame { projection: lens * frame.projection, ..frame }
    }
}

/// Any camera drawn into a rectangular region of a larger target, as if the region were its own canvas: the camera's
/// projection is built for the region's aspect ratio, then scaled and offset into the region. Tools use it for a boxed
/// preview between panels, toolbars, and timelines, where [`ShiftedCamera`] only re-centers a full-height image.
///
/// Positions are logical pixels with a top-left origin. Geometry outside the camera's frustum can still land on the
/// target outside the region, so draw opaque interface over the rest; culling, shadows, and picking ([`screen_ray`]
/// with the whole viewport) read the adjusted projection. A degenerate region leaves the camera unchanged.
#[derive(Clone, Copy, Debug)]
pub struct RegionCamera<'a, C: Camera> {
    /// The camera framed into the region; borrowed for the frame.
    pub camera: &'a C,
    /// The whole target's logical size.
    pub viewport_size: Vec2,
    /// The region's top-left corner in logical pixels.
    pub region_min: Vec2,
    /// The region's logical size.
    pub region_size: Vec2,
}

impl<'a, C: Camera> RegionCamera<'a, C> {
    /// Frame `camera` into the region at `region_min` with `region_size` inside a `viewport_size` target.
    pub fn new(camera: &'a C, viewport_size: Vec2, region_min: Vec2, region_size: Vec2) -> Self {
        Self { camera, viewport_size, region_min, region_size }
    }
}

impl<C: Camera> Camera for RegionCamera<'_, C> {
    fn camera_frame(&self, aspect: f32) -> CameraFrame {
        let valid = self.viewport_size.x > 0.0
            && self.viewport_size.y > 0.0
            && self.region_size.x > 0.0
            && self.region_size.y > 0.0
            && self.viewport_size.is_finite()
            && self.region_size.is_finite()
            && self.region_min.is_finite();
        if !valid {
            return self.camera.camera_frame(aspect);
        }
        let frame = self.camera.camera_frame(self.region_size.x / self.region_size.y);
        // Scaling clip x and y by the region's share of the target shrinks the image into a region-sized box; the
        // translation (times w) moves the box's center to the region's center in normalized device units.
        let scale = self.region_size / self.viewport_size;
        let center = self.region_min + (self.region_size * 0.5);
        let offset =
            Vec2::new(((center.x / self.viewport_size.x) * 2.0) - 1.0, 1.0 - ((center.y / self.viewport_size.y) * 2.0));
        let lens = Mat4::from_translation(offset.extend(0.0)) * Mat4::from_scale(scale.extend(1.0));
        CameraFrame { projection: lens * frame.projection, ..frame }
    }
}

/// Right-handed perspective projection with zero-to-one depth, matching the TypeScript camera's WebGPU matrix.
pub fn perspective(vertical_fov_radians: f32, aspect: f32, near: f32, far: f32) -> Mat4 {
    let scale = 1.0 / (vertical_fov_radians * 0.5).tan();
    let depth = far / (near - far);
    let offset = (far * near) / (near - far);
    Mat4::from_cols(
        Vec4::new(scale / aspect, 0.0, 0.0, 0.0),
        Vec4::new(0.0, scale, 0.0, 0.0),
        Vec4::new(0.0, 0.0, depth, -1.0),
        Vec4::new(0.0, 0.0, offset, 0.0),
    )
}

/// Right-handed orthographic projection with zero-to-one depth and a Y-up clip space.
pub fn orthographic(left: f32, right: f32, bottom: f32, top: f32, near: f32, far: f32) -> Mat4 {
    let width = right - left;
    let height = top - bottom;
    let depth = near - far;
    Mat4::from_cols(
        Vec4::new(2.0 / width, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 2.0 / height, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 1.0 / depth, 0.0),
        Vec4::new(-(right + left) / width, -(top + bottom) / height, near / depth, 1.0),
    )
}

/// Right-handed world-to-view matrix looking from `eye` toward `target` (camera forward is -Z).
pub fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let backward = (eye - target).normalize();
    let right = up.cross(backward).normalize();
    let camera_up = backward.cross(right);
    Mat4::from_cols(
        Vec4::new(right.x, camera_up.x, backward.x, 0.0),
        Vec4::new(right.y, camera_up.y, backward.y, 0.0),
        Vec4::new(right.z, camera_up.z, backward.z, 0.0),
        Vec4::new(-right.dot(eye), -camera_up.dot(eye), -backward.dot(eye), 1.0),
    )
}

/// A normalized world-space ray.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldRay {
    /// Ray origin on the near plane.
    pub origin: Vec3,
    /// Unit direction.
    pub direction: Vec3,
}

/// Project a world position to viewport-local logical pixels (top-left origin).
///
/// Returns `None` for points behind the camera, non-finite results, or an empty viewport.
pub fn project_to_viewport(position: Vec3, view_projection: Mat4, viewport_size: Vec2) -> Option<Vec2> {
    if viewport_size.x <= 0.0 || viewport_size.y <= 0.0 {
        return None;
    }
    let clip = view_projection * position.extend(1.0);
    if clip.w <= 0.0 || !clip.is_finite() {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    Some(Vec2::new(((ndc.x * 0.5) + 0.5) * viewport_size.x, (0.5 - (ndc.y * 0.5)) * viewport_size.y))
}

/// Build a world ray through a viewport-local logical pixel using the actual projection.
///
/// The origin lies on the near plane. Returns `None` for an empty viewport or a singular matrix.
pub fn screen_ray(point: Vec2, view_projection: Mat4, viewport_size: Vec2) -> Option<WorldRay> {
    if viewport_size.x <= 0.0 || viewport_size.y <= 0.0 || view_projection.determinant().abs() < f32::EPSILON {
        return None;
    }
    let inverse = view_projection.inverse();
    let x = ((point.x / viewport_size.x) * 2.0) - 1.0;
    let y = 1.0 - ((point.y / viewport_size.y) * 2.0);
    let near = inverse * Vec4::new(x, y, 0.0, 1.0);
    let far = inverse * Vec4::new(x, y, 1.0, 1.0);
    if near.w == 0.0 || far.w == 0.0 {
        return None;
    }
    let origin = near.truncate() / near.w;
    let delta = (far.truncate() / far.w) - origin;
    let length = delta.length();
    if length == 0.0 || !length.is_finite() || !origin.is_finite() {
        return None;
    }
    Some(WorldRay { origin, direction: delta / length })
}

/// Intersect a forward ray with the plane `dot(normal, point) + offset = 0`, in world units.
pub fn intersect_ray_plane(ray: WorldRay, normal: Vec3, offset: f32) -> Option<Vec3> {
    let denominator = normal.dot(ray.direction);
    if denominator.abs() < 1e-8 {
        return None;
    }
    let distance = -(normal.dot(ray.origin) + offset) / denominator;
    if distance < 0.0 || !distance.is_finite() {
        return None;
    }
    Some(ray.origin + (ray.direction * distance))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_and_picking_round_trip() {
        let camera = OrbitCamera::default();
        let frame = camera.camera_frame(16.0 / 9.0);
        let viewport = Vec2::new(1600.0, 900.0);

        // The orbit target projects to the viewport center.
        let center = project_to_viewport(camera.target, frame.view_projection(), viewport).unwrap();
        assert!((center - (viewport * 0.5)).length() < 0.01);

        // A ray through the center hits the ground plane at the target.
        let ray = screen_ray(viewport * 0.5, frame.view_projection(), viewport).unwrap();
        let hit = intersect_ray_plane(ray, Vec3::Y, 0.0).unwrap();
        assert!(hit.length() < 0.01);
    }

    #[test]
    fn region_cameras_draw_into_their_region_with_its_aspect() {
        let camera = OrbitCamera::default();
        let viewport = Vec2::new(1280.0, 800.0);
        let (region_min, region_size) = (Vec2::new(320.0, 100.0), Vec2::new(700.0, 250.0));
        let region = RegionCamera::new(&camera, viewport, region_min, region_size);
        let frame = region.camera_frame(viewport.x / viewport.y);

        // The target lands at the region's center, and picking through the whole viewport returns to it.
        let center = project_to_viewport(camera.target, frame.view_projection(), viewport).unwrap();
        assert!((center - (region_min + (region_size * 0.5))).length() < 0.01, "{center:?}");
        let ray = screen_ray(center, frame.view_projection(), viewport).unwrap();
        assert!(intersect_ray_plane(ray, Vec3::Y, 0.0).unwrap().length() < 0.01);

        // A point at the region camera's own frustum edge projects to the region's edge: the image fills the region
        // exactly as the same camera would fill a canvas of the region's size.
        let alone = camera.camera_frame(region_size.x / region_size.y);
        let basis = camera.basis();
        let half_height = (camera.vertical_fov_radians * 0.5).tan() * camera.distance;
        let top = camera.target + (basis.up * half_height);
        let in_region = project_to_viewport(top, frame.view_projection(), viewport).unwrap();
        let in_canvas = project_to_viewport(top, alone.view_projection(), region_size).unwrap();
        assert!((in_region - (region_min + in_canvas)).length() < 0.05, "{in_region:?} vs {in_canvas:?}");
        assert!((in_region.y - region_min.y).abs() < 0.05);
        assert_eq!(frame.view, alone.view);

        let degenerate = RegionCamera::new(&camera, viewport, region_min, Vec2::ZERO).camera_frame(1.6);
        assert_eq!(degenerate, camera.camera_frame(1.6));
    }

    #[test]
    fn shifted_cameras_center_the_image_between_insets() {
        let camera = OrbitCamera::default();
        let viewport = Vec2::new(1280.0, 800.0);
        let shifted = ShiftedCamera::between_insets(&camera, viewport.x, 0.0, 330.0);
        let frame = shifted.camera_frame(viewport.x / viewport.y);

        // The target lands at the center of the uncovered area, and picking through it returns to the target.
        let center = project_to_viewport(camera.target, frame.view_projection(), viewport).unwrap();
        assert!((center - Vec2::new((1280.0 - 330.0) * 0.5, 400.0)).length() < 0.01, "{center:?}");
        let ray = screen_ray(center, frame.view_projection(), viewport).unwrap();
        assert!(intersect_ray_plane(ray, Vec3::Y, 0.0).unwrap().length() < 0.01);

        // Perspective is unchanged: the view matrix and depth mapping match the unshifted camera.
        let original = camera.camera_frame(viewport.x / viewport.y);
        assert_eq!(frame.view, original.view);
        assert_eq!(frame.projection.z_axis.z, original.projection.z_axis.z);
        assert_eq!(ShiftedCamera::between_insets(&camera, 0.0, 10.0, 0.0).shift, Vec2::ZERO);
    }

    #[test]
    fn behind_camera_points_do_not_project() {
        let camera = OrbitCamera::default();
        let frame = camera.camera_frame(1.0);
        let behind = camera.basis().eye + (camera.basis().backward * 10.0);
        assert!(project_to_viewport(behind, frame.view_projection(), Vec2::splat(100.0)).is_none());
    }
}
