// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A dynamic bounding-volume hierarchy over world-space bounds: the spatial index that camera and shadow culling query.
//!
//! Leaves hold one item (a render proxy's slot index) with its bounds grown by a margin, so an item that moves a
//! little stays inside its leaf and costs no tree edit; one that leaves its margin is removed and reinserted.
//! Internal nodes bound their two children. Insertion descends toward the sibling whose bounds grow least in surface
//! area, and every edit rebalances its path with rotations, so queries stay logarithmic however items arrive and move.
//!
//! The tree only prunes. A query classifies node bounds with a conservative volume test (one that never reports a box
//! outside the volume while any part of it could be inside), skips the tests below a node wholly inside the volume, and
//! the caller tests each visited item's exact bounds, so a query returns exactly what a linear scan with the same test
//! returns.
//!
//! A loose grid would update more cheaply but needs one cell size, which suits neither a field of crates under one
//! huge ground slab nor a tool previewing a single asset; a tree rebuilt every frame costs `n log n` even when nothing
//! moves.

use joy_core::math::Vec3;

use crate::culling::Overlap;
use crate::mesh::Bounds;

/// The index that marks a missing node: no parent (the root), no children (a leaf), or an empty tree.
const NONE: u32 = u32::MAX;

/// Smallest margin a leaf's bounds grow by on each side, in world units.
const LEAF_MARGIN: f32 = 0.1;

/// Margin a leaf's bounds grow by on each side, as a fraction of the item's largest extent.
const LEAF_MARGIN_FRACTION: f32 = 0.1;

/// A leaf whose margin has grown past this multiple of a fresh leaf's surface area is refitted on its next update,
/// so an item that shrank stops widening every query that reaches it.
const LOOSE_LEAF_AREA_RATIO: f32 = 4.0;

/// Expected stack depth of a query; a balanced tree of a million items is about forty levels deep.
const QUERY_STACK_CAPACITY: usize = 64;

/// Handle to one leaf of a [`BoundsTree`], valid until the leaf is removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TreeLeaf(u32);

/// One node: a leaf holding an item, or an internal node bounding two children.
#[derive(Clone, Copy, Debug)]
struct TreeNode {
    /// For a leaf, the item's bounds grown by its margin; for an internal node, the union of its children's bounds.
    bounds: Bounds,
    parent: u32,
    /// Both [`NONE`] for a leaf.
    children: [u32; 2],
    /// Zero for a leaf; one more than the taller child for an internal node.
    height: u32,
    /// The item a leaf holds; unused by internal nodes.
    item: u32,
}

impl TreeNode {
    fn is_leaf(&self) -> bool {
        self.children[0] == NONE
    }
}

/// A dynamic AABB tree of items (render proxy slots). The owner keeps each item's [`TreeLeaf`] to update or remove it.
#[derive(Debug, Default)]
pub(crate) struct BoundsTree {
    nodes: Vec<TreeNode>,
    /// Node indices free for reuse.
    free: Vec<u32>,
    root: Option<u32>,
    leaves: usize,
}

impl BoundsTree {
    /// Items in the tree.
    pub(crate) fn len(&self) -> usize {
        self.leaves
    }

    /// Remove every item; outstanding leaves become invalid.
    pub(crate) fn clear(&mut self) {
        self.nodes.clear();
        self.free.clear();
        self.root = None;
        self.leaves = 0;
    }

    /// Add `item` with world `bounds` (finite; callers keep non-finite bounds out of the tree) and return its leaf.
    pub(crate) fn insert(&mut self, bounds: Bounds, item: u32) -> TreeLeaf {
        let leaf = self.allocate(TreeNode {
            bounds: grown(bounds),
            parent: NONE,
            children: [NONE; 2],
            height: 0,
            item,
        });
        self.insert_leaf(leaf);
        self.leaves += 1;
        TreeLeaf(leaf)
    }

    /// Remove a leaf.
    pub(crate) fn remove(&mut self, leaf: TreeLeaf) {
        self.remove_leaf(leaf.0);
        self.release(leaf.0);
        self.leaves -= 1;
    }

    /// Move a leaf's item to new world `bounds`. The tree changes only when the bounds leave the leaf's margin, or the
    /// margin has grown far looser than the item needs; returns whether the leaf was reinserted.
    pub(crate) fn update(&mut self, leaf: TreeLeaf, bounds: Bounds) -> bool {
        let current = self.nodes[leaf.0 as usize].bounds;
        let fresh = grown(bounds);
        let loose = surface_area(current) > (surface_area(fresh) * LOOSE_LEAF_AREA_RATIO);
        if contains(current, bounds) && !loose {
            return false;
        }
        self.remove_leaf(leaf.0);
        self.nodes[leaf.0 as usize].bounds = fresh;
        self.insert_leaf(leaf.0);
        true
    }

