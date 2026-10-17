// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Signed distance collision shapes: analytical primitives and voxel grids baked from triangle meshes.
//!
//! An [`SdfShape`] is an immutable, cheaply cloned handle. Construction validates the input, then precomputes
//! everything collision needs from the field alone: tight local bounds, unit-density mass properties, and the
//! high-curvature seed points that steer stochastic contact sampling (Eq. 7 of Giles and Andrews 2026). Bodies created
//! with [`PhysicsWorld::create_sdf_body`](super::super::PhysicsWorld::create_sdf_body) share the handle's data, so one
//! baked grid serves any number of instances.

use std::fmt;
use std::sync::Arc;

use glam::Vec3;
use rapier3d::math::{Matrix, Real, Vector};
use rapier3d::parry::bounding_volume::{Aabb, BoundingVolume};

use super::super::PhysicsError;
use super::super::convert::{from_solver_vector, to_solver_vector};
use super::sdf_primitives::{FieldSample, box_field, capsule_field, cylinder_field, sphere_field, torus_field};
use super::sdf_voxel::{TriangleMesh, VoxelField, unit};
use crate::core::SeededRandom;

// ---------------------------------------------------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------------------------------------------------

/// Defaults for [`SdfShape::from_triangle_mesh`]: 48 cells along the longest side (about 1.7 MB for a cube-like mesh).
pub const SDF_BAKE_DEFAULTS: SdfBakeOptions = SdfBakeOptions { resolution: 48 };

/// Smallest and largest accepted bake resolutions, in cells along the mesh's longest side.
const MIN_BAKE_RESOLUTION: u32 = 8;
const MAX_BAKE_RESOLUTION: u32 = 128;

/// Integration cells along the longest side when computing mass properties.
const MASS_CELLS: Real = 48.0;

/// Shapes enclosing less volume than this fraction of their bounding box cannot carry mass.
const MIN_VOLUME_FRACTION: f64 = 1.0e-6;

/// Surface candidates tested for curvature seeds, and the fixed stream that draws them (seeds are part of the shape,
/// so the same input always yields the same seeds).
const SEED_CANDIDATES: usize = 8000;
const SEED_RANDOM_SEED: i64 = 0x5DF_5EED;

/// Curvature threshold for a unit-size shape (`|Δφ| > κ`, Eq. 7); it scales with `1 / size`.
const SEED_CURVATURE_THRESHOLD: Real = 4.0;
/// Finite-difference step of the Laplacian, as a fraction of the shape's size.
const SEED_LAPLACIAN_STEP: Real = 0.001;
/// Minimum spacing between seeds, as a fraction of the shape's size, so seeds spread over every sharp region.
const SEED_SPACING: Real = 0.08;
/// Analytical candidates start this far (as a fraction of size) outside the bounds and are projected onto the surface.
const SEED_CANDIDATE_MARGIN: Real = 0.1;
/// Candidates farther than this fraction of size from the surface after projection are discarded.
const SEED_SURFACE_TOLERANCE: Real = 0.001;

/// Largest rate of change of a baked grid's distance, in meters per meter (see `SdfShapeData::lipschitz_bound`).
const VOXEL_LIPSCHITZ_BOUND: Real = 2.0;

/// Newton projection limits for moving a point onto the zero level set.
const PROJECTION_ITERATIONS: usize = 32;
const PROJECTION_TOLERANCE: Real = 1.0e-5;
/// Smallest squared gradient length used as a Newton denominator.
const MIN_GRADIENT_SQUARED: Real = 1.0e-8;

// ---------------------------------------------------------------------------------------------------------------------
// Public records
// ---------------------------------------------------------------------------------------------------------------------

