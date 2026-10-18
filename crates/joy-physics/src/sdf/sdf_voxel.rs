// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Voxel signed distance fields baked from triangle meshes (Sec. 3.6 of Giles and Andrews 2026).
//!
//! A regular grid around the mesh stores the signed distance and its gradient at every node; queries interpolate them
//! trilinearly and extrapolate outside the grid by adding the distance to it. Distances come from exact closest points
//! on the triangles. The sign comes from the winding number along grid lines in X, Y, and Z with a majority vote, so
//! procedural meshes with small holes, open seams, or overlapping parts still classify their interior.
//!
//! Positions and distances are meters in the mesh's own frame, which becomes the shape's local frame.

use rapier3d::math::{
    Real,
    Vector,
};
use rapier3d::parry::bounding_volume::Aabb;

use super::sdf_primitives::FieldSample;
use joy_core::{
    RandomSource,
    SeededRandom,
};

/// Empty grid cells around the mesh bounds on every side, so near-surface queries stay inside the grid.
const GRID_PADDING_CELLS: usize = 3;

/// Offsets of the classification lines from the grid nodes, as fractions of a cell. Irrational-looking offsets keep the
/// lines off shared vertices and edges, where a crossing could be counted twice or missed.
const LINE_JITTER: [Real; 2] = [0.123_456_7, 0.345_678_9];

/// Triangles whose projected area is below this fraction of the squared mesh size never cross a grid line.
const DEGENERATE_AREA_FRACTION: Real = 1.0e-12;

/// Three axis directions vote on the sign; a node is inside when at least this many agree.
const INSIDE_VOTES: u8 = 2;

/// Triangles per bounding volume hierarchy leaf.
const BVH_LEAF_TRIANGLES: usize = 4;

/// Validated triangle mesh with outward-facing winding, used while baking and for surface sampling.
pub(crate) struct TriangleMesh {
    positions: Vec<Vector>,
    triangles: Vec<[usize; 3]>,
    bounds: Aabb,
}

/// A baked distance grid: node `(x, y, z)` sits at `origin + (x, y, z) * cell`.
pub(crate) struct VoxelField {
    origin: Vector,
    cell: Real,
    counts: [usize; 3],
    // Per-node signed distances and gradients in X-major order. Distances are stored apart because contact search
    // evaluates them far more often than gradients, and a compact array keeps those lookups in cache.
    distances: Vec<Real>,
    gradients: Vec<Vector>,
}

/// Where a query falls in the grid: the clamped point, its cell's first node, and its fraction across the cell.
struct GridLookup {
    clamped: Vector,
    base: usize,
    fraction: Vector,
}

impl TriangleMesh {
    /// Validate positions and triangle indices and orient the triangles outward (positive enclosed volume).
    ///
    /// Fails on non-finite positions, an index count that is not a positive multiple of three, out-of-range indices,
    /// or a mesh without spatial extent. Degenerate triangles are kept; they contribute nothing to the bake.
    pub(crate) fn new(positions: &[Vector], indices: &[u32]) -> Result<Self, &'static str> {
        if positions.iter().any(|position| !position.is_finite()) {
            return Err("mesh positions must be finite");
        }
        if indices.is_empty() || !indices.len().is_multiple_of(3) {
            return Err("mesh indices must describe at least one triangle");
        }
        if indices.iter().any(|index| *index as usize >= positions.len()) {
            return Err("mesh indices must reference existing positions");
        }

        let (triangle_indices, _) = indices.as_chunks::<3>();
        let mut triangles: Vec<[usize; 3]> = triangle_indices
            .iter()
            .map(|triangle| [triangle[0] as usize, triangle[1] as usize, triangle[2] as usize])
            .collect();
        let bounds = Aabb::from_points(triangles.iter().flatten().map(|index| positions[*index]));
        if bounds.extents().max_element() <= 0.0 {
            return Err("mesh must have spatial extent");
        }

