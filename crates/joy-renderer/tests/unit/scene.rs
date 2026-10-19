// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene/mod.rs`.

use super::*;
use crate::mesh::MeshChannels;

fn asset() -> MeshAsset {
    MeshAsset::from_channels(MeshChannels {
        positions: &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: &[0, 1, 2],
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn nodes_compose_transforms_and_visibility() {
    use joy_core::math::Vec3;
    let mut scene = RenderScene::new();
    let moved =
        SceneNode { transform: Transform::new(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO, Vec3::splat(2.0)), visible: true };
    let root = scene.insert_node(moved, None).unwrap();
    let child = scene
        .insert_node(
            SceneNode { transform: Transform::new(Vec3::Y, Vec3::ZERO, Vec3::ONE), ..SceneNode::default() },
            Some(root),
        )
        .unwrap();
    let mesh = scene.insert(MeshInstance::new(asset()).with_transform(Transform::new(Vec3::Z, Vec3::ZERO, Vec3::ONE)));
    scene.set_parent(mesh, Some(child)).unwrap();
    assert_eq!(scene.parent(mesh), Some(child));
    // Local translations scale by the root and accumulate: (10, 0, 0) + 2 * ((0, 1, 0) + (0, 0, 1)).
    let world = scene.world_matrix(mesh).unwrap().transform_point3(Vec3::ZERO);
    assert!(world.abs_diff_eq(Vec3::new(10.0, 2.0, 2.0), 1e-5), "{world}");
    assert!(
        scene
            .node_world_matrix(child)
            .unwrap()
            .transform_point3(Vec3::ZERO)
            .abs_diff_eq(Vec3::new(10.0, 2.0, 0.0), 1e-5)
    );
    assert_eq!(scene.world_visible(mesh), Some(true));
    scene.node_mut(root).unwrap().visible = false;
    assert_eq!(scene.world_visible(mesh), Some(false));
}

#[test]
fn parenting_refuses_cycles_and_removal_takes_the_subtree() {
    let mut scene = RenderScene::new();
    let root = scene.insert_node(SceneNode::default(), None).unwrap();
    let child = scene.insert_node(SceneNode::default(), Some(root)).unwrap();
    let grandchild = scene.insert_node(SceneNode::default(), Some(child)).unwrap();
    assert_eq!(scene.set_node_parent(root, Some(grandchild)), Err(SceneGraphError::Cycle));
    assert_eq!(scene.set_node_parent(root, Some(root)), Err(SceneGraphError::Cycle));
    assert_eq!(scene.node_parent(root), None);
    let sibling = scene.insert_node(SceneNode::default(), None).unwrap();
    let kept = scene.create_mesh(asset());
    scene.set_parent(kept, Some(sibling)).unwrap();
    let doomed = scene.create_mesh(asset());
    scene.set_parent(doomed, Some(grandchild)).unwrap();

    assert!(scene.remove_node(child));
    assert!(!scene.remove_node(child));
    assert!(scene.node(grandchild).is_none() && scene.get(doomed).is_none());
    assert!(scene.node(root).is_some() && scene.get(kept).is_some());
    assert_eq!(scene.set_parent(kept, Some(grandchild)), Err(SceneGraphError::UnknownParent));
    assert_eq!(scene.insert_node(SceneNode::default(), Some(child)), Err(SceneGraphError::UnknownParent));
    // A recycled slot never aliases the removed node.
    let recycled = scene.insert_node(SceneNode::default(), None).unwrap();
    assert_ne!(recycled, child);
    scene.clear();
    assert!(scene.node(root).is_none() && scene.is_empty());
}

#[test]
fn stale_handles_do_not_alias_new_instances() {
    let mut scene = RenderScene::new();
    let first = scene.create_mesh(asset());
    assert!(scene.remove(first));
    let second = scene.create_mesh(asset());
    assert!(scene.get(first).is_none());
    assert!(scene.get(second).is_some());
    assert!(!scene.remove(first));
    assert_eq!(scene.len(), 1);
}

#[test]
fn collections_prune_missing_keys() {
    let mut scene = RenderScene::new();
    let mut collection = MeshCollection::new();
    let shared = asset();

    collection.begin_update();
    collection.set(&mut scene, "a", &shared).tint = LinearColor::rgb(1.0, 0.0, 0.0);
    collection.set(&mut scene, "b", &shared);
    collection.end_update(&mut scene);
    assert_eq!(scene.len(), 2);

    collection.begin_update();
    let retained = collection.set(&mut scene, "a", &shared);
    assert_eq!(retained.tint, LinearColor::rgb(1.0, 0.0, 0.0));
    collection.end_update(&mut scene);
    assert_eq!(scene.len(), 1);
    assert_eq!(collection.len(), 1);
}