/// Analytical shapes for [`SdfShape::primitive`]. Dimensions are meters in the shape's local frame; symmetric shapes
/// use +Y as their axis and are centered on the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SdfPrimitive {
    /// Box with positive half extents.
    Box {
        /// Half extents along X, Y, and Z.
        half_extents: Vec3,
    },
    /// Sphere.
    Sphere {
        /// Radius.
        radius: f32,
    },
    /// Capsule: a Y segment from `-half_length` to `half_length` swept by `radius`.
    Capsule {
        /// Radius of the swept sphere.
        radius: f32,
        /// Half the length of the core segment.
        half_length: f32,
    },
    /// Solid cylinder along Y with flat caps.
    Cylinder {
        /// Radius.
        radius: f32,
        /// Half the height.
        half_height: f32,
    },
    /// Ring torus around Y in the XZ plane; `minor_radius` must be smaller than `major_radius`.
    Torus {
        /// Distance from the center to the middle of the tube.
        major_radius: f32,
        /// Tube radius.
        minor_radius: f32,
    },
}

/// Grid settings for [`SdfShape::from_triangle_mesh`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SdfBakeOptions {
    /// Cells along the mesh's longest side, in `[8, 128]`. Memory grows with the cube: 48 cells take about 1.7 MB for
    /// a cube-like mesh, 100 cells (the paper's setting) about 17 MB.
    pub resolution: u32,
}

impl Default for SdfBakeOptions {
    fn default() -> Self {
        SDF_BAKE_DEFAULTS
    }
}

/// Immutable signed distance collision shape, shared by every body created from it.
///
/// Cloning copies a reference; the field, bounds, mass properties, and seeds are computed once at construction and
/// released when the last clone and the last body using it are dropped. Coordinates are meters in the shape's local
/// frame, which is the frame of the body that uses it.
#[derive(Clone)]
pub struct SdfShape {
    pub(crate) data: Arc<SdfShapeData>,
}

// ---------------------------------------------------------------------------------------------------------------------
// Shape data
// ---------------------------------------------------------------------------------------------------------------------

/// The distance field behind a shape.
pub(crate) enum SdfField {
    Box { half_extents: Vector },
    Sphere { radius: Real },
    Capsule { radius: Real, half_length: Real },
    Cylinder { radius: Real, half_height: Real },
    Torus { major_radius: Real, minor_radius: Real },
    Voxel(VoxelField),
}

/// Precomputed collision data of one shape, in its local frame.
pub(crate) struct SdfShapeData {
    field: SdfField,
    /// Tight local bounds of the zero level set.
    pub(crate) bounds: Aabb,
    /// Largest side of the bounds; scales tolerances so shapes of any size behave alike.
    pub(crate) size: Real,
    /// High-curvature surface points for stochastic contact sampling (Sec. 3.5.2).
    pub(crate) seeds: Vec<Vector>,
    /// Two convex shapes overlap in one convex region, which lets contact search skip random samples.
    pub(crate) convex: bool,
    /// Unit-density mass properties: volume (cubic meters), center of mass, and inertia about the center of mass.
    pub(crate) volume: Real,
    pub(crate) center_of_mass: Vector,
    pub(crate) inertia: Matrix,
}

impl SdfShape {
    /// Create an analytical shape. Fails unless every dimension is finite and positive, and a torus's minor radius is
    /// smaller than its major radius.
    pub fn primitive(primitive: SdfPrimitive) -> Result<Self, PhysicsError> {
        let (field, half_extents, convex) = match primitive {
            SdfPrimitive::Box { half_extents } => {
                positive(&[half_extents.x, half_extents.y, half_extents.z])?;
                let half_extents = to_solver_vector(half_extents);
                (SdfField::Box { half_extents }, half_extents, true)
            }
            SdfPrimitive::Sphere { radius } => {
                positive(&[radius])?;
                (SdfField::Sphere { radius }, Vector::splat(radius), true)
            }
            SdfPrimitive::Capsule { radius, half_length } => {
                positive(&[radius, half_length])?;
                (SdfField::Capsule { radius, half_length }, Vector::new(radius, radius + half_length, radius), true)
            }
            SdfPrimitive::Cylinder { radius, half_height } => {
                positive(&[radius, half_height])?;
                (SdfField::Cylinder { radius, half_height }, Vector::new(radius, half_height, radius), true)
            }
            SdfPrimitive::Torus { major_radius, minor_radius } => {
                positive(&[major_radius, minor_radius])?;
                if minor_radius >= major_radius {
                    return Err(PhysicsError::InvalidSdfShape("torus minor radius must be below its major radius"));
                }
                let outer = major_radius + minor_radius;
                (SdfField::Torus { major_radius, minor_radius }, Vector::new(outer, minor_radius, outer), false)
            }
        };
        let bounds = Aabb::new(-half_extents, half_extents);
        let data = SdfShapeData::finalize(field, bounds, convex, SurfaceSource::Projection)?;
        Ok(Self { data: Arc::new(data) })
    }

