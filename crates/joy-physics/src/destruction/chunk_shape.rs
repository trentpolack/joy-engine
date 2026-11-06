// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Convex chunk shapes: the solid a destructible piece actually occupies, for pieces that are not boxes.
//!
//! A [`ChunkPolyhedron`] is a convex polyhedron in world space (meters, Y up): vertices plus faces listed as vertex
//! index loops, counterclockwise seen from outside. Fracture ([`super::voronoi_fracture`]) produces them, and authored
//! pieces may supply their own. The structure reads them for damage, raycasts, occupancy, support bonds, stress mass,
//! colliders, and meshes; a chunk without one is the axis-aligned box its `center` and `size` describe.
//!
//! Every operation is `f64` arithmetic with no transcendental functions, so results are identical natively and in
//! WebAssembly.

use joy_core::math::DVec3;
use serde::{
    Deserialize,
    Serialize,
};

use super::DestructionError;

/// Distance within which a vertex may sit outside a face plane and still count as convex, in world units.
const CONVEXITY_TOLERANCE: f64 = 1.0e-6;

/// Two faces bond only when their normals are this close to opposite (the cosine of about 2.5 degrees).
const OPPOSED_FACE_COSINE: f64 = 0.999;

/// Points closer than this are one vertex when faces are rebuilt, in world units.
pub(crate) const WELD_DISTANCE: f64 = 1.0e-9;

/// The eight corners of an axis-aligned box, indexed by `x + 2y + 4z` bits (0 = minimum, 1 = maximum).
const BOX_FACES: [[u32; 4]; 6] = [
    [0, 4, 6, 2], // -X
    [1, 3, 7, 5], // +X
    [0, 1, 5, 4], // -Y
    [2, 6, 7, 3], // +Y
    [0, 2, 3, 1], // -Z
    [4, 5, 7, 6], // +Z
];

/// A convex polyhedron in world space: vertices plus counterclockwise (from outside) face loops.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkPolyhedron {
    /// World-space vertices in meters.
    pub vertices: Vec<DVec3>,
    /// Faces as loops of indices into `vertices`, counterclockwise seen from outside; at least three each.
    pub faces: Vec<Vec<u32>>,
}

/// A face's outward unit normal and plane offset: points `x` on the face satisfy `normal · x = offset`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FacePlane {
    pub(crate) normal: DVec3,
    pub(crate) offset: f64,
}

/// The shared face between two touching pieces: the first piece's outward normal and the overlapping area.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SharedFace {
    pub(crate) normal: DVec3,
    pub(crate) area: f64,
}

impl ChunkPolyhedron {
    /// The axis-aligned box centered at `center` with full dimensions `size`.
    pub fn from_box(center: DVec3, size: DVec3) -> Self {
        let half = size / 2.0;
        let vertices = (0..8u32)
            .map(|corner| {
                let sign = |bit: u32| if (corner >> bit) & 1 == 1 { 1.0 } else { -1.0 };
                center + DVec3::new(sign(0) * half.x, sign(1) * half.y, sign(2) * half.z)
            })
            .collect();
        Self {
            vertices,
            faces: BOX_FACES.iter().map(|face| face.to_vec()).collect(),
        }
    }

    /// Reject shapes that are not closed convex solids: fewer than four vertices or faces, faces with fewer than three
    /// vertices or out-of-range indices, non-finite coordinates, degenerate faces, vertices outside any face plane, or
    /// no volume.
    pub fn validate(&self) -> Result<(), DestructionError> {
        let invalid = || Err(DestructionError::InvalidChunkShape);
        if self.vertices.len() < 4 || self.faces.len() < 4 || !self.vertices.iter().all(|vertex| vertex.is_finite()) {
            return invalid();
        }
        let count = self.vertices.len() as u32;
        for face in &self.faces {
            if face.len() < 3 || face.iter().any(|index| *index >= count) {
                return invalid();
            }
        }
        for index in 0..self.faces.len() {
            let Some(plane) = self.face_plane(index) else {
                return invalid();
            };
            let outside =
                self.vertices.iter().any(|vertex| plane.normal.dot(*vertex) - plane.offset > CONVEXITY_TOLERANCE);
            if outside {
                return invalid();
            }
        }
        if self.volume() <= 0.0 {
            return invalid();
        }
        Ok(())
    }

