// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Narrow-phase bridge between the solver and SDF contact search.
//!
//! [`SdfQueryDispatcher`] runs first in the solver's dispatcher chain. Pairs without an SDF collider return
//! `Unsupported`, so the solver's default dispatcher handles them exactly as before. Pairs with one produce one contact
//! manifold per search point: manifolds carry a single normal, and SDF contacts can face many ways at once. The solver
//! merges manifolds with nearly parallel normals before solving.
//!
//! Each pair keeps a workspace with its deterministic random stream and the cached points that warm start the next
//! step. Cached points align with the pair's manifolds, so a carried point also carries its solver impulses.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use rapier3d::math::{Pose, Real, Vector};
use rapier3d::parry::query::details::NormalConstraints;
use rapier3d::parry::query::{
    ClosestPoints, Contact, ContactManifold, ContactManifoldsWorkspace, NonlinearRigidMotion,
    PersistentQueryDispatcher, QueryDispatcher, ShapeCastHit, ShapeCastOptions, ShapeDistance, ShapeIntersection,
    TrackedContact, TypedWorkspaceData, Unsupported, WorkspaceData,
};
use rapier3d::parry::shape::{Ball, Cuboid, HalfSpace, PackedFeatureId, Shape};

use super::SdfCollisionStatistics;
use super::sdf_collider::SdfCollider;
use super::sdf_contact::{CachedContact, PairShape, SearchWork, search_contacts};
use crate::core::SeededRandom;

/// Seed of every pair's sampling stream. Pairs start identically, so identical worlds stay identical.
const PAIR_RANDOM_SEED: i64 = 0x5DF_C0DE;

/// Collision work of the current fixed step, shared between a world and its dispatcher.
///
/// These are diagnostics only: counters never feed back into the simulation.
#[derive(Debug, Default)]
pub(crate) struct SdfCollisionCounters {
    pairs: AtomicU32,
    contacts: AtomicU32,
    iterations: AtomicU32,
    evaluations: AtomicU32,
}

impl SdfCollisionCounters {
    /// Start counting a new fixed step.
    pub(crate) fn reset(&self) {
        for counter in [&self.pairs, &self.contacts, &self.iterations, &self.evaluations] {
            counter.store(0, Ordering::Relaxed);
        }
    }

    /// Copy the counts.
    pub(crate) fn snapshot(&self) -> SdfCollisionStatistics {
        SdfCollisionStatistics {
            pairs: self.pairs.load(Ordering::Relaxed),
            contacts: self.contacts.load(Ordering::Relaxed),
            iterations: self.iterations.load(Ordering::Relaxed),
            distance_queries: self.evaluations.load(Ordering::Relaxed).saturating_mul(2),
        }
    }

    fn record(&self, contacts: u32, work: SearchWork) {
        self.pairs.fetch_add(1, Ordering::Relaxed);
        self.contacts.fetch_add(contacts, Ordering::Relaxed);
        self.iterations.fetch_add(work.iterations, Ordering::Relaxed);
        self.evaluations.fetch_add(work.evaluations, Ordering::Relaxed);
    }
}

/// Contact generation for pairs involving an [`SdfCollider`]; chain it before the solver's default dispatcher.
#[derive(Clone)]
pub(crate) struct SdfQueryDispatcher {
    counters: Arc<SdfCollisionCounters>,
}

impl SdfQueryDispatcher {
    pub(crate) fn new(counters: Arc<SdfCollisionCounters>) -> Self {
        Self { counters }
    }
}

/// Per-pair state kept by the solver between steps.
#[derive(Clone)]
struct SdfPairWorkspace {
    random: SeededRandom,
    /// Last step's points in manifold order; the search reads its leading entries as the cache.
    cache: Vec<CachedContact>,
}

impl WorkspaceData for SdfPairWorkspace {
    fn as_typed_workspace_data(&self) -> TypedWorkspaceData<'_> {
        TypedWorkspaceData::Custom
    }

    fn clone_dyn(&self) -> Box<dyn WorkspaceData> {
        Box::new(self.clone())
    }
}

