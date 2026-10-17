// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The solver-side collider for an [`SdfShape`]: a custom parry shape with bounds, mass properties, sphere-traced
//! ray casts, and Newton point projection. Contacts between it and other shapes come from
//! [`SdfQueryDispatcher`](super::sdf_dispatcher::SdfQueryDispatcher); the solver itself treats it like any collider.

use rapier3d::math::{
    Real,
    Vector,
};
use rapier3d::parry::bounding_volume::{
    Aabb,
    BoundingSphere,
    BoundingVolume,
};
use rapier3d::parry::mass_properties::MassProperties;
use rapier3d::parry::query::{
    PointProjection,
    PointQuery,
    Ray,
    RayCast,
    RayIntersection,
};
use rapier3d::parry::shape::{
    FeatureId,
    Shape,
    ShapeType,
    TypedShape,
};

use super::sdf_shape::{
    SdfShape,
    SdfShapeData,
};

/// Sphere-tracing steps before a ray gives up.
const RAY_MAX_STEPS: usize = 256;
/// A ray hits once it is closer to the surface than this fraction of the shape's size.
const RAY_HIT_FRACTION: Real = 1.0e-4;
/// Fraction of the distance a ray advances per step; baked grids interpolate distances, which can slightly overestimate
/// them near sharp features.
const RAY_STEP_FRACTION: Real = 0.9;
/// Angle (radians) the shape may rotate before contacts can change; conservative for sharp, non-convex fields.
const CCD_ANGULAR_THICKNESS: Real = std::f32::consts::FRAC_PI_4;

/// A shared [`SdfShape`] attached to a solver collider.
#[derive(Clone)]
pub(crate) struct SdfCollider {
    pub(crate) shape: SdfShape,
}

impl SdfCollider {
    pub(crate) fn data(&self) -> &SdfShapeData {
        &self.shape.data
    }
}

impl Shape for SdfCollider {
    fn compute_local_aabb(&self) -> Aabb {
        self.data().bounds
    }

    fn compute_local_bounding_sphere(&self) -> BoundingSphere {
        self.data().bounds.bounding_sphere()
    }

    fn clone_dyn(&self) -> Box<dyn Shape> {
        Box::new(self.clone())
    }

    fn scale_dyn(&self, _scale: Vector, _num_subdivisions: u32) -> Option<Box<dyn Shape>> {
        // Scaling would need a rebaked field and new seeds; build a new SdfShape instead.
        None
    }

    fn mass_properties(&self, density: Real) -> MassProperties {
        let data = self.data();
        MassProperties::with_inertia_matrix(data.center_of_mass, data.volume * density, data.inertia * density)
    }

    fn shape_type(&self) -> ShapeType {
        ShapeType::Custom
    }

    fn as_typed_shape(&self) -> TypedShape<'_> {
        TypedShape::Custom(self)
    }

    fn ccd_thickness(&self) -> Real {
        self.data().bounds.half_extents().min_element()
    }

    fn ccd_angular_thickness(&self) -> Real {
        CCD_ANGULAR_THICKNESS
    }

    fn is_convex(&self) -> bool {
        self.data().convex
    }
}

impl RayCast for SdfCollider {
    /// Sphere-trace the field from where the ray enters the bounds. A ray starting inside reports a zero time of
    /// impact when `solid`, and otherwise the exit point with an inward normal.
    fn cast_local_ray_and_get_normal(
        &self,
        ray: &Ray,
        max_time_of_impact: Real,
        solid: bool,
    ) -> Option<RayIntersection> {
        let data = self.data();
        let speed = ray.dir.length();
        if speed <= 0.0 || !speed.is_finite() {
            return None;
        }
        let hit_distance = RAY_HIT_FRACTION * data.size;
        let bounds = data.bounds.loosened(2.0 * hit_distance);
        let mut time = bounds.cast_local_ray(ray, max_time_of_impact, true)?;

        // Inside the shape at the start: solid casts stop immediately, hollow casts march to the exit.
        let mut sample = data.sample(ray.point_at(time));
        if sample.distance < 0.0 {
            if solid {
                return Some(RayIntersection::new(time, Vector::ZERO, FeatureId::Unknown));
            }
            for _ in 0..RAY_MAX_STEPS {
                sample = data.sample(ray.point_at(time));
                if sample.distance >= 0.0 {
                    return Some(RayIntersection::new(time, -sample.gradient.normalize_or_zero(), FeatureId::Unknown));
                }
                time += (-sample.distance).max(hit_distance) / speed;
                if time > max_time_of_impact || !bounds.contains_local_point(ray.point_at(time)) {
                    return None;
                }
            }
            return None;
        }

        for _ in 0..RAY_MAX_STEPS {
            sample = data.sample(ray.point_at(time));
            if sample.distance < hit_distance {
                return Some(RayIntersection::new(time, sample.gradient.normalize_or_zero(), FeatureId::Unknown));
            }
            time += (RAY_STEP_FRACTION * sample.distance) / speed;
            if time > max_time_of_impact || !bounds.contains_local_point(ray.point_at(time)) {
                return None;
            }
        }
        None
    }
}

impl PointQuery for SdfCollider {
    fn project_local_point(&self, point: Vector, solid: bool) -> PointProjection {
        let data = self.data();
        let inside = data.sample(point).distance < 0.0;
        if inside && solid {
            return PointProjection::new(true, point);
        }
        PointProjection::new(inside, data.project(point))
    }

    fn project_local_point_and_get_feature(&self, point: Vector) -> (PointProjection, FeatureId) {
        (self.project_local_point(point, false), FeatureId::Unknown)
    }
}
