// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Contact search between two distance fields: Algorithm 1 of Giles and Andrews 2026.
//!
//! Points inside both shapes are where both fields are negative, so the deepest overlap minimizes
//! `max(φ1, φ2)`. Each step minimizes a smooth upper bound of that maximum (Eq. 2) minus a repulsion from the points
//! already found (Eq. 3), starting from a handful of seeds: last step's points carried along with the bodies (the
//! temporal cache, Sec. 3.5.1) and random high-curvature seeds inside the search domain (Sec. 3.5.2). The result is a
//! sparse, well-spread manifold of at most a few points per pair.
//!
//! Everything is expressed in the first shape's local frame; `pose12` places the second shape in that frame. Lengths
//! are meters, and the paper's unit-size tolerances scale with the smaller shape's size.

use rapier3d::math::{
    Pose,
    Real,
    Vector,
};
use rapier3d::parry::bounding_volume::{
    Aabb,
    BoundingVolume,
};

use rapier3d::parry::shape::HeightField;

use super::sdf_primitives::{
    FieldSample,
    box_field,
    half_space_field,
    heightfield_field,
    sphere_field,
};
use super::sdf_shape::SdfShapeData;
use super::sdf_voxel::unit;
use joy_core::SeededRandom;

// ---------------------------------------------------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------------------------------------------------

/// Search parameters; the defaults are the paper's (Sec. 4) plus the reference implementation's stability additions.
struct ContactSearchSettings {
    /// Points carried to the next step (`n_cache`).
    cache_size: usize,
    /// New stochastic samples per pair and step (`n_rand`).
    random_samples: usize,
    /// Skip random samples for two convex shapes whose full cache is in contact: they overlap in one convex region,
    /// which the cached points already track. Roughly halves the cost of resting convex stacks.
    skip_convex_samples: bool,
    /// Softmax smoothing `ε` of Eq. 2 for unit-size shapes; scales with the squared size.
    softmax_smoothing: Real,
    /// Repulsion weight `α` of Eq. 3.
    repulsion: Real,
    /// Optimizer tolerance `τ` for unit-size shapes, also the duplicate distance; scales with size.
    tolerance: Real,
    /// Improvement of the objective a new sample needs to displace a cached point, for unit-size shapes. Keeps equally
    /// deep points on flat faces from trading places every step, which would defeat solver warm starting.
    replacement_threshold: Real,
    /// Optimizer iteration limit.
    max_iterations: usize,
    /// Initial simplex size of new samples, as a fraction of the search domain.
    simplex_fraction: Real,
    /// Smallest initial simplex of cached points, in tolerances; cached points only need to follow the bodies.
    cached_simplex_tolerances: Real,
    /// Convergence tolerance of cached points, as a fraction of `tolerance`.
    cached_tolerance_fraction: Real,
    /// Depth gain (in tolerances) a cached point needs before it leaves its carried position.
    cached_improvement: Real,
    /// Cached points carried less than this fraction of the tolerance are re-measured without optimizing.
    settled_fraction: Real,
}

const CONTACT_SEARCH: ContactSearchSettings = ContactSearchSettings {
    cache_size: 4,
    random_samples: 1,
    skip_convex_samples: true,
    softmax_smoothing: 0.1,
    repulsion: 0.01,
    tolerance: 0.01,
    replacement_threshold: 0.01,
    max_iterations: 50,
    simplex_fraction: 0.1,
    cached_simplex_tolerances: 5.0,
    cached_tolerance_fraction: 0.25,
    cached_improvement: 0.01,
    settled_fraction: 0.1,
};

/// Smallest squared gradient length used as a Newton denominator.
const MIN_GRADIENT_SQUARED: Real = 1.0e-8;
/// Newton steps when projecting an optimized point onto a surface, and the precision (in tolerances) that ends them
/// early. Exact fields land in one step; baked grids take a few.
const PROJECTION_STEPS: usize = 4;
const PROJECTION_PRECISION: Real = 0.01;
/// Normals shorter than this (degenerate gradients) cannot orient a contact.
const MIN_NORMAL_LENGTH_SQUARED: Real = 0.25;
/// Nelder-Mead also stops when the objective varies less than this fraction of the tolerance across the simplex.
const FLAT_SIMPLEX_FRACTION: Real = 0.05;
/// Only half-spaces this close to an axis clip the search domain.
const AXIS_ALIGNED_COSINE: Real = 0.999;
/// Rate-of-change bound used for heightfields, whose triangle-plane distance can step across triangle edges.
const HEIGHTFIELD_LIPSCHITZ_BOUND: Real = 2.0;