    /// Bake a closed (or nearly closed) triangle mesh into a voxel distance grid.
    ///
    /// `positions` are meters in the mesh's frame, which becomes the shape's local frame, and `indices` lists three
    /// position indices per triangle. Winding may face either way; small holes and overlapping parts are tolerated.
    /// Fails on non-finite positions, malformed indices, a resolution outside `[8, 128]`, or a mesh that encloses no
    /// volume. Baking is CPU work proportional to the grid size; build shapes once and share them.
    pub fn from_triangle_mesh(
        positions: &[Vec3],
        indices: &[u32],
        options: SdfBakeOptions,
    ) -> Result<Self, PhysicsError> {
        if !(MIN_BAKE_RESOLUTION..=MAX_BAKE_RESOLUTION).contains(&options.resolution) {
            return Err(PhysicsError::OutOfRange {
                name: "resolution",
                minimum: f64::from(MIN_BAKE_RESOLUTION),
                maximum: f64::from(MAX_BAKE_RESOLUTION),
            });
        }
        let positions: Vec<Vector> = positions.iter().map(|position| to_solver_vector(*position)).collect();
        let mesh = TriangleMesh::new(&positions, indices).map_err(PhysicsError::InvalidSdfShape)?;
        let field = SdfField::Voxel(VoxelField::bake(&mesh, options.resolution));
        let data = SdfShapeData::finalize(field, mesh.bounds(), false, SurfaceSource::Mesh(&mesh))?;
        Ok(Self { data: Arc::new(data) })
    }

    /// Signed distance in meters from a local point to the surface; negative inside.
    pub fn distance(&self, point: Vec3) -> f32 {
        self.data.sample(to_solver_vector(point)).distance
    }

    /// Tight local bounds of the surface as `(minimum, maximum)` corners in meters.
    pub fn local_bounds(&self) -> (Vec3, Vec3) {
        (from_solver_vector(self.data.bounds.mins), from_solver_vector(self.data.bounds.maxs))
    }

    /// Enclosed volume in cubic meters, integrated from the field.
    pub fn volume(&self) -> f32 {
        self.data.volume
    }

    /// Number of precomputed high-curvature seed points.
    pub fn seed_count(&self) -> usize {
        self.data.seeds.len()
    }

    /// Whether the shape is a convex primitive (boxes, spheres, capsules, and cylinders).
    pub fn is_convex(&self) -> bool {
        self.data.convex
    }

    /// Bytes held by the field (the voxel grid for baked meshes; analytical shapes store only their dimensions).
    pub fn field_bytes(&self) -> usize {
        match &self.data.field {
            SdfField::Voxel(voxel) => voxel.memory_bytes(),
            _ => std::mem::size_of::<SdfField>(),
        }
    }
}

impl fmt::Debug for SdfShape {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SdfShape")
            .field("bounds", &self.local_bounds())
            .field("volume", &self.data.volume)
            .field("seeds", &self.data.seeds.len())
            .field("convex", &self.data.convex)
            .finish()
    }
}

/// Where curvature seed candidates come from.
enum SurfaceSource<'a> {
    /// Random points around the bounds, projected onto the surface along the gradient.
    Projection,
    /// Area-weighted points on the source triangles.
    Mesh(&'a TriangleMesh),
}

