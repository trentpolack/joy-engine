// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The craterable ground: a flat plane plus additive crater edits inside a fixed patch around the structure, and a
//! flat apron beyond it. Heights are an exact function of the crater list (the engine's `TerrainCrater` profile, the
//! same edit streamed terrain applies), so the structure's anchors, the heightfield collider, and the rendered mesh all
//! read one surface. Meters, Y up; craters are cleared only by reset.

use joy_engine::math::{
    DVec3,
    Vec3,
};
use joy_engine::physics::HeightfieldOptions;
use joy_engine::render_types::Mesh;
use joy_engine::terrain::TerrainCrater;

use super::config::destruction_config::{
    CHARGE,
    GROUND,
};

/// Bisection steps that refine a ground ray hit after marching finds the crossing.
const RAY_REFINEMENT_STEPS: u32 = 24;

/// Unit corner offsets of a quad in the chunk mesh's +Y face winding: (-X, -Z), (+X, -Z), (+X, +Z), (-X, +Z).
const QUAD_CORNERS: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// Owns the crater edit log and derives heights, collision grids, and mesh data from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CraterGround {
    craters: Vec<TerrainCrater>,
}

impl CraterGround {
    /// Flat ground with no craters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Craters in the order they were added.
    pub fn craters(&self) -> &[TerrainCrater] {
        &self.craters
    }

    /// Ground height under world (`x`, `z`): the flat ground plus every crater's offset (offsets add and commute).
    pub fn height_at(&self, x: f64, z: f64) -> f64 {
        GROUND.height + self.craters.iter().map(|crater| crater.offset_at(x, z)).sum::<f64>()
    }

    /// Blast a charge crater centered under world (`x`, `z`), moved inward so its whole reach stays inside the
    /// craterable patch, and return it.
    pub fn add_charge(&mut self, x: f64, z: f64) -> TerrainCrater {
        let mut crater = TerrainCrater {
            center: [x, z],
            radius: CHARGE.radius,
            depth: CHARGE.depth,
            rim_height: CHARGE.rim_height,
        };
        let reach = crater.reach();
        for (axis, half_extent) in GROUND.patch_half_extents.into_iter().enumerate() {
            let limit = (half_extent - reach).max(0.0);
            crater.center[axis] = crater.center[axis].clamp(-limit, limit);
        }
        self.craters.push(crater);
        crater
    }

    /// Remove every crater.
    pub fn clear(&mut self) {
        self.craters.clear();
    }

    /// Samples per axis of the patch grid: X columns, then Z rows.
    pub fn grid_size(&self) -> (usize, usize) {
        let [half_x, half_z] = GROUND.patch_half_extents;
        let samples = |half: f64| ((half * 2.0) / GROUND.sample_spacing).round() as usize + 1;
        (samples(half_x), samples(half_z))
    }

    /// The patch as a static heightfield: sample (0, 0) at its -X, -Z corner, rows running along +Z.
    pub fn heightfield(&self) -> HeightfieldOptions {
        let (columns, rows) = self.grid_size();
        let mut heights = Vec::with_capacity(columns * rows);
        for row in 0..rows {
            for column in 0..columns {
                let (x, z) = self.sample_position(column, row);
                heights.push(self.height_at(x, z) as f32);
            }
        }
        let [half_x, half_z] = GROUND.patch_half_extents;
        HeightfieldOptions {
            origin: Vec3::new(-half_x as f32, 0.0, -half_z as f32),
            spacing: [GROUND.sample_spacing as f32; 2],
            columns,
            rows,
            heights,
            friction: GROUND.friction,
            restitution: GROUND.restitution,
        }
    }

