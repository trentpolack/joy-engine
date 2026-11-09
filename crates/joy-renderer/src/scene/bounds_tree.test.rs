// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/scene/bounds_tree.rs`.

use joy_core::random::{
    RandomSource,
    SeededRandom,
};

use super::*;

/// Check every link, bound, and height, and that no node leans more than one level, returning the leaf count.
fn validate(tree: &BoundsTree, index: u32, parent: u32) -> usize {
    let node = tree.node(index);
    assert_eq!(node.parent, parent, "node {index} links to its parent");
    if node.is_leaf() {
        assert_eq!(node.height, 0);
        return 1;
    }
    let [left, right] = node.children;
    let (left_node, right_node) = (tree.node(left), tree.node(right));
    assert!(contains(node.bounds, left_node.bounds) && contains(node.bounds, right_node.bounds));
    assert_eq!(node.height, 1 + left_node.height.max(right_node.height));
    assert!(left_node.height.abs_diff(right_node.height) <= 1, "node {index} is balanced");
    validate(tree, left, index) + validate(tree, right, index)
}

fn random_box(random: &mut SeededRandom) -> Bounds {
    let mut value = |extent: f64| ((random.next_unit() * 2.0) - 1.0) as f32 * extent as f32;
    let center = Vec3::new(value(100.0), value(100.0), value(100.0));
    let half = Vec3::new(value(4.0).abs(), value(4.0).abs(), value(4.0).abs()) + Vec3::splat(0.01);
    Bounds {
        min: center - half,
        max: center + half,
    }
}

fn overlaps(left: &Bounds, right: &Bounds) -> bool {
    left.min.cmple(right.max).all() && right.min.cmple(left.max).all()
}

#[test]
fn edits_keep_the_tree_balanced_and_queries_exact() {
    let mut random = SeededRandom::new(77);
    let mut tree = BoundsTree::default();
    let mut items: Vec<Option<(TreeLeaf, Bounds)>> = Vec::new();
    for item in 0..500 {
        let bounds = random_box(&mut random);
        items.push(Some((tree.insert(bounds, item), bounds)));
    }

    for round in 0..8 {
        // Move, remove, and re-add items.
        for _ in 0..120 {
            let item = (random.next_unit() * items.len() as f64) as usize;
            match items[item] {
                Some((leaf, _)) if random.next_unit() < 0.3 => {
                    tree.remove(leaf);
                    items[item] = None;
                }
                Some((leaf, _)) => {
                    let bounds = random_box(&mut random);
                    tree.update(leaf, bounds);
                    items[item] = Some((leaf, bounds));
                }
                None => {
                    let bounds = random_box(&mut random);
                    items[item] = Some((tree.insert(bounds, item as u32), bounds));
                }
            }
        }
        let live = items.iter().flatten().count();
        assert_eq!(tree.len(), live);
        assert_eq!(validate(&tree, tree.root.unwrap(), NONE), live, "round {round}");

        // Every query finds exactly the items a scan finds.
        for _ in 0..10 {
            let volume = random_box(&mut random);
            let volume = Bounds {
                min: volume.min - Vec3::splat(20.0),
                max: volume.max + Vec3::splat(20.0),
            };
            let mut found = Vec::new();
            tree.query(
                |bounds| Overlap::reaching(overlaps(bounds, &volume)),
                |item, _| {
                    let (_, bounds) = items[item as usize].expect("only live items are visited");
                    if overlaps(&bounds, &volume) {
                        found.push(item);
                    }
                },
            );
            found.sort_unstable();
            let expected: Vec<u32> = (0..items.len())
                .filter(|item| items[*item].is_some_and(|(_, bounds)| overlaps(&bounds, &volume)))
                .map(|item| item as u32)
                .collect();
            assert_eq!(found, expected, "round {round}");
        }
    }
}