// ---------------------------------------------------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------------------------------------------------

/// One side of a pair, as a distance field in its own local frame.
pub(crate) enum PairShape<'a> {
    /// A signed distance shape.
    Sdf(&'a SdfShapeData),
    /// A solver box, measured by its exact field.
    Cuboid { half_extents: Vector },
    /// A solver sphere.
    Ball { radius: Real },
    /// A solver half-space through the origin with an outward unit normal; unbounded.
    HalfSpace { normal: Vector },
    /// A solver heightfield (terrain), measured to the plane of the triangle under each point.
    HeightField(&'a HeightField),
}

/// A manifold point carried to the next step, stored in both shapes' frames so it moves with both bodies.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CachedContact {
    /// The optimized point in the first shape's frame.
    pub(crate) position1: Vector,
    /// The same point in the second shape's frame.
    pub(crate) position2: Vector,
    /// Contact normal in the first shape's frame.
    pub(crate) normal1: Vector,
    /// Whether the point was a contact (within the prediction distance).
    pub(crate) active: bool,
}

/// One manifold point in the first shape's frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ContactPoint {
    /// Optimized point `x*` (Eq. 4).
    pub(crate) position: Vector,
    /// Unit normal from the first shape toward the second.
    pub(crate) normal: Vector,
    /// Signed gap along the normal in meters; negative when the shapes overlap.
    pub(crate) separation: Real,
    /// Contact anchor on the first shape's surface.
    pub(crate) anchor1: Vector,
    /// Contact anchor on the second shape's surface; `anchor2 - anchor1 = normal * separation`.
    pub(crate) anchor2: Vector,
    /// Cache slot this point was warm started from, if any.
    pub(crate) cache_slot: Option<usize>,
    /// Value of Eq. 2 at the point.
    softmax: Real,
    /// Eq. 3 against the final manifold, used for ordering.
    rank: Real,
}

/// Work done by a search, for diagnostics.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SearchWork {
    /// Optimizer iterations.
    pub(crate) iterations: u32,
    /// Objective evaluations; each queries both fields.
    pub(crate) evaluations: u32,
}

/// One-sided contact measurement: a surface point of one shape read against the other shape's field.
#[derive(Clone, Copy)]
struct SurfaceReading {
    normal: Vector,
    separation: Real,
    anchor1: Vector,
    anchor2: Vector,
}

/// A cached point re-optimized this step: where it came from and how far the bodies carried it.
#[derive(Clone, Copy)]
struct CacheSeed {
    slot: usize,
    normal: Vector,
    advected: Real,
}

// ---------------------------------------------------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------------------------------------------------