    /// The smallest axis-aligned box containing every vertex, as (minimum corner, maximum corner).
    pub fn bounds(&self) -> (DVec3, DVec3) {
        let mut minimum = DVec3::splat(f64::INFINITY);
        let mut maximum = DVec3::splat(f64::NEG_INFINITY);
        for vertex in &self.vertices {
            minimum = minimum.min(*vertex);
            maximum = maximum.max(*vertex);
        }
        (minimum, maximum)
    }

    /// Enclosed volume in cubic meters (zero for an invalid shape).
    pub fn volume(&self) -> f64 {
        let reference = self.vertex_centroid();
        let mut volume = 0.0;
        for face in &self.faces {
            let Some(first) = face.first().map(|index| self.vertex(*index)) else {
                continue;
            };
            for pair in face[1..].windows(2) {
                let (second, third) = (self.vertex(pair[0]), self.vertex(pair[1]));
                volume += (first - reference).dot((second - reference).cross(third - reference)) / 6.0;
            }
        }
        volume.max(0.0)
    }

    /// The solid's center of volume (its center of mass at uniform density).
    pub fn centroid(&self) -> DVec3 {
        let reference = self.vertex_centroid();
        let mut weighted = DVec3::ZERO;
        let mut volume = 0.0;
        for face in &self.faces {
            let Some(first) = face.first().map(|index| self.vertex(*index)) else {
                continue;
            };
            for pair in face[1..].windows(2) {
                let (second, third) = (self.vertex(pair[0]), self.vertex(pair[1]));
                let tetrahedron = (first - reference).dot((second - reference).cross(third - reference)) / 6.0;
                weighted += (reference + first + second + third) * (tetrahedron / 4.0);
                volume += tetrahedron;
            }
        }
        if volume <= 0.0 {
            return reference;
        }
        weighted / volume
    }

    /// Horizontal area of the solid's underside: the downward-facing faces projected onto the ground plane, which for a
    /// convex solid is its footprint.
    pub fn footprint_area(&self) -> f64 {
        let mut area = 0.0;
        for face in &self.faces {
            let points: Vec<DVec3> = face.iter().map(|index| self.vertex(*index)).collect();
            let doubled = newell_normal(&points);
            if doubled.y < 0.0 {
                area -= doubled.y / 2.0;
            }
        }
        area
    }

    /// The same shape moved by `offset` meters.
    pub fn translated(&self, offset: DVec3) -> Self {
        Self {
            vertices: self.vertices.iter().map(|vertex| *vertex + offset).collect(),
            faces: self.faces.clone(),
        }
    }

    /// The same shape scaled by `factor` about `pivot`.
    pub fn scaled_about(&self, pivot: DVec3, factor: f64) -> Self {
        Self {
            vertices: self.vertices.iter().map(|vertex| pivot + ((*vertex - pivot) * factor)).collect(),
            faces: self.faces.clone(),
        }
    }

    /// Entry parameter of the ray `origin + direction * t` (`t >= 0`) into the solid, or `None` when it misses. A ray
    /// that starts inside enters at zero.
    pub fn ray_entry(&self, origin: DVec3, direction: DVec3) -> Option<f64> {
        let mut entry: f64 = 0.0;
        let mut exit = f64::INFINITY;
        for index in 0..self.faces.len() {
            let plane = self.face_plane(index)?;
            let distance = plane.offset - plane.normal.dot(origin);
            let rate = plane.normal.dot(direction);
            if rate.abs() < f64::EPSILON {
                if distance < 0.0 {
                    return None;
                }
                continue;
            }
            let crossing = distance / rate;
            if rate < 0.0 {
                entry = entry.max(crossing);
            } else {
                exit = exit.min(crossing);
            }
            if exit < entry {
                return None;
            }
        }
        Some(entry)
    }