impl SdfShapeData {
    /// Signed distance and gradient at a local point.
    pub(crate) fn sample(&self, point: Vector) -> FieldSample {
        match &self.field {
            SdfField::Box { half_extents } => box_field(point, *half_extents),
            SdfField::Sphere { radius } => sphere_field(point, *radius),
            SdfField::Capsule { radius, half_length } => capsule_field(point, *radius, *half_length),
            SdfField::Cylinder { radius, half_height } => cylinder_field(point, *radius, *half_height),
            SdfField::Torus { major_radius, minor_radius } => torus_field(point, *major_radius, *minor_radius),
            SdfField::Voxel(voxel) => voxel.sample(point),
        }
    }

    /// Signed distance alone at a local point; cheaper than [`Self::sample`] for baked grids.
    pub(crate) fn distance(&self, point: Vector) -> Real {
        match &self.field {
            SdfField::Voxel(voxel) => voxel.distance(point),
            _ => self.sample(point).distance,
        }
    }

    /// Upper bound on how fast the distance can change per meter. Exact fields change at most one meter per meter;
    /// trilinear interpolation and extrapolation of a baked grid can exceed that, by at most a factor of two.
    pub(crate) fn lipschitz_bound(&self) -> Real {
        match &self.field {
            SdfField::Voxel(_) => VOXEL_LIPSCHITZ_BOUND,
            _ => 1.0,
        }
    }

    /// Move a local point onto the zero level set with Newton steps along the gradient (valid for inexact fields too).
    pub(crate) fn project(&self, mut point: Vector) -> Vector {
        let tolerance = PROJECTION_TOLERANCE * self.size;
        for _ in 0..PROJECTION_ITERATIONS {
            let sample = self.sample(point);
            if sample.distance.abs() < tolerance {
                break;
            }
            point -= sample.gradient * (sample.distance / sample.gradient.length_squared().max(MIN_GRADIENT_SQUARED));
        }
        point
    }

    /// Compute mass properties and curvature seeds for a field.
    fn finalize(field: SdfField, bounds: Aabb, convex: bool, surface: SurfaceSource<'_>) -> Result<Self, PhysicsError> {
        let size = bounds.extents().max_element();
        let mut data = Self {
            field,
            bounds,
            size,
            seeds: Vec::new(),
            convex,
            volume: 0.0,
            center_of_mass: bounds.center(),
            inertia: Matrix::IDENTITY,
        };
        data.integrate_mass_properties()?;
        data.seeds = data.curvature_seeds(surface);
        Ok(data)
    }

    /// Unit-density volume, center of mass, and inertia by integrating over a voxelization of the field.
    ///
    /// Moments accumulate in `f64` so large grids do not lose small contributions; each inside cell adds its own
    /// second moment (`h² / 12` per axis) so coarse grids keep the right inertia for boxes.
    fn integrate_mass_properties(&mut self) -> Result<(), PhysicsError> {
        let extents = self.bounds.extents();
        let spacing = self.size / MASS_CELLS;
        let counts = [0, 1, 2].map(|axis| ((extents[axis] / spacing).ceil() as usize).max(1));
        let cell =
            Vector::new(extents.x / counts[0] as Real, extents.y / counts[1] as Real, extents.z / counts[2] as Real);
        let cell_volume = f64::from(cell.x * cell.y * cell.z);

        let mut mass = 0.0_f64;
        let mut first = [0.0_f64; 3];
        let mut second = [[0.0_f64; 3]; 3];
        for z in 0..counts[2] {
            for y in 0..counts[1] {
                for x in 0..counts[0] {
                    let center =
                        self.bounds.mins + (cell * Vector::new(x as Real + 0.5, y as Real + 0.5, z as Real + 0.5));
                    if self.sample(center).distance >= 0.0 {
                        continue;
                    }
                    let center = [center.x, center.y, center.z].map(f64::from);
                    mass += cell_volume;
                    for i in 0..3 {
                        first[i] += center[i] * cell_volume;
                        for j in 0..3 {
                            let own = if i == j { f64::from(cell[i] * cell[i]) / 12.0 } else { 0.0 };
                            second[i][j] += ((center[i] * center[j]) + own) * cell_volume;
                        }
                    }
                }
            }
        }

        let bounds_volume = f64::from(extents.x) * f64::from(extents.y) * f64::from(extents.z);
        if mass <= MIN_VOLUME_FRACTION * bounds_volume {
            return Err(PhysicsError::InvalidSdfShape("shape must enclose a volume"));
        }
        let center = first.map(|moment| moment / mass);

        // Second moment about the center of mass S, then inertia I = tr(S) Id - S.
        let mut moment = [[0.0_f64; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                moment[i][j] = second[i][j] - (mass * center[i] * center[j]);
            }
        }
        let trace = moment[0][0] + moment[1][1] + moment[2][2];
        let inertia = |i: usize, j: usize| (if i == j { trace } else { 0.0 } - moment[i][j]) as Real;

        self.volume = mass as Real;
        self.center_of_mass = Vector::new(center[0] as Real, center[1] as Real, center[2] as Real);
        self.inertia = Matrix::from_cols(
            Vector::new(inertia(0, 0), inertia(1, 0), inertia(2, 0)),
            Vector::new(inertia(0, 1), inertia(1, 1), inertia(2, 1)),
            Vector::new(inertia(0, 2), inertia(1, 2), inertia(2, 2)),
        );
        Ok(())
    }