/// Run Algorithm 1 for one pair and return the manifold sorted best first; its leading points form the next cache.
///
/// `prediction` is the solver's contact distance in meters: shapes closer than it get contacts, and the search domain
/// grows by half of it on each side. Returns no points when the bounds do not overlap.
pub(crate) fn search_contacts(
    shape1: &PairShape<'_>,
    shape2: &PairShape<'_>,
    pose12: &Pose,
    prediction: Real,
    cache: &[CachedContact],
    random: &mut SeededRandom,
    work: &mut SearchWork,
) -> Vec<ContactPoint> {
    let Some(domain) = search_domain(shape1, shape2, pose12, prediction) else {
        return Vec::new();
    };
    let Some(size) = pair_size(shape1, shape2) else {
        return Vec::new();
    };
    let mut search = ContactSearch {
        shape1,
        shape2,
        pose12,
        domain,
        tolerance: CONTACT_SEARCH.tolerance * size,
        smoothing: CONTACT_SEARCH.softmax_smoothing * (size * size),
        points: Vec::new(),
        work,
    };
    search.work.evaluations += 1;
    if !search.may_touch(prediction) {
        return Vec::new();
    }

    // Alg. 1, lines 4-10: warm start from the cache, carried with the bodies. Each cached point is stored in both
    // frames, so the midpoint moves with the average motion of the two bodies (Eq. 6) without needing velocities.
    let cached = cache.len().min(CONTACT_SEARCH.cache_size);
    for (slot, entry) in cache[..cached].iter().enumerate() {
        let carried2 = pose12.transform_point(entry.position2);
        let seed = (entry.position1 + carried2) * 0.5;
        let advected = (seed - entry.position1).length();
        search.add_point(
            seed,
            Some(CacheSeed {
                slot,
                normal: entry.normal1,
                advected,
            }),
        );
    }

    // Alg. 1, lines 12-18: stochastic samples find new contact regions and top the cache back up.
    let cached_contacts = cache[..cached].iter().filter(|entry| entry.active).count();
    let convex_pair_tracked = CONTACT_SEARCH.skip_convex_samples
        && shape1.convex()
        && shape2.convex()
        && cached_contacts == CONTACT_SEARCH.cache_size;
    let random_samples = if convex_pair_tracked { 0 } else { CONTACT_SEARCH.random_samples };
    let samples = random_samples + CONTACT_SEARCH.cache_size.saturating_sub(cached);
    for _ in 0..samples {
        let seed = search.stochastic_sample(random);
        search.add_point(seed, None);
    }

    search.finish(size)
}

/// Working state of one pair's search.
struct ContactSearch<'a> {
    shape1: &'a PairShape<'a>,
    shape2: &'a PairShape<'a>,
    pose12: &'a Pose,
    domain: Aabb,
    tolerance: Real,
    smoothing: Real,
    points: Vec<ContactPoint>,
    work: &'a mut SearchWork,
}

