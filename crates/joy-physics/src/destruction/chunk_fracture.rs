// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Voronoi fracture: split a solid into irregular convex pieces that fill it exactly, so destructible geometry breaks
//! into the shards players expect instead of a grid of boxes.
//!
//! Each piece is the region closer to its site than to any other, clipped to the solid: the solid's polyhedron cut by
//! the bisector plane between its site and every other site. Neighboring pieces therefore share exact faces, which the
//! support graph reads as bonds. Sites come from the caller (authored, or [`fracture_sites`] for a seeded spread), so
//! fracture is deterministic and identical on every target.

use joy_core::RandomSource;
use joy_core::math::DVec3;

use super::chunk_shape::{
    ChunkPolyhedron,
    WELD_DISTANCE,
    newell_normal,
};
use super::{
    DestructibleChunkDefinition,
    DestructionError,
};

/// Separator between a fractured chunk's id and its piece index (`wall-3/7`).
pub const FRACTURE_ID_SEPARATOR: &str = "/";

/// Pieces thinner than this along every axis, in meters, are discarded as slivers.
const MINIMUM_PIECE_SIZE: f64 = 1.0e-4;

/// `count` sites drawn from `random` (a seeded stream for repeatable fracture) and spread through the box at `center` with full dimensions `size`: one jittered site per cell of
/// the coarsest grid holding `count` cells, kept in grid order, so pieces are irregular but roughly even in size.
pub fn fracture_sites(center: DVec3, size: DVec3, count: usize, random: &mut impl RandomSource) -> Vec<DVec3> {
    if count == 0 {
        return Vec::new();
    }

    // Split the longest axes first so pieces stay roughly as wide as they are tall.
    let mut cells = [1usize; 3];
    while cells.iter().product::<usize>() < count {
        let axis = (0..3)
            .max_by(|left, right| (size[*left] / cells[*left] as f64).total_cmp(&(size[*right] / cells[*right] as f64)))
            .unwrap_or(0);
        cells[axis] += 1;
    }

    let minimum = center - (size / 2.0);
    let mut sites = Vec::with_capacity(count);
    'grid: for z in 0..cells[2] {
        for y in 0..cells[1] {
            for x in 0..cells[0] {
                if sites.len() == count {
                    break 'grid;
                }
                let cell = DVec3::new(x as f64, y as f64, z as f64);
                let jitter = DVec3::new(random.next_unit(), random.next_unit(), random.next_unit());
                let fraction = (cell + jitter) / DVec3::new(cells[0] as f64, cells[1] as f64, cells[2] as f64);
                sites.push(minimum + (fraction * size));
            }
        }
    }
    sites
}

/// Split `solid` into one convex piece per site, in site order; sites whose cell is empty or a sliver produce nothing.
pub fn voronoi_fracture(solid: &ChunkPolyhedron, sites: &[DVec3]) -> Vec<ChunkPolyhedron> {
    let mut pieces = Vec::with_capacity(sites.len());
    for (index, site) in sites.iter().enumerate() {
        let mut cell = Some(FaceLoops::from_polyhedron(solid));
        for (other_index, other) in sites.iter().enumerate() {
            if other_index == index || other == site {
                continue;
            }
            // Keep the half-space nearer this site: normal · x <= offset on the bisector plane.
            let normal = (*other - *site).normalize();
            let offset = normal.dot((*site + *other) / 2.0);
            cell = cell.and_then(|loops| loops.clip(normal, offset));
        }
        if let Some(piece) = cell.and_then(FaceLoops::into_polyhedron) {
            pieces.push(piece);
        }
    }
    pieces
}