    /// Visit every item whose leaf bounds `classify` does not place outside the volume, pruning every subtree whose
    /// bounds it does, and classifying nothing below a node it places wholly inside.
    ///
    /// `classify` must be conservative and monotone: a box outside must contain no box that is not, and a box inside
    /// must contain only boxes inside. Each visit receives the item and how its leaf bounds lie: an item whose leaf is
    /// [`Overlap::Inside`] lies inside the volume by at least its margin, and otherwise callers test the item's exact
    /// bounds themselves. Items arrive in tree order, not insertion order.
    pub(crate) fn query(&self, mut classify: impl FnMut(&Bounds) -> Overlap, mut visit: impl FnMut(u32, Overlap)) {
        let Some(root) = self.root else {
            return;
        };
        // Each entry carries whether an ancestor already lay wholly inside the volume.
        let mut stack = Vec::with_capacity(QUERY_STACK_CAPACITY);
        stack.push((root, false));
        while let Some((index, inside)) = stack.pop() {
            let node = &self.nodes[index as usize];
            let overlap = if inside { Overlap::Inside } else { classify(&node.bounds) };
            if overlap == Overlap::Outside {
                continue;
            }
            if node.is_leaf() {
                visit(node.item, overlap);
            } else {
                let inside = overlap == Overlap::Inside;
                stack.push((node.children[1], inside));
                stack.push((node.children[0], inside));
            }
        }
    }

    // --- Tree edits -----------------------------------------------------------------------------------------------

    /// Place a detached leaf beside the sibling that grows the tree least, then refit and rebalance its path.
    fn insert_leaf(&mut self, leaf: u32) {
        let Some(root) = self.root else {
            self.root = Some(leaf);
            self.node_mut(leaf).parent = NONE;
            return;
        };

        // Descend while splitting a child is cheaper than pairing with the whole subtree (surface-area heuristic).
        let leaf_bounds = self.node(leaf).bounds;
        let mut index = root;
        while !self.node(index).is_leaf() {
            let node = *self.node(index);
            let area = surface_area(node.bounds);
            let combined_area = surface_area(union(node.bounds, leaf_bounds));

            // Pairing here creates a parent over this node and the leaf; descending pushes the growth into a child.
            let pair_cost = 2.0 * combined_area;
            let inherited_cost = 2.0 * (combined_area - area);
            let child_cost = |child: u32| {
                let child_node = self.node(child);
                let enlarged = surface_area(union(child_node.bounds, leaf_bounds));
                if child_node.is_leaf() {
                    enlarged + inherited_cost
                } else {
                    (enlarged - surface_area(child_node.bounds)) + inherited_cost
                }
            };
            let left_cost = child_cost(node.children[0]);
            let right_cost = child_cost(node.children[1]);
            if pair_cost < left_cost && pair_cost < right_cost {
                break;
            }
            index = if left_cost < right_cost { node.children[0] } else { node.children[1] };
        }

        // A new parent takes the sibling's place and holds the sibling and the leaf.
        let sibling = index;
        let old_parent = self.node(sibling).parent;
        let sibling_node = *self.node(sibling);
        let parent = self.allocate(TreeNode {
            bounds: union(sibling_node.bounds, leaf_bounds),
            parent: old_parent,
            children: [sibling, leaf],
            height: sibling_node.height + 1,
            item: NONE,
        });
        self.replace_child(old_parent, sibling, parent);
        self.node_mut(sibling).parent = parent;
        self.node_mut(leaf).parent = parent;
        self.refit_upward(parent);
    }

    /// Detach a leaf (it stays allocated): its sibling takes its parent's place, and the path above is refit.
    fn remove_leaf(&mut self, leaf: u32) {
        if self.root == Some(leaf) {
            self.root = None;
            return;
        }
        let parent = self.node(leaf).parent;
        let grandparent = self.node(parent).parent;
        let [left, right] = self.node(parent).children;
        let sibling = if left == leaf { right } else { left };

        self.replace_child(grandparent, parent, sibling);
        self.node_mut(sibling).parent = grandparent;
        self.release(parent);
        if grandparent != NONE {
            self.refit_upward(grandparent);
        }
    }

    /// Rebalance, then refit the bounds and height of `index` and every ancestor.
    fn refit_upward(&mut self, mut index: u32) {
        while index != NONE {
            index = self.balance(index);
            let [left, right] = self.node(index).children;
            let (left, right) = (*self.node(left), *self.node(right));
            let node = self.node_mut(index);
            node.bounds = union(left.bounds, right.bounds);
            node.height = 1 + left.height.max(right.height);
            index = node.parent;
        }
    }