impl ContactSearch<'_> {
    /// Minimize the objective from `seed`, measure the contact there (Sec. 3.4), and add it to the manifold.
    fn add_point(&mut self, seed: Vector, cached: Option<CacheSeed>) {
        let point = match cached {
            // Bodies that barely moved relative to each other leave a cached point where it converged; re-optimizing
            // it would only let the repulsion shuffle it. Settled piles skip most of their optimizer work this way.
            Some(cached) if cached.advected < (CONTACT_SEARCH.settled_fraction * self.tolerance) => {
                seed.clamp(self.domain.mins, self.domain.maxs)
            }
            _ => self.optimize(seed, cached),
        };

        // The optimizer converges where both distances are about equal, so the paper's rule (Eq. 5, the more deeply
        // penetrating field at x* gives the normal) is ambiguous wherever an edge or corner meets a face, and tilts
        // resting contacts sideways. Instead, measure one-sidedly from both surfaces near x*, as SDF-mesh methods do:
        // project x* onto each shape's surface and read the other field there. The deeper reading wins; near-ties on
        // cached points keep the normal closest to the previous one, so contacts do not flip between steps.
        let from_first = self.measure_from_first(point);
        let from_second = self.measure_from_second(point);
        let reading = match (from_first, from_second) {
            (Some(first), Some(second)) => {
                let tied = (first.separation - second.separation).abs() < self.tolerance;
                match cached {
                    Some(cached) if tied => {
                        if first.normal.dot(cached.normal) >= second.normal.dot(cached.normal) {
                            first
                        } else {
                            second
                        }
                    }
                    _ => {
                        if first.separation <= second.separation {
                            first
                        } else {
                            second
                        }
                    }
                }
            }
            (Some(reading), None) | (None, Some(reading)) => reading,
            (None, None) => return,
        };

        self.points.push(ContactPoint {
            position: point,
            normal: reading.normal,
            separation: reading.separation,
            anchor1: reading.anchor1,
            anchor2: reading.anchor2,
            cache_slot: cached.map(|cached| cached.slot),
            softmax: self.depth(point),
            rank: 0.0,
        });
    }

    /// Minimize the objective from `seed` with Nelder-Mead and return the point to measure.
    ///
    /// The initial simplex covers a tenth of the domain. A cached point only has to follow the bodies, so its simplex
    /// covers how far it was carried (otherwise it would wander across flat faces), and it converges more tightly: a
    /// lagging contact on a rolling body tilts its normal and acts as drag. It also keeps its carried position unless
    /// the optimizer found a deeper one: on flat faces the depth is level and only the repulsion moves points, whose
    /// balance changes with the order points are found in, so without this they trade places from step to step.
    fn optimize(&mut self, seed: Vector, cached: Option<CacheSeed>) -> Vector {
        let mut step =
            (self.domain.extents() * CONTACT_SEARCH.simplex_fraction).max(Vector::splat(0.5 * self.tolerance));
        let mut tolerance = self.tolerance;
        if let Some(cached) = cached {
            let carried = cached.advected.max(CONTACT_SEARCH.cached_simplex_tolerances * self.tolerance);
            step = step.min(Vector::splat(carried));
            tolerance *= CONTACT_SEARCH.cached_tolerance_fraction;
        }
        let minimum = nelder_mead(|point| self.objective(point), seed, &self.domain, step, tolerance);
        self.work.iterations += minimum.iterations;
        self.work.evaluations += minimum.evaluations;

        if cached.is_none() {
            return minimum.point;
        }
        self.work.evaluations += 2;
        let carried_depth = self.depth(seed);
        if self.depth(minimum.point) > carried_depth - (CONTACT_SEARCH.cached_improvement * self.tolerance) {
            return seed.clamp(self.domain.mins, self.domain.maxs);
        }
        minimum.point
    }

    /// Project `point` onto the first shape's surface and read the second field there: the normal is the second
    /// shape's inward surface direction and the separation its signed distance.
    fn measure_from_first(&self, point: Vector) -> Option<SurfaceReading> {
        let surface1 = self.shape1.project(point, self.tolerance);
        let sample = self.sample2(surface1);
        let normal = -sample.gradient.normalize_or_zero();
        if normal.length_squared() < MIN_NORMAL_LENGTH_SQUARED {
            return None;
        }
        Some(SurfaceReading {
            normal,
            separation: sample.distance,
            anchor1: surface1,
            anchor2: surface1 + (normal * sample.distance),
        })
    }

    /// Project `point` onto the second shape's surface and read the first field there.
    fn measure_from_second(&self, point: Vector) -> Option<SurfaceReading> {
        let local = self.pose12.inverse_transform_point(point);
        let surface2 = self.pose12.transform_point(self.shape2.project(local, self.tolerance));
        let sample = self.shape1.sample(surface2);
        let normal = sample.gradient.normalize_or_zero();
        if normal.length_squared() < MIN_NORMAL_LENGTH_SQUARED {
            return None;
        }
        Some(SurfaceReading {
            normal,
            separation: sample.distance,
            anchor1: surface2 - (normal * sample.distance),
            anchor2: surface2,
        })
    }

    /// Eq. 2 alone: how deep a point lies in both shapes, without repulsion.
    fn depth(&self, point: Vector) -> Real {
        softmax(self.shape1.distance(point), self.distance2(point), self.smoothing)
    }

    /// The second shape's distance at a point of the first shape's frame.
    fn distance2(&self, point: Vector) -> Real {
        self.shape2.distance(self.pose12.inverse_transform_point(point))
    }

    /// Whether some point of the domain could lie within `prediction` of both surfaces. Distance fields change at a
    /// bounded rate, so a center farther from either surface than that rate times the domain's half-diagonal (plus
    /// the prediction distance) proves the domain holds no contact. Overlapping bounds of non-convex shapes often
    /// fail this test, which saves the whole search.
    fn may_touch(&self, prediction: Real) -> bool {
        let center = self.domain.center();
        let radius = self.domain.half_extents().length();
        let reach1 = (self.shape1.lipschitz_bound() * radius) + prediction;
        let reach2 = (self.shape2.lipschitz_bound() * radius) + prediction;
        self.shape1.distance(center) <= reach1 && self.distance2(center) <= reach2
    }

    /// Eq. 3: the smooth maximum of both distances minus the repulsion from points already in the manifold.
    fn objective(&self, point: Vector) -> Real {
        let repulsion: Real = self.points.iter().map(|found| (point - found.position).length()).sum();
        self.depth(point) - (CONTACT_SEARCH.repulsion * repulsion)
    }

    /// The second shape's field at a point of the first shape's frame, with its gradient in that frame.
    fn sample2(&self, point: Vector) -> FieldSample {
        let sample = self.shape2.sample(self.pose12.inverse_transform_point(point));
        FieldSample {
            distance: sample.distance,
            gradient: self.pose12.transform_vector(sample.gradient),
        }
    }

    /// STOCHASTICSAMPLE (Sec. 3.5.2): a uniformly chosen high-curvature seed of either shape inside the domain, or a
    /// uniform point of the domain when no seed lies inside it.
    fn stochastic_sample(&self, random: &mut SeededRandom) -> Vector {
        let mut sample =
            self.domain.mins + (Vector::new(unit(random), unit(random), unit(random)) * self.domain.extents());
        let mut candidates = 0_u32;
        let mut consider = |seed: Vector, random: &mut SeededRandom| {
            if !self.domain.contains_local_point(seed) {
                return;
            }
            // Reservoir sampling keeps each inside seed with equal probability without collecting them.
            candidates += 1;
            if ((unit(random) * candidates as Real) as u32).min(candidates - 1) == 0 {
                sample = seed;
            }
        };
        self.shape1.for_each_seed(|seed| consider(seed, random));

        // The second shape's seeds are culled against the domain's bounds in their own frame first, so only the few
        // near the overlap are transformed (the reference implementation culls both shapes this way).
        let local_domain = self.domain.transform_by(&self.pose12.inverse());
        self.shape2.for_each_seed(|seed| {
            if local_domain.contains_local_point(seed) {
                consider(self.pose12.transform_point(seed), random);
            }
        });
        sample
    }

    /// Alg. 1, lines 20-28: drop points that converged within the tolerance of an earlier point (cached points come
    /// first and survive), then sort by Eq. 3 against the final manifold, preferring deep points far from the others.
    fn finish(self, size: Real) -> Vec<ContactPoint> {
        let duplicate_squared = self.tolerance * self.tolerance;
        let mut unique: Vec<ContactPoint> = Vec::with_capacity(self.points.len());
        for point in &self.points {
            if unique.iter().all(|kept| (kept.position - point.position).length_squared() >= duplicate_squared) {
                unique.push(*point);
            }
        }

        let replacement = CONTACT_SEARCH.replacement_threshold * size;
        let positions: Vec<Vector> = unique.iter().map(|point| point.position).collect();
        for point in &mut unique {
            let spread: Real = positions.iter().map(|other| (point.position - *other).length()).sum();
            let novelty = if point.cache_slot.is_some() { 0.0 } else { replacement };
            point.rank = point.softmax + novelty - (CONTACT_SEARCH.repulsion * spread);
        }
        unique.sort_by(|left, right| left.rank.total_cmp(&right.rank));
        unique
    }
}