    /// Whether a sphere reaches the solid. Uses the largest face-plane distance, which is exact over faces and slightly
    /// generous near edges and corners (it may report a sphere that only reaches an edge's extended planes).
    pub fn reaches_sphere(&self, center: DVec3, radius: f64) -> bool {
        self.plane_distance(center) <= radius
    }

    /// Whether an axis-aligned box overlaps the solid's interior, by separating planes on the box axes and the faces.
    /// Touching faces do not overlap.
    pub fn overlaps_box(&self, center: DVec3, half_extents: DVec3) -> bool {
        let (minimum, maximum) = self.bounds();
        let separated_on_box_axes = (0..3).any(|axis| {
            maximum[axis] <= center[axis] - half_extents[axis] || minimum[axis] >= center[axis] + half_extents[axis]
        });
        if separated_on_box_axes {
            return false;
        }
        for index in 0..self.faces.len() {
            let Some(plane) = self.face_plane(index) else {
                continue;
            };
            let reach = (plane.normal.abs()).dot(half_extents);
            if plane.normal.dot(center) - reach >= plane.offset {
                return false;
            }
        }
        true
    }

    /// The outward plane of face `index`, or `None` for a degenerate face.
    pub(crate) fn face_plane(&self, index: usize) -> Option<FacePlane> {
        let face = self.faces.get(index)?;
        let points: Vec<DVec3> = face.iter().map(|vertex| self.vertex(*vertex)).collect();
        let normal = newell_normal(&points);
        let length = normal.length();
        if length <= f64::EPSILON {
            return None;
        }
        let normal = normal / length;
        let centroid = points.iter().copied().sum::<DVec3>() / points.len() as f64;
        Some(FacePlane {
            normal,
            offset: normal.dot(centroid),
        })
    }

    /// The face of `other` this shape shares, if any: opposed coplanar faces (within `tolerance` meters) whose outlines
    /// overlap. Returns this shape's outward normal for the largest overlap and the total overlapping area.
    pub(crate) fn shared_face(&self, other: &Self, tolerance: f64) -> Option<SharedFace> {
        let mut best: Option<(DVec3, f64)> = None;
        let mut total_area = 0.0;
        for own in 0..self.faces.len() {
            let Some(own_plane) = self.face_plane(own) else {
                continue;
            };
            for theirs in 0..other.faces.len() {
                let Some(their_plane) = other.face_plane(theirs) else {
                    continue;
                };
                let opposed = own_plane.normal.dot(their_plane.normal) <= -OPPOSED_FACE_COSINE;
                let coplanar = (own_plane.offset + their_plane.offset).abs() <= tolerance;
                if !opposed || !coplanar {
                    continue;
                }
                let area = overlap_area(&self.face_points(own), &other.face_points(theirs), own_plane.normal);
                if area <= 0.0 {
                    continue;
                }
                total_area += area;
                if best.is_none_or(|(_, largest)| area > largest) {
                    best = Some((own_plane.normal, area));
                }
            }
        }
        best.map(|(normal, _)| SharedFace {
            normal,
            area: total_area,
        })
    }

    /// The largest signed distance from `point` to any face plane: negative inside, positive outside.
    fn plane_distance(&self, point: DVec3) -> f64 {
        (0..self.faces.len())
            .filter_map(|index| self.face_plane(index))
            .map(|plane| plane.normal.dot(point) - plane.offset)
            .fold(f64::NEG_INFINITY, f64::max)
    }

    fn vertex(&self, index: u32) -> DVec3 {
        self.vertices.get(index as usize).copied().unwrap_or(DVec3::ZERO)
    }

