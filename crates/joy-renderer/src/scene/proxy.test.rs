// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene/proxy.rs`.

use joy_core::math::{
    DVec3,
    Vec3,
};
use joy_core::random::{
    RandomSource,
    SeededRandom,
};
use joy_render_types::Mesh;

use super::*;
use crate::camera::{
    look_at,
    perspective,
};
use crate::culling::{
    ClipPlanes,
    Overlap,
    bounds_outside_view,
    outside_frustum,
    sphere_touches_bounds,
};
use crate::mesh::MeshAsset;
use crate::scene::{
    MeshInstanceId,
    SceneNode,
};

/// A unit box asset centered on the origin.
fn box_asset() -> MeshAsset {
    let corners: Vec<DVec3> = (0..8)
        .map(|corner| {
            let axis = |bit: usize| if corner & bit != 0 { 0.5 } else { -0.5 };
            DVec3::new(axis(1), axis(2), axis(4))
        })
        .collect();
    let indices = vec![0, 1, 3, 0, 3, 2, 4, 6, 7, 4, 7, 5, 0, 4, 5, 0, 5, 1, 2, 3, 7, 2, 7, 6];
    MeshAsset::new(&Mesh::new(corners, indices)).expect("box geometry is valid")
}

fn uniform(random: &mut SeededRandom, minimum: f32, maximum: f32) -> f32 {
    minimum + ((random.next_unit() as f32) * (maximum - minimum))
}

fn random_vector(random: &mut SeededRandom, extent: f32) -> Vec3 {
    Vec3::new(uniform(random, -extent, extent), uniform(random, -extent, extent), uniform(random, -extent, extent))
}

fn random_transform(random: &mut SeededRandom) -> Transform {
    let scale = Vec3::new(uniform(random, 0.2, 3.0), uniform(random, 0.2, 3.0), uniform(random, 0.2, 3.0));
    Transform::new(random_vector(random, 60.0), random_vector(random, 3.0), scale)
}

/// One culling volume: a camera frustum (tested as the camera tests instances) or a light's sphere.
enum Volume {
    Frustum(Mat4),
    Sphere(Vec3, f32),
}

impl Volume {
    /// How the bounds tree prunes with the volume, as frames do: clip planes for a view, the sphere test for a light.
    fn classify(&self, bounds: &Bounds) -> Overlap {
        match self {
            Self::Frustum(view_projection) => ClipPlanes::new(*view_projection).classify(bounds),
            Self::Sphere(..) => Overlap::reaching(self.reaches(bounds)),
        }
    }

    /// The conservative box test for the volume.
    fn reaches(&self, bounds: &Bounds) -> bool {
        match self {
            Self::Frustum(view_projection) => !bounds_outside_view(*bounds, *view_projection),
            Self::Sphere(center, radius) => sphere_touches_bounds(*center, *radius, *bounds),
        }
    }

    /// The exact test a frame applies to one instance.
    fn holds(&self, instance: &MeshInstance, world: Mat4, bounds: Bounds) -> bool {
        match self {
            Self::Frustum(view_projection) => !outside_frustum(instance.asset.bounds(), world, *view_projection),
            Self::Sphere(..) => self.reaches(&bounds),
        }
    }
}

fn random_volumes(random: &mut SeededRandom) -> Vec<Volume> {
    let mut volumes = Vec::new();
    for _ in 0..6 {
        let eye = random_vector(random, 80.0);
        let focus = random_vector(random, 20.0);
        let projection = perspective(uniform(random, 0.4, 1.6), uniform(random, 0.6, 2.0), 0.5, 90.0);
        volumes.push(Volume::Frustum(projection * look_at(eye, focus, Vec3::Y)));
        volumes.push(Volume::Sphere(random_vector(random, 60.0), uniform(random, 2.0, 30.0)));
    }
    volumes
}

/// Slots every instance in `volume` occupies, found by composing and testing every instance in turn.
fn brute_force(scene: &RenderScene, volume: &Volume) -> Vec<u32> {
    let mut found = Vec::new();
    for (id, instance) in scene.iter() {
        let (world, shown) = scene.compose(instance.parent, instance.transform.matrix(), instance.visible);
        if !shown || instance.opacity <= 0.0 {
            continue;
        }
        let bounds = world_bounds(instance.asset.bounds(), world);
        if volume.holds(instance, world, bounds) {
            found.push(id.slot());
        }
    }
    found
}

/// Slots the bounds tree finds in `volume`, with the same exact test applied to each candidate.
fn tree_query(scene: &RenderScene, volume: &Volume) -> Vec<u32> {
    let proxies = scene.proxies();
    let mut found = Vec::new();
    proxies.query(
        |bounds| volume.classify(bounds),
        |index, proxy, overlap| {
            let (_, instance) = scene.instance_at(index).expect("drawn proxies hold live instances");
            if overlap == Overlap::Inside || volume.holds(instance, proxy.world, proxy.bounds) {
                found.push(index);
            }
        },
    );
    found.sort_unstable();
    found
}

fn assert_tree_matches_scan(scene: &RenderScene, volumes: &[Volume], round: usize) {
    let mut found = 0;
    for (index, volume) in volumes.iter().enumerate() {
        let expected = brute_force(scene, volume);
        assert_eq!(tree_query(scene, volume), expected, "round {round}, volume {index}");
        found += expected.len();
    }
    assert!(found > 0, "the volumes reach some instances");
}