impl<ManifoldData, ContactData> PersistentQueryDispatcher<ManifoldData, ContactData> for SdfQueryDispatcher
where
    ManifoldData: Default + Clone + Send + Sync,
    ContactData: Default + Copy + Send + Sync,
{
    /// Replace the pair's manifolds with this step's SDF contacts; `pos12` places the second shape in the first's
    /// frame and `prediction` is the contact distance in meters.
    fn contact_manifolds(
        &self,
        pos12: &Pose,
        g1: &dyn Shape,
        g2: &dyn Shape,
        prediction: Real,
        manifolds: &mut Vec<ContactManifold<ManifoldData, ContactData>>,
        workspace: &mut Option<ContactManifoldsWorkspace>,
    ) -> Result<(), Unsupported> {
        if !is_sdf(g1) && !is_sdf(g2) {
            return Err(Unsupported);
        }
        let (Some(shape1), Some(shape2)) = (pair_shape(g1), pair_shape(g2)) else {
            // Shapes without a distance field (meshes, compounds) never touch SDF colliders in this prototype.
            manifolds.clear();
            return Ok(());
        };

        let state = pair_workspace(workspace);
        if state.cache.len() != manifolds.len() {
            // Something else rewrote the manifolds; their order no longer matches the cache.
            state.cache.clear();
        }
        let mut work = SearchWork::default();
        let points = search_contacts(&shape1, &shape2, pos12, prediction, &state.cache, &mut state.random, &mut work);

        // One manifold per point. Points carried from the cache reuse their manifold and its solver impulses.
        let mut previous: Vec<Option<ContactManifold<ManifoldData, ContactData>>> =
            std::mem::take(manifolds).into_iter().map(Some).collect();
        state.cache.clear();
        let mut contacts = 0;
        for point in &points {
            let reused = point.cache_slot.and_then(|slot| previous.get_mut(slot).and_then(Option::take));
            let mut manifold = reused.unwrap_or_else(|| ContactManifold::with_data(0, 0, ManifoldData::default()));
            let warm_start = manifold.points.first().map(|contact| contact.data).unwrap_or_default();

            manifold.points.clear();
            manifold.local_n1 = point.normal;
            manifold.local_n2 = pos12.inverse_transform_vector(-point.normal);
            manifold.subshape1 = 0;
            manifold.subshape2 = 0;
            manifold.subshape_poses = None;
            let mut contact = TrackedContact::new(
                point.anchor1,
                pos12.inverse_transform_point(point.anchor2),
                PackedFeatureId::UNKNOWN,
                PackedFeatureId::UNKNOWN,
                point.separation,
            );
            contact.data = warm_start;
            manifold.points.push(contact);
            manifolds.push(manifold);

            let active = point.separation < prediction;
            contacts += u32::from(active);
            state.cache.push(CachedContact {
                position1: point.position,
                position2: pos12.inverse_transform_point(point.position),
                normal1: point.normal,
                active,
            });
        }
        self.counters.record(contacts, work);
        Ok(())
    }

    fn contact_manifold_convex_convex(
        &self,
        _pos12: &Pose,
        _g1: &dyn Shape,
        _g2: &dyn Shape,
        _normal_constraints1: Option<&dyn NormalConstraints>,
        _normal_constraints2: Option<&dyn NormalConstraints>,
        _prediction: Real,
        _manifold: &mut ContactManifold<ManifoldData, ContactData>,
    ) -> Result<(), Unsupported> {
        // Only composite shapes call this for their parts; SDF colliders are never parts.
        Err(Unsupported)
    }
}

/// Single-shot queries (sensors, shape casts, continuous collision) are not implemented for SDF colliders; they fall
/// through to the default dispatcher, which reports them unsupported, so SDF bodies are skipped by those queries.
impl QueryDispatcher for SdfQueryDispatcher {
    fn intersection_test(&self, _: &Pose, _: &dyn Shape, _: &dyn Shape) -> Result<ShapeIntersection, Unsupported> {
        Err(Unsupported)
    }

    fn distance(&self, _: &Pose, _: &dyn Shape, _: &dyn Shape) -> Result<ShapeDistance, Unsupported> {
        Err(Unsupported)
    }

    fn contact(&self, _: &Pose, _: &dyn Shape, _: &dyn Shape, _: Real) -> Result<Option<Contact>, Unsupported> {
        Err(Unsupported)
    }

    fn closest_points(&self, _: &Pose, _: &dyn Shape, _: &dyn Shape, _: Real) -> Result<ClosestPoints, Unsupported> {
        Err(Unsupported)
    }

    fn cast_shapes(
        &self,
        _: &Pose,
        _: Vector,
        _: &dyn Shape,
        _: &dyn Shape,
        _: ShapeCastOptions,
    ) -> Result<Option<ShapeCastHit>, Unsupported> {
        Err(Unsupported)
    }

    fn cast_shapes_nonlinear(
        &self,
        _: &NonlinearRigidMotion,
        _: &dyn Shape,
        _: &NonlinearRigidMotion,
        _: &dyn Shape,
        _: Real,
        _: Real,
        _: bool,
    ) -> Result<Option<ShapeCastHit>, Unsupported> {
        Err(Unsupported)
    }
}

fn is_sdf(shape: &dyn Shape) -> bool {
    (shape as &dyn std::any::Any).is::<SdfCollider>()
}

/// The distance field of a collider, when it has one the search can evaluate.
fn pair_shape(shape: &dyn Shape) -> Option<PairShape<'_>> {
    let any = shape as &dyn std::any::Any;
    if let Some(sdf) = any.downcast_ref::<SdfCollider>() {
        return Some(PairShape::Sdf(sdf.data()));
    }
    if let Some(cuboid) = any.downcast_ref::<Cuboid>() {
        return Some(PairShape::Cuboid { half_extents: cuboid.half_extents });
    }
    if let Some(ball) = any.downcast_ref::<Ball>() {
        return Some(PairShape::Ball { radius: ball.radius });
    }
    if let Some(half_space) = any.downcast_ref::<HalfSpace>() {
        return Some(PairShape::HalfSpace { normal: half_space.normal });
    }
    None
}

/// This pair's workspace, created on first contact or when the solver dropped it (for example after a shape change).
fn pair_workspace(workspace: &mut Option<ContactManifoldsWorkspace>) -> &mut SdfPairWorkspace {
    let ours = workspace.as_ref().is_some_and(|existing| existing.0.is::<SdfPairWorkspace>());
    if !ours {
        let fresh = SdfPairWorkspace { random: SeededRandom::new(PAIR_RANDOM_SEED), cache: Vec::new() };
        *workspace = Some(ContactManifoldsWorkspace(Box::new(fresh)));
    }
    workspace
        .as_mut()
        .and_then(|existing| existing.0.downcast_mut::<SdfPairWorkspace>())
        .expect("the pair workspace was just ensured to be an SdfPairWorkspace")
}