    /// Ground mesh data: the patch grid (split along the heightfield's (-X, -Z) to (+X, +Z) diagonal, so the drawn
    /// surface matches the collider) with smooth normals, ringed by four flat apron quads. Colors are left to the tint.
    pub fn mesh_data(&self) -> Mesh {
        let (columns, rows) = self.grid_size();
        let mut positions = Vec::with_capacity((columns * rows) + 16);
        let mut normals = Vec::with_capacity(positions.capacity());
        let mut indices = Vec::new();

        for row in 0..rows {
            for column in 0..columns {
                let (x, z) = self.sample_position(column, row);
                positions.push(DVec3::new(x, self.height_at(x, z), z));
                normals.push(self.normal_at(x, z));
            }
        }
        for row in 0..(rows - 1) {
            for column in 0..(columns - 1) {
                // Corners (-X, -Z), (+X, -Z), (-X, +Z), (+X, +Z); both triangles share the (-X, -Z)-(+X, +Z) diagonal.
                let corner = ((row * columns) + column) as u32;
                let east = corner + 1;
                let north = corner + columns as u32;
                let north_east = north + 1;
                indices.extend_from_slice(&[corner, east, north_east, corner, north_east, north]);
            }
        }

        // The apron: south and north strips span the full width; west and east strips fill beside the patch.
        let apron = GROUND.apron_half_extent;
        let [half_x, half_z] = GROUND.patch_half_extents;
        let strips = [
            ([-apron, -apron], [apron, -half_z]),
            ([-apron, half_z], [apron, apron]),
            ([-apron, -half_z], [-half_x, half_z]),
            ([half_x, -half_z], [apron, half_z]),
        ];
        for (minimum, maximum) in strips {
            let start = positions.len() as u32;
            for [x_fraction, z_fraction] in QUAD_CORNERS {
                let x = minimum[0] + ((maximum[0] - minimum[0]) * x_fraction);
                let z = minimum[1] + ((maximum[1] - minimum[1]) * z_fraction);
                positions.push(DVec3::new(x, GROUND.height, z));
                normals.push(Vec3::Y);
            }
            indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }

        Mesh {
            normals: Some(normals),
            ..Mesh::new(positions, indices)
        }
    }

    /// Where a world-space ray first meets the ground within `maximum_distance` (ray parameter units), or `None`.
    ///
    /// Marches at half the sample spacing, then bisects the crossing, so crater walls are found without a mesh query.
    pub fn raycast(&self, origin: DVec3, direction: DVec3, maximum_distance: f64) -> Option<DVec3> {
        let length = direction.length();
        if !origin.is_finite() || !direction.is_finite() || length <= 0.0 {
            return None;
        }
        let step = (GROUND.sample_spacing / 2.0) / length;
        let above = |distance: f64| {
            let point = origin + (direction * distance);
            point.y > self.height_at(point.x, point.z)
        };
        if !above(0.0) {
            return Some(origin);
        }

        let mut previous = 0.0;
        let mut distance = step;
        while previous < maximum_distance {
            let current = distance.min(maximum_distance);
            if !above(current) {
                let (mut low, mut high) = (previous, current);
                for _ in 0..RAY_REFINEMENT_STEPS {
                    let middle = (low + high) / 2.0;
                    if above(middle) {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                return Some(origin + (direction * high));
            }
            previous = current;
            distance += step;
        }
        None
    }

    /// World X and Z of a patch sample.
    fn sample_position(&self, column: usize, row: usize) -> (f64, f64) {
        let [half_x, half_z] = GROUND.patch_half_extents;
        let x = -half_x + (column as f64 * GROUND.sample_spacing);
        let z = -half_z + (row as f64 * GROUND.sample_spacing);
        (x, z)
    }

    /// Unit surface normal from central differences of the exact height function.
    fn normal_at(&self, x: f64, z: f64) -> Vec3 {
        let delta = GROUND.sample_spacing / 2.0;
        let slope_x = (self.height_at(x + delta, z) - self.height_at(x - delta, z)) / (delta * 2.0);
        let slope_z = (self.height_at(x, z + delta) - self.height_at(x, z - delta)) / (delta * 2.0);
        DVec3::new(-slope_x, 1.0, -slope_z).normalize().as_vec3()
    }
}