#[test]
fn tree_queries_match_a_brute_force_scan_as_instances_move_arrive_and_leave() {
    let mut random = SeededRandom::new(1234);
    let asset = box_asset();
    let mut scene = RenderScene::new();
    let root = scene.insert_node(SceneNode::default(), None).unwrap();
    let arm = scene
        .insert_node(
            SceneNode {
                transform: Transform::new(Vec3::new(5.0, 0.0, 0.0), Vec3::new(0.0, 0.7, 0.0), Vec3::ONE),
                ..SceneNode::default()
            },
            Some(root),
        )
        .unwrap();
    let mut live: Vec<MeshInstanceId> = Vec::new();
    for index in 0..400 {
        let id = scene.insert(MeshInstance::new(asset.clone()).with_transform(random_transform(&mut random)));
        if index % 7 == 0 {
            scene.set_parent(id, Some(if index % 2 == 0 { root } else { arm })).unwrap();
        }
        live.push(id);
    }
    let volumes = random_volumes(&mut random);
    assert_tree_matches_scan(&scene, &volumes, 0);

    for round in 1..=6 {
        // Move some instances, some far enough to leave their leaf margins and some barely.
        for _ in 0..60 {
            let id = live[(random.next_unit() * live.len() as f64) as usize];
            let instance = scene.get_mut(id).unwrap();
            if random.next_unit() < 0.5 {
                instance.transform = random_transform(&mut random);
            } else {
                instance.transform.position += random_vector(&mut random, 0.05);
            }
        }
        // Remove some, add some (recycling slots), and hide or fade a few.
        for _ in 0..40 {
            let position = (random.next_unit() * live.len() as f64) as usize;
            assert!(scene.remove(live.swap_remove(position)));
        }
        for _ in 0..50 {
            live.push(scene.insert(MeshInstance::new(asset.clone()).with_transform(random_transform(&mut random))));
        }
        for _ in 0..10 {
            let id = live[(random.next_unit() * live.len() as f64) as usize];
            let instance = scene.get_mut(id).unwrap();
            if random.next_unit() < 0.5 {
                instance.visible = !instance.visible;
            } else {
                instance.opacity = if instance.opacity > 0.0 { 0.0 } else { 1.0 };
            }
        }
        // Swing the parented instances.
        scene.node_mut(root).unwrap().transform.rotation.y += 0.4;
        if round == 3 {
            scene.node_mut(arm).unwrap().visible = false;
        }
        if round == 5 {
            scene.node_mut(arm).unwrap().visible = true;
            scene.set_node_parent(arm, None).unwrap();
        }
        assert_tree_matches_scan(&scene, &volumes, round);
    }
}

#[test]
fn editing_a_node_marks_and_refreshes_every_instance_beneath_it() {
    let asset = box_asset();
    let mut scene = RenderScene::new();
    let root = scene.insert_node(SceneNode::default(), None).unwrap();
    let child = scene.insert_node(SceneNode::default(), Some(root)).unwrap();
    let under_child = scene.create_mesh(asset.clone());
    scene.set_parent(under_child, Some(child)).unwrap();
    let under_root = scene.create_mesh(asset.clone());
    scene.set_parent(under_root, Some(root)).unwrap();
    let unparented = scene.create_mesh(asset);
    let revision = scene.proxies().revision();

    // Moving the root marks both instances beneath it (directly and through the child node), and nothing else.
    scene.node_mut(root).unwrap().transform.position = Vec3::new(0.0, 4.0, 0.0);
    {
        let mut proxies = scene.proxies.lock().unwrap();
        proxies.mark_beneath_pending_nodes(&scene);
        assert!(proxies.pending(under_child.slot()).intersects(ProxyChanges::HIERARCHY));
        assert!(proxies.pending(under_root.slot()).intersects(ProxyChanges::HIERARCHY));
        assert!(proxies.pending(unparented.slot()).is_empty());
    }

    // The refresh moves both, and records each as a moved caster.
    let proxies = scene.proxies();
    for id in [under_child, under_root] {
        let proxy = proxies.get(id.slot()).unwrap();
        assert_eq!(proxy.world, scene.world_matrix(id).unwrap());
        assert!((proxy.bounds.center() - Vec3::new(0.0, 4.0, 0.0)).length() < 1e-5);
    }
    assert_eq!(proxies.get(unparented.slot()).unwrap().world, Mat4::IDENTITY);
    let changes = proxies.caster_changes_since(revision).unwrap();
    assert_eq!(changes.len(), 2, "{changes:?}");
    assert!(changes.iter().all(|change| change.before.is_some() && change.after.is_some()));
    drop(proxies);

    // Hiding the child node stops the instance beneath it from drawing or casting; a borrow that changes nothing
    // records nothing.
    let revision = scene.proxies().revision();
    scene.node_mut(child).unwrap().visible = false;
    let _ = scene.node_mut(root);
    let proxies = scene.proxies();
    assert!(!proxies.get(under_child.slot()).unwrap().drawn);
    assert!(proxies.get(under_root.slot()).unwrap().drawn);
    let changes = proxies.caster_changes_since(revision).unwrap();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].before.is_some() && changes[0].after.is_none());
}