    fn face_points(&self, index: usize) -> Vec<DVec3> {
        self.faces.get(index).map(|face| face.iter().map(|vertex| self.vertex(*vertex)).collect()).unwrap_or_default()
    }

    fn vertex_centroid(&self) -> DVec3 {
        if self.vertices.is_empty() {
            return DVec3::ZERO;
        }
        self.vertices.iter().copied().sum::<DVec3>() / self.vertices.len() as f64
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Polygon helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Newell's method: the area-weighted normal of a planar loop (twice its area long), counterclockwise positive.
pub(crate) fn newell_normal(points: &[DVec3]) -> DVec3 {
    let mut normal = DVec3::ZERO;
    for (index, current) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        normal.x += (current.y - next.y) * (current.z + next.z);
        normal.y += (current.z - next.z) * (current.x + next.x);
        normal.z += (current.x - next.x) * (current.y + next.y);
    }
    normal
}

/// Area where two coplanar convex loops overlap. `own` winds counterclockwise about `normal`; `theirs` belongs to the
/// touching face, so it winds the other way and is reversed before clipping.
fn overlap_area(own: &[DVec3], theirs: &[DVec3], normal: DVec3) -> f64 {
    let (tangent, bitangent) = plane_basis(normal);
    let project = |point: &DVec3| [point.dot(tangent), point.dot(bitangent)];
    let subject: Vec<[f64; 2]> = own.iter().map(project).collect();
    let clip: Vec<[f64; 2]> = theirs.iter().rev().map(project).collect();
    polygon_area(&clip_convex(&subject, &clip))
}

/// Two unit vectors spanning the plane perpendicular to `normal`, forming a right-handed basis with it.
fn plane_basis(normal: DVec3) -> (DVec3, DVec3) {
    let helper = if normal.y.abs() < 0.9 { DVec3::Y } else { DVec3::X };
    let tangent = helper.cross(normal).normalize();
    (tangent, normal.cross(tangent))
}

/// Sutherland–Hodgman: `subject` clipped to the inside of the counterclockwise convex polygon `clip`.
fn clip_convex(subject: &[[f64; 2]], clip: &[[f64; 2]]) -> Vec<[f64; 2]> {
    let mut output = subject.to_vec();
    for (index, edge_start) in clip.iter().enumerate() {
        if output.is_empty() {
            break;
        }
        let edge_end = clip[(index + 1) % clip.len()];
        let side = |point: &[f64; 2]| {
            ((edge_end[0] - edge_start[0]) * (point[1] - edge_start[1]))
                - ((edge_end[1] - edge_start[1]) * (point[0] - edge_start[0]))
        };
        let input = std::mem::take(&mut output);
        for (point_index, current) in input.iter().enumerate() {
            let previous = input[(point_index + input.len() - 1) % input.len()];
            let (current_side, previous_side) = (side(current), side(&previous));
            if current_side >= 0.0 {
                if previous_side < 0.0 {
                    output.push(intersect(previous, *current, previous_side, current_side));
                }
                output.push(*current);
            } else if previous_side >= 0.0 {
                output.push(intersect(previous, *current, previous_side, current_side));
            }
        }
    }
    output
}

/// The point where a segment crosses a clip edge, from the two endpoints' signed sides.
fn intersect(start: [f64; 2], end: [f64; 2], start_side: f64, end_side: f64) -> [f64; 2] {
    let fraction = start_side / (start_side - end_side);
    [start[0] + ((end[0] - start[0]) * fraction), start[1] + ((end[1] - start[1]) * fraction)]
}

/// Shoelace area of a counterclockwise polygon (zero for fewer than three points).
fn polygon_area(points: &[[f64; 2]]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut twice_area = 0.0;
    for (index, current) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        twice_area += (current[0] * next[1]) - (next[0] * current[1]);
    }
    (twice_area / 2.0).max(0.0)
}