    /// Surface points whose Laplacian magnitude exceeds the curvature threshold, thinned to a minimum spacing.
    fn curvature_seeds(&self, surface: SurfaceSource<'_>) -> Vec<Vector> {
        let mut random = SeededRandom::new(SEED_RANDOM_SEED);
        let candidates = match surface {
            SurfaceSource::Mesh(mesh) => mesh.surface_samples(SEED_CANDIDATES, &mut random),
            SurfaceSource::Projection => self.projected_surface_samples(&mut random),
        };

        let threshold = SEED_CURVATURE_THRESHOLD / self.size;
        let step = SEED_LAPLACIAN_STEP * self.size;
        let spacing_squared = (SEED_SPACING * self.size) * (SEED_SPACING * self.size);
        let mut seeds: Vec<Vector> = Vec::new();
        for candidate in candidates {
            if self.laplacian(candidate, step).abs() <= threshold {
                continue;
            }
            // Poisson-disk rejection keeps seeds spread over every high-curvature region instead of piling up.
            if seeds.iter().all(|seed| (*seed - candidate).length_squared() >= spacing_squared) {
                seeds.push(candidate);
            }
        }
        seeds
    }

    /// Random points around the bounds, projected onto the surface; points that fail to converge are dropped.
    fn projected_surface_samples(&self, random: &mut SeededRandom) -> Vec<Vector> {
        let region = self.bounds.loosened(SEED_CANDIDATE_MARGIN * self.size);
        let extents = region.extents();
        let tolerance = SEED_SURFACE_TOLERANCE * self.size;
        let mut samples = Vec::with_capacity(SEED_CANDIDATES);
        for _ in 0..SEED_CANDIDATES {
            let offset = Vector::new(unit(random), unit(random), unit(random)) * extents;
            let point = self.project(region.mins + offset);
            if self.sample(point).distance.abs() < tolerance {
                samples.push(point);
            }
        }
        samples
    }

    /// Laplacian of the field by central differences of its gradient (Eq. 7).
    fn laplacian(&self, point: Vector, step: Real) -> Real {
        let mut sum = 0.0;
        for axis in 0..3 {
            let mut offset = Vector::ZERO;
            offset[axis] = step;
            let ahead = self.sample(point + offset).gradient[axis];
            let behind = self.sample(point - offset).gradient[axis];
            sum += (ahead - behind) / (2.0 * step);
        }
        sum
    }
}

/// Require finite, positive dimensions.
fn positive(values: &[f32]) -> Result<(), PhysicsError> {
    if values.iter().all(|value| value.is_finite() && *value > 0.0) {
        return Ok(());
    }
    Err(PhysicsError::InvalidSdfShape("dimensions must be finite and positive"))
}
