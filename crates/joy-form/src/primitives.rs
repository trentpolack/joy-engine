// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Box and sphere primitives as ready-to-draw geometry.

use std::{
    cell::RefCell,
    rc::Rc,
};

use crate::{
    Diagnostic,
    GeometryData,
    builder::Builder,
    value::Budget,
};

/// Longitudinal segment count the TypeScript `meshSphere(radius)` uses when none is given.
pub const DEFAULT_SPHERE_SEGMENTS: u32 = 20;

/// Build a box centered on the origin with full extents in script units.
///
/// The result matches the TypeScript `meshBox(width, height, depth)` vertex and index data,
/// and equals evaluating `box(0, 0, 0, width, height, depth)` without a palette: 8 shared
/// corner vertices with zero (flat-shaded) normals and UVs, 12 triangles, FORM's default vertex
/// tint, and one Default material used by every triangle. The caller owns the returned data.
///
/// Returns a [`Diagnostic`] at line 1, column 1 when an extent is not positive or a corner
/// would fall outside ±1,000,000.
pub fn mesh_box(width: f64, height: f64, depth: f64) -> Result<GeometryData, Diagnostic> {
    let mut builder = primitive_builder()?;
    builder.box_geometry(&[0.0, 0.0, 0.0, width, height, depth]).map_err(Diagnostic::unlocated)?;
    Ok(builder.data)
}

/// Build a UV sphere centered on the origin with a radius in script units.
///
/// `segments` is the longitudinal division count (3–96; see [`DEFAULT_SPHERE_SEGMENTS`]); the
/// sphere has `ceil(segments / 2)` latitude rows. The result matches the TypeScript
/// `meshSphere(radius, segments)` vertex and index data, and equals evaluating
/// `sphere(0, 0, 0, radius, segments)` without a palette: seam-duplicated vertices with unit
/// radial normals and UVs, FORM's default vertex tint, and one Default material. The caller owns
/// the returned data.
///
/// Returns a [`Diagnostic`] at line 1, column 1 when the radius is not positive, `segments` is
/// outside 3–96, or a vertex would fall outside ±1,000,000.
pub fn mesh_sphere(radius: f64, segments: u32) -> Result<GeometryData, Diagnostic> {
    let mut builder = primitive_builder()?;
    builder.sphere(&[0.0, 0.0, 0.0, radius, f64::from(segments)]).map_err(Diagnostic::unlocated)?;
    Ok(builder.data)
}

// Each primitive owns a fresh budget; only the builder's own vertex and triangle limits apply.
fn primitive_builder() -> Result<Builder, Diagnostic> {
    Builder::new(None, Rc::new(RefCell::new(Budget::default()))).map_err(Diagnostic::unlocated)
}
