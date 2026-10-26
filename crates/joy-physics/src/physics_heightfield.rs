// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Static heightfield bodies: a grid of heights that boxes, spheres, vehicle wheels, raycasts, and SDF bodies all
//! collide with.
//!
//! Every cell splits along its (-X, -Z) to (+X, +Z) diagonal, the same split the terrain renderer and terrain sampling
//! use, so a body rests exactly on the drawn triangles. Rapier's own contact generation handles primitives; SDF bodies
//! read the heightfield as a distance field (the signed distance to the plane of the triangle under a point).

use joy_core::math::Vec3;
use rapier3d::geometry::SharedShape;
use rapier3d::math::Vector;
use rapier3d::parry::shape::{
    HeightField,
    HeightFieldCellStatus,
    HeightFieldFlags,
};
use rapier3d::parry::utils::Array2;
use rapier3d::pipeline::ActiveHooks;

use super::convert::{
    finite_number,
    finite_vector,
};
use super::physics_body::{
    BODY_DEFAULTS,
    BodyOptions,
    ValidatedBody,
};
use super::{
    BodyHandle,
    PhysicsError,
    PhysicsWorld,
};

/// A static heightfield collider. World coordinates are meters, Y up.
#[derive(Clone, Debug, PartialEq)]
pub struct HeightfieldOptions {
    /// World position of sample (0, 0) with zero height: the grid's -X, -Z corner, before heights add to Y.
    pub origin: Vec3,
    /// Meters between neighboring samples along X and Z; finite and positive.
    pub spacing: [f32; 2],
    /// Samples along X; at least two.
    pub columns: usize,
    /// Samples along Z; at least two.
    pub rows: usize,
    /// Heights in meters, row-major (-Z row first), `columns * rows` finite values.
    pub heights: Vec<f32>,
    /// Friction coefficient; nonnegative. Combined with the other body's by multiplication.
    pub friction: f32,
    /// Restitution in `[0, 1]`. Combined with the other body's by multiplication.
    pub restitution: f32,
}

impl PhysicsWorld {
    /// Create a static heightfield body and return its handle.
    ///
    /// Validates the grid before changing the world. The body never moves; replace a heightfield after an edit by
    /// removing it and creating another.
    pub fn create_heightfield_body(&mut self, options: &HeightfieldOptions) -> Result<BodyHandle, PhysicsError> {
        let origin = finite_vector(options.origin, "origin")?;
        let spacing_x = finite_number(options.spacing[0], "spacing", f64::from(f32::MIN_POSITIVE), f64::INFINITY)?;
        let spacing_z = finite_number(options.spacing[1], "spacing", f64::from(f32::MIN_POSITIVE), f64::INFINITY)?;
        let valid_grid = options.columns >= 2
            && options.rows >= 2
            && options.heights.len() == options.columns * options.rows
            && options.heights.iter().all(|height| height.is_finite());
        if !valid_grid {
            return Err(PhysicsError::InvalidHeightfield);
        }

        // Rapier centers heightfields on their body; place the body at the grid's center.
        let width = spacing_x * (options.columns - 1) as f32;
        let depth = spacing_z * (options.rows - 1) as f32;
        let validated = ValidatedBody::new(&BodyOptions {
            mass: 0.0,
            position: origin + Vec3::new(width * 0.5, 0.0, depth * 0.5),
            friction: options.friction,
            restitution: options.restitution,
            ..BODY_DEFAULTS
        })?;

        // Parry stores heights column-major with rows along Z and columns along X.
        let mut column_major = Vec::with_capacity(options.heights.len());
        for column in 0..options.columns {
            for row in 0..options.rows {
                column_major.push(options.heights[(row * options.columns) + column]);
            }
        }
        let heights = Array2::new(options.rows, options.columns, column_major);

        // Parry's internal-edge correction assumes its default diagonal; with the zigzag split it discards contacts
        // and bodies fall through, so it stays off.
        let mut heightfield =
            HeightField::with_flags(heights, Vector::new(width, 1.0, depth), HeightFieldFlags::empty());
        for row in 0..options.rows - 1 {
            for column in 0..options.columns - 1 {
                heightfield.set_cell_status(row, column, HeightFieldCellStatus::ZIGZAG_SUBDIVISION);
            }
        }
        Ok(self.insert_body(validated, SharedShape::new(heightfield), ActiveHooks::empty()))
    }
}
