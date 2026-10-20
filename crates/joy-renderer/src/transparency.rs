// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The frame's transparent queue: blended mesh ranges, transient transparent triangles, and caller-supplied runs (such
//! as particle batches) sorted back to front together.
//!
//! Every element is ordered by its view depth, the distance along the camera's forward axis: a blended mesh by the
//! center of its bounds and a triangle by its centroid. So a particle behind a glass pane draws before the pane, and one
//! in front draws after it, whichever list each came from. The sort is stable, so equal depths keep the order elements
//! were supplied in (meshes in scene order, then transient triangles, then caller runs), and already-sorted runs such
//! as particle batches keep their order. Consecutive triangles that share a pipeline and texture merge into one draw.
//!
//! Intersecting transparent surfaces remain an approximation, not order-independent transparency.

use std::ops::Range;

use joy_core::LinearColor;
use joy_core::math::Vec3;

use crate::frame::{
    ColorVertex,
    TexturedVertex,
    TransparentDraw,
};
use crate::mesh::MeshVertex;

/// One draw of the transparent queue, in drawing order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TransparentStep {
    /// A blended mesh range: the position in [`TransparentQueue::mesh_order`], which is also its offset among the
    /// frame's blended instances.
    Mesh(usize),
    /// Unlit vertex-color triangles in the renderer's transient vertex stream.
    Colored(Range<u32>),
    /// Textured triangles in the textured pass's vertex stream, sampling one texture.
    Textured { texture: u64, vertices: Range<u32> },
}

/// The sorted queue and the vertex streams its steps draw from.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct TransparentQueue {
    /// Draws, back to front.
    pub(crate) steps: Vec<TransparentStep>,
    /// Blended mesh items (indices into the caller's list) in drawing order.
    pub(crate) mesh_order: Vec<usize>,
    /// Textured vertices in drawing order, for the textured pass's buffer.
    pub(crate) textured_vertices: Vec<TexturedVertex>,
}

/// One sortable transparent element.
enum Element<'a> {
    Mesh(usize),
    Colored(&'a [ColorVertex; 3]),
    Textured { texture: u64, triangle: &'a [TexturedVertex; 3] },
}

/// Sort every transparent element back to front for a camera at `eye` looking along unit `forward`, and lay out the
/// vertex streams the steps draw from.
///
/// `mesh_depths` holds each blended mesh item's view depth, in scene order. Colored vertices are appended to `transient`
/// (after whatever it already holds); incomplete trailing triangles of every list are dropped.
pub(crate) fn build_transparent_queue(
    mesh_depths: &[f32],
    triangles: &[ColorVertex],
    draws: &[TransparentDraw<'_>],
    eye: Vec3,
    forward: Vec3,
    transient: &mut Vec<MeshVertex>,
) -> TransparentQueue {
    let depth = |positions: [[f32; 3]; 3]| {
        let centroid = positions.iter().map(|position| Vec3::from_array(*position)).sum::<Vec3>() / 3.0;
        (centroid - eye).dot(forward)
    };

    // Gather every element with its view depth, in the order supplied.
    let mut elements: Vec<(f32, Element<'_>)> =
        mesh_depths.iter().enumerate().map(|(index, depth)| (*depth, Element::Mesh(index))).collect();
    for triangle in triangles.as_chunks::<3>().0 {
        elements.push((depth(triangle.each_ref().map(|vertex| vertex.position)), Element::Colored(triangle)));
    }
    for draw in draws {
        match draw {
            TransparentDraw::Colored(vertices) => {
                for triangle in vertices.as_chunks::<3>().0 {
                    let key = depth(triangle.each_ref().map(|vertex| vertex.position));
                    elements.push((key, Element::Colored(triangle)));
                }
            }
            TransparentDraw::Textured { texture, vertices } => {
                for triangle in vertices.as_chunks::<3>().0 {
                    let key = depth(triangle.each_ref().map(|vertex| vertex.position));
                    elements.push((key, Element::Textured { texture: texture.id(), triangle }));
                }
            }
        }
    }

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
            Element::Textured { texture, triangle } => {
                let start = queue.textured_vertices.len() as u32;
                queue.textured_vertices.extend_from_slice(triangle);
                let end = queue.textured_vertices.len() as u32;
                match queue.steps.last_mut() {
                    Some(TransparentStep::Textured { texture: current, vertices })
                        if *current == texture && vertices.end == start =>
                    {
                        vertices.end = end;
                    }
                    _ => queue.steps.push(TransparentStep::Textured { texture, vertices: start..end }),
                }
            }
        }
    }
    queue
}

#[cfg(test)]
#[path = "transparency.test.rs"]
mod tests;