        // Winding decides which side is inside, so triangles must face outward; flip them all when the enclosed signed
        // volume is negative.
        let signed_volume: Real =
            triangles.iter().map(|[a, b, c]| positions[*a].dot(positions[*b].cross(positions[*c])) / 6.0).sum();
        if signed_volume < 0.0 {
            for triangle in &mut triangles {
                triangle.swap(1, 2);
            }
        }
        Ok(Self { positions: positions.to_vec(), triangles, bounds })
    }

    /// Tight local bounds of the referenced vertices.
    pub(crate) fn bounds(&self) -> Aabb {
        self.bounds
    }

    /// Area-weighted points on the triangles, drawn from `random`.
    pub(crate) fn surface_samples(&self, count: usize, random: &mut SeededRandom) -> Vec<Vector> {
        let mut cumulative_area = Vec::with_capacity(self.triangles.len());
        let mut total_area = 0.0;
        for triangle in &self.triangles {
            let [a, b, c] = self.vertices(*triangle);
            total_area += 0.5 * (b - a).cross(c - a).length();
            cumulative_area.push(total_area);
        }
        if total_area <= 0.0 {
            return Vec::new();
        }

        let mut samples = Vec::with_capacity(count);
        for _ in 0..count {
            let target = unit(random) * total_area;
            let index = cumulative_area.partition_point(|area| *area < target).min(self.triangles.len() - 1);
            let [a, b, c] = self.vertices(self.triangles[index]);

            // Uniform barycentric sampling: the square root keeps the density even across the triangle.
            let first = unit(random).sqrt();
            let second = unit(random);
            samples.push((a * (1.0 - first)) + (b * (first * (1.0 - second))) + (c * (first * second)));
        }
        samples
    }

    fn vertices(&self, triangle: [usize; 3]) -> [Vector; 3] {
        triangle.map(|index| self.positions[index])
    }
}

impl VoxelField {
    /// Bake `mesh` into a grid with `resolution` cells along its longest side, plus padding.
    pub(crate) fn bake(mesh: &TriangleMesh, resolution: u32) -> Self {
        let bounds = mesh.bounds();
        let extents = bounds.extents();
        let cell = extents.max_element() / resolution as Real;
        let origin = bounds.mins - (Vector::ONE * (GRID_PADDING_CELLS as Real * cell));
        let counts = [0, 1, 2].map(|axis| (extents[axis] / cell).ceil() as usize + 1 + (2 * GRID_PADDING_CELLS));
        let mut field = Self { origin, cell, counts, distances: Vec::new(), gradients: Vec::new() };

        let votes = field.classify_inside(mesh);
        (field.distances, field.gradients) = field.closest_point_samples(mesh, &votes);
        field
    }

    /// Signed distance at a local point: trilinear inside the grid, plus the distance to the grid outside it.
    pub(crate) fn distance(&self, point: Vector) -> Real {
        let lookup = self.lookup(point);
        self.interpolate(&lookup, |index| self.distances[index]) + (point - lookup.clamped).length()
    }

    /// Signed distance and gradient at a local point. Beyond the grid the gradient leans toward the query point.
    pub(crate) fn sample(&self, point: Vector) -> FieldSample {
        let lookup = self.lookup(point);
        let distance = self.interpolate(&lookup, |index| self.distances[index]);
        let mut gradient = self.interpolate(&lookup, |index| self.gradients[index]);
        let outside = point - lookup.clamped;
        let outside_length = outside.length();
        if outside_length > 0.0 {
            gradient = (gradient + (outside / outside_length)).normalize_or_zero();
        }
        FieldSample { distance: distance + outside_length, gradient }
    }

    /// Bytes held by the grid.
    pub(crate) fn memory_bytes(&self) -> usize {
        (self.distances.len() * std::mem::size_of::<Real>()) + (self.gradients.len() * std::mem::size_of::<Vector>())
    }

    /// Locate a point's cell, clamping it into the grid first.
    fn lookup(&self, point: Vector) -> GridLookup {
        let last = self.counts.map(|count| (count - 1) as Real);
        let highest = self.origin + (Vector::new(last[0], last[1], last[2]) * self.cell);
        let clamped = point.clamp(self.origin, highest);
        let grid = (clamped - self.origin) / self.cell;
        let cell = [0, 1, 2].map(|axis| (grid[axis] as usize).min(self.counts[axis] - 2));
        let fraction = grid - Vector::new(cell[0] as Real, cell[1] as Real, cell[2] as Real);
        GridLookup { clamped, base: self.index(cell), fraction }
    }