    /// Rotate the taller grandchild of `a` up when its children's heights differ by more than one; returns the node
    /// now in `a`'s place.
    fn balance(&mut self, a: u32) -> u32 {
        let node_a = *self.node(a);
        if node_a.is_leaf() || node_a.height < 2 {
            return a;
        }
        let [b, c] = node_a.children;
        let difference = self.node(c).height as i64 - self.node(b).height as i64;

        if difference > 1 {
            // Rotate C up: C takes A's place, A keeps B and takes C's shorter child.
            let [f, g] = self.node(c).children;
            self.node_mut(c).children[0] = a;
            self.node_mut(c).parent = node_a.parent;
            self.node_mut(a).parent = c;
            self.replace_child(node_a.parent, a, c);
            let (kept, moved) = if self.node(f).height > self.node(g).height { (f, g) } else { (g, f) };
            self.node_mut(c).children[1] = kept;
            self.node_mut(a).children[1] = moved;
            self.node_mut(moved).parent = a;
            self.refit_pair(a, c, b, moved, kept);
            return c;
        }

        if difference < -1 {
            // Rotate B up: B takes A's place, A keeps C and takes B's shorter child.
            let [d, e] = self.node(b).children;
            self.node_mut(b).children[0] = a;
            self.node_mut(b).parent = node_a.parent;
            self.node_mut(a).parent = b;
            self.replace_child(node_a.parent, a, b);
            let (kept, moved) = if self.node(d).height > self.node(e).height { (d, e) } else { (e, d) };
            self.node_mut(b).children[1] = kept;
            self.node_mut(a).children[0] = moved;
            self.node_mut(moved).parent = a;
            self.refit_pair(a, b, c, moved, kept);
            return b;
        }
        a
    }

    /// After a rotation: `lower` now holds `stay` and `moved`, and `upper` holds `lower` and `kept`.
    fn refit_pair(&mut self, lower: u32, upper: u32, stay: u32, moved: u32, kept: u32) {
        let (stay, moved, kept) = (*self.node(stay), *self.node(moved), *self.node(kept));
        let lower_bounds = union(stay.bounds, moved.bounds);
        let lower_height = 1 + stay.height.max(moved.height);
        let node = self.node_mut(lower);
        node.bounds = lower_bounds;
        node.height = lower_height;
        let node = self.node_mut(upper);
        node.bounds = union(lower_bounds, kept.bounds);
        node.height = 1 + lower_height.max(kept.height);
    }

    /// Point `parent`'s link to `old` at `new` instead; with no parent, `new` becomes the root.
    fn replace_child(&mut self, parent: u32, old: u32, new: u32) {
        if parent == NONE {
            self.root = Some(new);
            return;
        }
        let node = self.node_mut(parent);
        if node.children[0] == old {
            node.children[0] = new;
        } else {
            node.children[1] = new;
        }
    }

    // --- Node storage ---------------------------------------------------------------------------------------------

    fn allocate(&mut self, node: TreeNode) -> u32 {
        if let Some(index) = self.free.pop() {
            self.nodes[index as usize] = node;
            return index;
        }
        self.nodes.push(node);
        (self.nodes.len() - 1) as u32
    }

    fn release(&mut self, index: u32) {
        self.free.push(index);
    }

    fn node(&self, index: u32) -> &TreeNode {
        &self.nodes[index as usize]
    }

    fn node_mut(&mut self, index: u32) -> &mut TreeNode {
        &mut self.nodes[index as usize]
    }
}

/// An item's bounds grown by its leaf margin.
fn grown(bounds: Bounds) -> Bounds {
    let extent = (bounds.max - bounds.min).max_element();
    let margin = Vec3::splat(LEAF_MARGIN.max(extent * LEAF_MARGIN_FRACTION));
    Bounds {
        min: bounds.min - margin,
        max: bounds.max + margin,
    }
}

fn union(left: Bounds, right: Bounds) -> Bounds {
    Bounds {
        min: left.min.min(right.min),
        max: left.max.max(right.max),
    }
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer.min.cmple(inner.min).all() && outer.max.cmpge(inner.max).all()
}

/// Surface area, the cost the insertion heuristic minimizes: a query volume is about as likely to reach a box as the
/// box is large in every direction.
fn surface_area(bounds: Bounds) -> f32 {
    let size = bounds.max - bounds.min;
    2.0 * ((size.x * size.y) + (size.y * size.z) + (size.z * size.x))
}

#[cfg(test)]
#[path = "bounds_tree.test.rs"]
mod tests;