impl PairShape<'_> {
    /// Signed distance and gradient at a point of this shape's frame.
    fn sample(&self, point: Vector) -> FieldSample {
        match self {
            Self::Sdf(data) => data.sample(point),
            Self::Cuboid {
                half_extents,
            } => box_field(point, *half_extents),
            Self::Ball {
                radius,
            } => sphere_field(point, *radius),
            Self::HalfSpace {
                normal,
            } => half_space_field(point, *normal),
            Self::HeightField(heightfield) => heightfield_field(point, heightfield),
        }
    }

    /// Signed distance alone at a point of this shape's frame.
    fn distance(&self, point: Vector) -> Real {
        match self {
            Self::Sdf(data) => data.distance(point),
            _ => self.sample(point).distance,
        }
    }

    /// Upper bound on the field's rate of change in meters per meter.
    fn lipschitz_bound(&self) -> Real {
        match self {
            Self::Sdf(data) => data.lipschitz_bound(),
            Self::Cuboid {
                ..
            }
            | Self::Ball {
                ..
            }
            | Self::HalfSpace {
                ..
            } => 1.0,
            // The plane under a point changes across triangle edges, so the distance can step there; a looser bound
            // keeps the overlap test conservative, as for baked grids.
            Self::HeightField(_) => HEIGHTFIELD_LIPSCHITZ_BOUND,
        }
    }

    /// Move a point of this shape's frame onto its surface with Newton steps, which also suit inexact fields.
    fn project(&self, mut point: Vector, tolerance: Real) -> Vector {
        let precision = tolerance * PROJECTION_PRECISION;
        for _ in 0..PROJECTION_STEPS {
            let sample = self.sample(point);
            if sample.distance.abs() < precision {
                break;
            }
            point -= sample.gradient * (sample.distance / gradient_squared(sample));
        }
        point
    }

    /// Local bounds, or `None` for unbounded half-spaces.
    fn local_bounds(&self) -> Option<Aabb> {
        match self {
            Self::Sdf(data) => Some(data.bounds),
            Self::Cuboid {
                half_extents,
            } => Some(Aabb::new(-*half_extents, *half_extents)),
            Self::Ball {
                radius,
            } => Some(Aabb::new(Vector::splat(-*radius), Vector::splat(*radius))),
            Self::HalfSpace {
                ..
            } => None,
            Self::HeightField(heightfield) => Some(*heightfield.root_aabb()),
        }
    }

    /// Largest side of the bounds, or `None` for half-spaces.
    fn size(&self) -> Option<Real> {
        self.local_bounds().map(|bounds| bounds.extents().max_element())
    }

    fn convex(&self) -> bool {
        match self {
            Self::Sdf(data) => data.convex,
            Self::Cuboid {
                ..
            }
            | Self::Ball {
                ..
            }
            | Self::HalfSpace {
                ..
            } => true,
            Self::HeightField(_) => false,
        }
    }

    /// Visit the high-curvature seeds in this shape's frame. A solver box's corners are its sharpest points.
    fn for_each_seed(&self, mut visit: impl FnMut(Vector)) {
        match self {
            Self::Sdf(data) => data.seeds.iter().for_each(|seed| visit(*seed)),
            Self::Cuboid {
                half_extents,
            } => {
                for corner in 0..8 {
                    let signs = Vector::new(
                        if corner & 1 == 0 { -1.0 } else { 1.0 },
                        if corner & 2 == 0 { -1.0 } else { 1.0 },
                        if corner & 4 == 0 { -1.0 } else { 1.0 },
                    );
                    visit(*half_extents * signs);
                }
            }
            Self::Ball {
                ..
            }
            | Self::HalfSpace {
                ..
            }
            | Self::HeightField(_) => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Search domain V (Sec. 3.1): the overlap of both shapes' bounds, grown by half the prediction distance, in the first
/// shape's frame. A half-space contributes no bounds, but clips the domain to its side when it is axis aligned.
fn search_domain(shape1: &PairShape<'_>, shape2: &PairShape<'_>, pose12: &Pose, prediction: Real) -> Option<Aabb> {
    let margin = 0.5 * prediction;
    let bounds1 = shape1.local_bounds().map(|bounds| bounds.loosened(margin));
    let bounds2 = shape2.local_bounds().map(|bounds| bounds.transform_by(pose12).loosened(margin));
    let mut domain = match (bounds1, bounds2) {
        (Some(first), Some(second)) => first.intersection(&second)?,
        (Some(bounds), None) | (None, Some(bounds)) => bounds,
        (None, None) => return None,
    };

    // A half-space keeps the points with `normal · x <= offset`; the margin lets near contacts in.
    let plane = match (shape1, shape2) {
        (
            PairShape::HalfSpace {
                normal,
            },
            _,
        ) => Some((*normal, margin)),
        (
            _,
            PairShape::HalfSpace {
                normal,
            },
        ) => {
            let normal = pose12.transform_vector(*normal);
            Some((normal, normal.dot(pose12.translation) + margin))
        }
        _ => None,
    };
    if let Some((normal, offset)) = plane {
        for axis in 0..3 {
            if normal[axis] > AXIS_ALIGNED_COSINE {
                domain.maxs[axis] = domain.maxs[axis].min(offset / normal[axis]);
            } else if normal[axis] < -AXIS_ALIGNED_COSINE {
                domain.mins[axis] = domain.mins[axis].max(offset / normal[axis]);
            }
        }
    }

    let valid = domain.mins.is_finite() && domain.maxs.is_finite() && domain.mins.cmple(domain.maxs).all();
    valid.then_some(domain)
}

/// The smaller bounded shape's size; tolerances scale with it.
fn pair_size(shape1: &PairShape<'_>, shape2: &PairShape<'_>) -> Option<Real> {
    match (shape1.size(), shape2.size()) {
        (Some(first), Some(second)) => Some(first.min(second)),
        (Some(size), None) | (None, Some(size)) => Some(size),
        (None, None) => None,
    }
}

/// Eq. 2: a smooth upper bound of `max(first, second)`, exact when `smoothing` is zero.
fn softmax(first: Real, second: Real, smoothing: Real) -> Real {
    let difference = first - second;
    0.5 * (first + second + ((difference * difference) + smoothing).sqrt())
}

fn gradient_squared(sample: FieldSample) -> Real {
    sample.gradient.length_squared().max(MIN_GRADIENT_SQUARED)
}

/// Result of a Nelder-Mead run.
struct Minimum {
    point: Vector,
    iterations: u32,
    evaluations: u32,
}

/// Derivative-free Nelder-Mead simplex minimization (Sec. 3.3) inside `domain`, with the standard coefficients
/// (reflection 1, expansion 2, contraction and shrink 1/2). Trial points are projected onto the domain. Stops when the
/// simplex fits within `tolerance` of its best vertex, when the objective is flat across it, or after the iteration
/// limit. Only distances are queried, which makes it robust to discontinuous or inexact gradients.
fn nelder_mead(
    objective: impl Fn(Vector) -> Real,
    start: Vector,
    domain: &Aabb,
    step: Vector,
    tolerance: Real,
) -> Minimum {
    let project = |point: Vector| point.clamp(domain.mins, domain.maxs);
    let mut evaluations = 0;
    let mut evaluate = |point: Vector| {
        evaluations += 1;
        objective(point)
    };

    // Initial simplex: the start plus one step along each axis, pointing into the domain.
    let start = project(start);
    let mut vertices = [start; 4];
    for axis in 0..3 {
        let toward_inside = if start[axis] + step[axis] <= domain.maxs[axis] { step[axis] } else { -step[axis] };
        vertices[axis + 1][axis] += toward_inside;
        vertices[axis + 1] = project(vertices[axis + 1]);
    }
    let mut values = vertices.map(&mut evaluate);

    let mut iterations: u32 = 0;
    while (iterations as usize) < CONTACT_SEARCH.max_iterations {
        // Order the vertices from best to worst.
        for i in 1..4 {
            let mut j = i;
            while j > 0 && values[j] < values[j - 1] {
                values.swap(j, j - 1);
                vertices.swap(j, j - 1);
                j -= 1;
            }
        }

        let extent = (1..4).map(|i| (vertices[i] - vertices[0]).length()).fold(0.0, Real::max);
        if extent < tolerance || (values[3] - values[0]) < (FLAT_SIMPLEX_FRACTION * tolerance) {
            break;
        }
        iterations += 1;

        let centroid = (vertices[0] + vertices[1] + vertices[2]) / 3.0;
        let reflected = project(centroid + (centroid - vertices[3]));
        let reflected_value = evaluate(reflected);
        if reflected_value < values[0] {
            let expanded = project(centroid + ((centroid - vertices[3]) * 2.0));
            let expanded_value = evaluate(expanded);
            if expanded_value < reflected_value {
                vertices[3] = expanded;
                values[3] = expanded_value;
            } else {
                vertices[3] = reflected;
                values[3] = reflected_value;
            }
        } else if reflected_value < values[2] {
            vertices[3] = reflected;
            values[3] = reflected_value;
        } else {
            // Contract toward the better of the reflected and worst points; shrink toward the best if that fails.
            let toward = if reflected_value < values[3] { reflected } else { vertices[3] };
            let contracted = centroid + ((toward - centroid) * 0.5);
            let contracted_value = evaluate(contracted);
            if contracted_value < reflected_value.min(values[3]) {
                vertices[3] = contracted;
                values[3] = contracted_value;
            } else {
                for i in 1..4 {
                    vertices[i] = vertices[0] + ((vertices[i] - vertices[0]) * 0.5);
                    values[i] = evaluate(vertices[i]);
                }
            }
        }
    }

    let best = (1..4).fold(0, |best, i| if values[i] < values[best] { i } else { best });
    Minimum {
        point: vertices[best],
        iterations,
        evaluations,
    }
}
