// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Fixed top-down orthographic camera for arcade games.
//!
//! Arcade games lay out a flat world in logical pixels (one world unit per CSS pixel, +Y down the screen) and give
//! geometry a height in Z that only decides depth: which surface hides which, and where translucent layers and particle
//! batches fall in the renderer's back-to-front order. [`TopDownView`] is the [`Camera`] that draws such a world
//! through the scene renderer, so arcade frames share its depth, transparency queue, instanced particles, and display
//! processing with every other game. Display density is deliberately absent, so a sharper display never changes how
//! much of the world is framed.

use joy_core::math::{
    Mat4,
    Vec2,
};

use crate::RenderError;
use crate::camera::{
    Camera,
    CameraFrame,
    orthographic,
};

/// Camera placement for [`TopDownView`]: the camera sits at world `height` on +Z looking down -Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopDownCamera {
    /// Camera Z in world units.
    pub height: f32,
    /// Near distance in front of the camera, in world units.
    pub near: f32,
    /// Far distance in front of the camera, in world units; greater than `near`.
    pub far: f32,
}

/// A top-down orthographic view of a `size` logical region centered on `center`, looking down -Z.
///
/// X and Y use the caller's logical world units (one per CSS pixel in the arcade games) and +Y is down the screen.
/// Depth maps `near..far` in front of the camera to zero-to-one clip depth on every backend, so raising geometry along
/// +Z brings it toward the viewer. The view's axes are the world's own (right +X, up +Y, backward +Z), which is how
/// camera-facing particles orient their rolls; the projection, not the view, flips Y onto the screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TopDownView {
    size: Vec2,
    center: Vec2,
    camera: TopDownCamera,
}

impl TopDownView {
    /// Frame the `size` logical region around `center` from `camera`. Fails for an empty region or a non-positive
    /// depth range.
    pub fn new(size: Vec2, center: Vec2, camera: TopDownCamera) -> Result<Self, RenderError> {
        if size.x <= 0.0 || size.y <= 0.0 {
            return Err(RenderError::InvalidFrame("logical viewport dimensions must be positive".into()));
        }
        if camera.far - camera.near <= 0.0 {
            return Err(RenderError::InvalidFrame("camera far distance must exceed its near distance".into()));
        }
        Ok(Self {
            size,
            center,
            camera,
        })
    }

    /// The framed region's logical size.
    pub fn size(&self) -> Vec2 {
        self.size
    }

    /// The framed region's center in world units.
    pub fn center(&self) -> Vec2 {
        self.center
    }

    /// The camera placement.
    pub fn camera(&self) -> TopDownCamera {
        self.camera
    }

    /// The column-major world-to-clip matrix the renderer draws with.
    pub fn view_projection(&self) -> Mat4 {
        self.camera_frame(self.size.x / self.size.y).view_projection()
    }
}

impl Camera for TopDownView {
    /// The view frames exactly its region whatever `aspect` the viewport has; arcade games size the region to the
    /// logical viewport, so the two agree.
    fn camera_frame(&self, _aspect: f32) -> CameraFrame {
        let eye = self.center.extend(self.camera.height);
        let half = self.size * 0.5;
        // Swapping the bottom and top edges maps +Y down the screen.
        let projection = orthographic(-half.x, half.x, half.y, -half.y, self.camera.near, self.camera.far);
        CameraFrame {
            view: Mat4::from_translation(-eye),
            projection,
            eye,
            focus: self.center.extend(0.0),
            near: self.camera.near,
            far: self.camera.far,
        }
    }
}

#[cfg(test)]
#[path = "top_down_camera.test.rs"]
mod tests;