    /// Trilinear interpolation of per-node values around a lookup, as nested lerps along X, then Y, then Z.
    fn interpolate<T>(&self, lookup: &GridLookup, value: impl Fn(usize) -> T) -> T
    where
        T: Copy + std::ops::Add<Output = T> + std::ops::Sub<Output = T> + std::ops::Mul<Real, Output = T>,
    {
        let lerp = |from: T, to: T, amount: Real| from + ((to - from) * amount);
        let row = self.counts[0];
        let slice = self.counts[0] * self.counts[1];
        let Vector { x, y, z } = lookup.fraction;
        let at = |offset: usize| value(lookup.base + offset);
        let near = lerp(lerp(at(0), at(1), x), lerp(at(row), at(row + 1), x), y);
        let far = lerp(lerp(at(slice), at(slice + 1), x), lerp(at(slice + row), at(slice + row + 1), x), y);
        lerp(near, far, z)
    }

    /// Count, for every node, how many of the three axis directions place it inside the mesh by winding number.
    fn classify_inside(&self, mesh: &TriangleMesh) -> Vec<u8> {
        let mut votes = vec![0_u8; self.node_count()];
        let mesh_size = mesh.bounds().extents().max_element();
        let degenerate_area = DEGENERATE_AREA_FRACTION * (mesh_size * mesh_size);
        for axis in 0..3 {
            let across = [(axis + 1) % 3, (axis + 2) % 3];
            let jitter = LINE_JITTER.map(|fraction| fraction * self.cell);
            let line_counts = [self.counts[across[0]], self.counts[across[1]]];
            let mut crossings: Vec<Vec<(Real, i32)>> = vec![Vec::new(); line_counts[0] * line_counts[1]];

            // Record where every grid line along `axis` passes through a triangle, and in which direction.
            for triangle in &mesh.triangles {
                let [v0, v1, v2] = mesh.vertices(*triangle);
                let [b, c] = across;
                let determinant = ((v1[b] - v0[b]) * (v2[c] - v0[c])) - ((v2[b] - v0[b]) * (v1[c] - v0[c]));
                if determinant.abs() < degenerate_area {
                    continue;
                }
                let line_range = |dimension: usize, slot: usize| {
                    let low = v0[dimension].min(v1[dimension]).min(v2[dimension]);
                    let high = v0[dimension].max(v1[dimension]).max(v2[dimension]);
                    let first = ((low - self.origin[dimension] - jitter[slot]) / self.cell).ceil().max(0.0) as usize;
                    let last = ((high - self.origin[dimension] - jitter[slot]) / self.cell).floor();
                    (first, if last < 0.0 { None } else { Some((last as usize).min(line_counts[slot] - 1)) })
                };
                let (first_b, Some(last_b)) = line_range(b, 0) else {
                    continue;
                };
                let (first_c, Some(last_c)) = line_range(c, 1) else {
                    continue;
                };
                for k in first_c..=last_c {
                    for j in first_b..=last_b {
                        let along_b = self.origin[b] + (j as Real * self.cell) + jitter[0] - v0[b];
                        let along_c = self.origin[c] + (k as Real * self.cell) + jitter[1] - v0[c];
                        let w1 = ((along_b * (v2[c] - v0[c])) - ((v2[b] - v0[b]) * along_c)) / determinant;
                        let w2 = (((v1[b] - v0[b]) * along_c) - (along_b * (v1[c] - v0[c]))) / determinant;
                        if w1 < 0.0 || w2 < 0.0 || (w1 + w2) > 1.0 {
                            continue;
                        }
                        // The determinant's sign is the normal's sign along the line, so negative values enter.
                        let position = v0[axis] + (w1 * (v1[axis] - v0[axis])) + (w2 * (v2[axis] - v0[axis]));
                        let direction = if determinant < 0.0 { 1 } else { -1 };
                        crossings[j + (k * line_counts[0])].push((position, direction));
                    }
                }
            }

            // Sweep each line in order, accumulating the winding number at every node it passes.
            for k in 0..line_counts[1] {
                for j in 0..line_counts[0] {
                    let line = &mut crossings[j + (k * line_counts[0])];
                    line.sort_by(|left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)));
                    let mut passed = 0;
                    let mut winding = 0;
                    for i in 0..self.counts[axis] {
                        let position = self.origin[axis] + (i as Real * self.cell);
                        while passed < line.len() && line[passed].0 < position {
                            winding += line[passed].1;
                            passed += 1;
                        }
                        if winding > 0 {
                            let mut node = [0; 3];
                            node[axis] = i;
                            node[across[0]] = j;
                            node[across[1]] = k;
                            votes[self.index(node)] += 1;
                        }
                    }
                }
            }
        }
        votes
    }

    /// Exact closest-point distances and gradients for every node, signed by the inside votes.
    fn closest_point_samples(&self, mesh: &TriangleMesh, votes: &[u8]) -> (Vec<Real>, Vec<Vector>) {
        let hierarchy = TriangleHierarchy::new(mesh);
        let mut distances = vec![0.0; self.node_count()];
        let mut gradients = vec![Vector::ZERO; self.node_count()];
        let direction_epsilon = self.cell * 1.0e-4;
        for z in 0..self.counts[2] {
            for y in 0..self.counts[1] {
                // The previous node's closest triangle usually stays closest and bounds the next search.
                let mut previous_triangle: Option<usize> = None;
                for x in 0..self.counts[0] {
                    let point = self.origin + (Vector::new(x as Real, y as Real, z as Real) * self.cell);
                    let mut best = ClosestPoint { distance_squared: Real::INFINITY, point, triangle: 0 };
                    if let Some(triangle) = previous_triangle {
                        let [a, b, c] = mesh.vertices(mesh.triangles[triangle]);
                        let closest = closest_point_on_triangle(point, a, b, c);
                        // Slightly loosened so the same triangle is found again by the full search.
                        let distance_squared = ((closest - point).length_squared() * 1.0001) + 1.0e-12;
                        best = ClosestPoint { distance_squared, point: closest, triangle };
                    }
                    hierarchy.closest(mesh, point, &mut best);
                    previous_triangle = Some(best.triangle);

                    let index = self.index([x, y, z]);
                    let inside = votes[index] >= INSIDE_VOTES;
                    let distance = (point - best.point).length();
                    let gradient = if distance > direction_epsilon {
                        let away_from_surface = (point - best.point) / distance;
                        if inside { -away_from_surface } else { away_from_surface }
                    } else {
                        let [a, b, c] = mesh.vertices(mesh.triangles[best.triangle]);
                        (b - a).cross(c - a).normalize_or_zero()
                    };
                    distances[index] = if inside { -distance } else { distance };
                    gradients[index] = gradient;
                }
            }
        }
        (distances, gradients)
    }

    fn node_count(&self) -> usize {
        self.counts[0] * self.counts[1] * self.counts[2]
    }

    fn index(&self, node: [usize; 3]) -> usize {
        node[0] + (self.counts[0] * (node[1] + (self.counts[1] * node[2])))
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Closest points
// ---------------------------------------------------------------------------------------------------------------------

/// Best closest point found so far; searches only accept points nearer than `distance_squared`.
struct ClosestPoint {
    distance_squared: Real,
    point: Vector,
    triangle: usize,
}

/// One bounding volume hierarchy node. Leaves own `count` triangles starting at `first` in the ordering.
struct HierarchyNode {
    bounds: Aabb,
    children: Option<[usize; 2]>,
    first: usize,
    count: usize,
}

/// Bounding volume hierarchy over a mesh's triangles, split at centroid medians, for closest-point queries.
struct TriangleHierarchy {
    nodes: Vec<HierarchyNode>,
    order: Vec<usize>,
}

impl TriangleHierarchy {
    fn new(mesh: &TriangleMesh) -> Self {
        let centroids: Vec<Vector> = mesh
            .triangles
            .iter()
            .map(|triangle| {
                let [a, b, c] = mesh.vertices(*triangle);
                (a + b + c) / 3.0
            })
            .collect();
        let mut hierarchy = Self { nodes: Vec::new(), order: (0..mesh.triangles.len()).collect() };
        hierarchy.build(mesh, &centroids, 0, mesh.triangles.len());
        hierarchy
    }

    fn build(&mut self, mesh: &TriangleMesh, centroids: &[Vector], first: usize, count: usize) -> usize {
        let range = first..(first + count);
        let bounds = Aabb::from_points(
            self.order[range.clone()].iter().flat_map(|triangle| mesh.vertices(mesh.triangles[*triangle])),
        );
        let index = self.nodes.len();
        self.nodes.push(HierarchyNode { bounds, children: None, first, count });
        if count <= BVH_LEAF_TRIANGLES {
            return index;
        }

        // Median split along the longest axis of the centroids.
        let centers = Aabb::from_points(self.order[range.clone()].iter().map(|triangle| centroids[*triangle]));
        let extents = centers.extents();
        let axis = if extents.x > extents.y {
            if extents.x > extents.z { 0 } else { 2 }
        } else if extents.y > extents.z {
            1
        } else {
            2
        };
        let half = count / 2;
        self.order[range].select_nth_unstable_by(half, |left, right| {
            centroids[*left][axis].total_cmp(&centroids[*right][axis]).then(left.cmp(right))
        });
        let left = self.build(mesh, centroids, first, half);
        let right = self.build(mesh, centroids, first + half, count - half);
        self.nodes[index].children = Some([left, right]);
        index
    }

    /// Improve `best` with any triangle closer to `point`.
    fn closest(&self, mesh: &TriangleMesh, point: Vector, best: &mut ClosestPoint) {
        let mut stack = vec![0];
        while let Some(node_index) = stack.pop() {
            let node = &self.nodes[node_index];
            if distance_squared_to_box(&node.bounds, point) >= best.distance_squared {
                continue;
            }
            let Some([left, right]) = node.children else {
                for triangle in &self.order[node.first..(node.first + node.count)] {
                    let [a, b, c] = mesh.vertices(mesh.triangles[*triangle]);
                    let closest = closest_point_on_triangle(point, a, b, c);
                    let distance_squared = (closest - point).length_squared();
                    if distance_squared < best.distance_squared {
                        *best = ClosestPoint { distance_squared, point: closest, triangle: *triangle };
                    }
                }
                continue;
            };

            // Visit the nearer child first so it tightens the bound for the farther one.
            let left_first = distance_squared_to_box(&self.nodes[left].bounds, point)
                < distance_squared_to_box(&self.nodes[right].bounds, point);
            if left_first {
                stack.extend([right, left]);
            } else {
                stack.extend([left, right]);
            }
        }
    }
}

/// Closest point on triangle `abc` to `point` (Ericson, *Real-Time Collision Detection*, 5.1.5).
fn closest_point_on_triangle(point: Vector, a: Vector, b: Vector, c: Vector) -> Vector {
    let ab = b - a;
    let ac = c - a;
    let ap = point - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }

    let bp = point - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }

    let vc = (d1 * d4) - (d3 * d2);
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + (ab * (d1 / (d1 - d3)));
    }

    let cp = point - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }

    let vb = (d5 * d2) - (d1 * d6);
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + (ac * (d2 / (d2 - d6)));
    }

    let va = (d3 * d6) - (d5 * d4);
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + ((c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }

    let denominator = 1.0 / (va + vb + vc);
    a + (ab * (vb * denominator)) + (ac * (vc * denominator))
}

fn distance_squared_to_box(bounds: &Aabb, point: Vector) -> Real {
    (point - point.clamp(bounds.mins, bounds.maxs)).length_squared()
}

/// Next value of a seeded stream as a solver scalar in `[0, 1)`.
pub(crate) fn unit(random: &mut SeededRandom) -> Real {
    random.next_unit() as Real
}
