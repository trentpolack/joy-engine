// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The frame's transparent queue: blended mesh ranges, transient transparent triangles, and particle batches sorted
//! back to front together.
//!
//! Every element is ordered by its view depth, the distance along the camera's forward axis: a blended mesh by the
//! center of its bounds, a triangle by its centroid, and a particle batch by its sort point, the center of its bounds
//! unless the particle frame names one (its particles are already ordered inside the batch). So a particle effect
//! behind a glass pane draws before the pane, and one in front draws after it. The sort is stable, so equal depths keep
//! the order elements were supplied in (meshes in scene order, then transient triangles, then particle batches).
//! Consecutive transient triangles merge into one draw.
//!
//! Intersecting transparent surfaces, and particle effects that overlap each other or a surface in depth, remain an
//! approximation, not order-independent transparency.

use std::ops::Range;

use joy_core::LinearColor;
use joy_core::math::Vec3;

use crate::frame::ColorVertex;
use crate::mesh::MeshVertex;

/// One draw of the transparent queue, in drawing order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TransparentStep {
    /// A blended mesh range: the position in [`TransparentQueue::mesh_order`], which is also its offset among the
    /// frame's blended instances.
    Mesh(usize),
    /// Unlit vertex-color triangles in the renderer's transient vertex stream.
    Colored(Range<u32>),
    /// A prepared particle batch, by its index in the particle pass's batches.
    Particles(usize),
}

/// The sorted queue: draws back to front, and the blended meshes in drawing order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TransparentQueue {
    /// Draws, back to front.
    pub(crate) steps: Vec<TransparentStep>,
    /// Blended mesh items (indices into the caller's list) in drawing order.
    pub(crate) mesh_order: Vec<usize>,
}

/// One sortable transparent element.
enum Element<'a> {
    Mesh(usize),
    Colored(&'a [ColorVertex; 3]),
    Particles(usize),
}

/// Sort every transparent element back to front for a camera at `eye` looking along unit `forward`.
///
/// `mesh_depths` holds each blended mesh item's view depth, in scene order, and `particle_depths` each prepared
/// particle batch's. Colored vertices are appended to `transient` (after whatever it already holds); an incomplete
/// trailing triangle is dropped.
pub(crate) fn build_transparent_queue(
    mesh_depths: &[f32],
    triangles: &[ColorVertex],
    particle_depths: impl Iterator<Item = f32>,
    eye: Vec3,
    forward: Vec3,
    transient: &mut Vec<MeshVertex>,
) -> TransparentQueue {
    // Gather every element with its view depth, in the order supplied.
    let mut elements: Vec<(f32, Element<'_>)> =
        mesh_depths.iter().enumerate().map(|(index, depth)| (*depth, Element::Mesh(index))).collect();
    for triangle in triangles.as_chunks::<3>().0 {
        let positions = triangle.each_ref().map(|vertex| vertex.position);
        elements.push((triangle_depth(positions, eye, forward), Element::Colored(triangle)));
    }
    elements.extend(particle_depths.enumerate().map(|(index, depth)| (depth, Element::Particles(index))));

    // Farthest first; the sort is stable, so equal depths keep the supplied order.
    elements.sort_by(|left, right| right.0.total_cmp(&left.0));

    let mut queue = TransparentQueue::default();
    for (_, element) in elements {
        match element {
            Element::Mesh(index) => {
                queue.steps.push(TransparentStep::Mesh(queue.mesh_order.len()));
                queue.mesh_order.push(index);
            }
            Element::Colored(triangle) => {
                let start = transient.len() as u32;
                transient.extend(
                    triangle.iter().map(|vertex| MeshVertex::colored(vertex.position, LinearColor::from(vertex.color))),
                );
                let end = transient.len() as u32;
                match queue.steps.last_mut() {
                    Some(TransparentStep::Colored(range)) if range.end == start => range.end = end,
                    _ => queue.steps.push(TransparentStep::Colored(start..end)),
                }
            }
            Element::Particles(index) => queue.steps.push(TransparentStep::Particles(index)),
        }
    }
    queue
}

fn triangle_depth(positions: [[f32; 3]; 3], eye: Vec3, forward: Vec3) -> f32 {
    let centroid = positions.iter().map(|position| Vec3::from_array(*position)).sum::<Vec3>() / 3.0;
    (centroid - eye).dot(forward)
}

#[cfg(test)]
#[path = "transparency.test.rs"]
mod tests;