/// Fracture one chunk into pieces at `sites`: each piece keeps the chunk's material and hit points, takes its bounding
/// box as `center` and `size`, carries its exact shape, and is named `<chunk id>/<piece index>`.
pub fn fracture_chunk(
    chunk: &DestructibleChunkDefinition,
    sites: &[DVec3],
) -> Result<Vec<DestructibleChunkDefinition>, DestructionError> {
    let solid = chunk.solid();
    solid.validate()?;
    let pieces = voronoi_fracture(&solid, sites);
    Ok(pieces
        .into_iter()
        .enumerate()
        .map(|(index, shape)| {
            let (minimum, maximum) = shape.bounds();
            DestructibleChunkDefinition {
                id: format!("{}{FRACTURE_ID_SEPARATOR}{index}", chunk.id),
                center: (minimum + maximum) / 2.0,
                size: maximum - minimum,
                material: chunk.material.clone(),
                hit_points: chunk.hit_points,
                shape: Some(shape),
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------------------------------
// Clipping
// ---------------------------------------------------------------------------------------------------------------------

/// A convex polyhedron as loose face loops (points, counterclockwise from outside) while it is being clipped.
#[derive(Clone, Debug)]
struct FaceLoops {
    faces: Vec<Vec<DVec3>>,
}

impl FaceLoops {
    fn from_polyhedron(solid: &ChunkPolyhedron) -> Self {
        let faces = solid
            .faces
            .iter()
            .map(|face| face.iter().filter_map(|index| solid.vertices.get(*index as usize).copied()).collect())
            .collect();
        Self {
            faces,
        }
    }

    /// Keep the part where `normal · x <= offset`, closing the cut with a cap face; `None` when nothing remains.
    fn clip(self, normal: DVec3, offset: f64) -> Option<Self> {
        let side = |point: DVec3| normal.dot(point) - offset;
        let mut faces = Vec::with_capacity(self.faces.len() + 1);
        let mut cut = Vec::new();
        for face in self.faces {
            let mut kept = Vec::with_capacity(face.len() + 1);
            for (index, current) in face.iter().enumerate() {
                let next = face[(index + 1) % face.len()];
                let (current_side, next_side) = (side(*current), side(next));
                if current_side <= 0.0 {
                    kept.push(*current);
                }
                // An edge that crosses the plane contributes its crossing point to this face and to the cap.
                if (current_side < 0.0 && next_side > 0.0) || (current_side > 0.0 && next_side < 0.0) {
                    let crossing = *current + ((next - *current) * (current_side / (current_side - next_side)));
                    kept.push(crossing);
                    cut.push(crossing);
                } else if current_side == 0.0 {
                    cut.push(*current);
                }
            }
            if kept.len() >= 3 {
                faces.push(kept);
            }
        }
        if faces.len() < 3 {
            return None;
        }
        if let Some(cap) = cap_face(&cut, normal) {
            faces.push(cap);
        }
        Some(Self {
            faces,
        })
    }

    /// Weld shared points into indexed vertices and drop degenerate faces; `None` for a sliver or an empty piece.
    fn into_polyhedron(self) -> Option<ChunkPolyhedron> {
        let mut vertices: Vec<DVec3> = Vec::new();
        let mut faces = Vec::with_capacity(self.faces.len());
        for face in self.faces {
            let mut indices: Vec<u32> = Vec::with_capacity(face.len());
            for point in face {
                let index = match vertices.iter().position(|vertex| vertex.distance(point) <= WELD_DISTANCE) {
                    Some(index) => index,
                    None => {
                        vertices.push(point);
                        vertices.len() - 1
                    }
                } as u32;
                if indices.last() != Some(&index) && indices.first() != Some(&index) {
                    indices.push(index);
                }
            }
            if indices.len() >= 3 {
                faces.push(indices);
            }
        }
        let piece = ChunkPolyhedron {
            vertices,
            faces,
        };
        let (minimum, maximum) = piece.bounds();
        if (maximum - minimum).max_element() < MINIMUM_PIECE_SIZE || piece.validate().is_err() {
            return None;
        }
        Some(piece)
    }
}

/// The face closing a cut: the crossing points ordered counterclockwise seen from outside (along `normal`), with
/// duplicates welded. `None` when fewer than three distinct points remain.
fn cap_face(points: &[DVec3], normal: DVec3) -> Option<Vec<DVec3>> {
    let mut unique: Vec<DVec3> = Vec::with_capacity(points.len());
    for point in points {
        if !unique.iter().any(|kept| kept.distance(*point) <= WELD_DISTANCE) {
            unique.push(*point);
        }
    }
    if unique.len() < 3 {
        return None;
    }
    let centroid = unique.iter().copied().sum::<DVec3>() / unique.len() as f64;
    let helper = if normal.y.abs() < 0.9 { DVec3::Y } else { DVec3::X };
    let tangent = helper.cross(normal).normalize();
    let bitangent = normal.cross(tangent);

    // Order by angle about the centroid without trigonometry: half-plane first, then the cross-product sign.
    let mut ordered = unique;
    ordered.sort_by(|left, right| {
        let a = [(*left - centroid).dot(tangent), (*left - centroid).dot(bitangent)];
        let b = [(*right - centroid).dot(tangent), (*right - centroid).dot(bitangent)];
        let half = |point: [f64; 2]| if point[1] > 0.0 || (point[1] == 0.0 && point[0] > 0.0) { 0 } else { 1 };
        half(a).cmp(&half(b)).then_with(|| {
            let cross = (a[0] * b[1]) - (a[1] * b[0]);
            0.0f64.total_cmp(&cross)
        })
    });
    if newell_normal(&ordered).dot(normal) < 0.0 {
        ordered.reverse();
    }
    Some(ordered)
}
