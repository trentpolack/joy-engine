// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The props both bins drop: FORM sources evaluated once into a render mesh, a signed distance collider, and the
//! bounding box the comparison bin uses as its collider.
//!
//! Four props bake their FORM triangles into voxel distance grids; the ring uses the analytical torus field, and its
//! FORM mesh only draws it. Every prop is evaluated once at startup and shared by every body that uses it.

use joy_engine::form::{
    CompiledScript,
    GeometryData,
};
use joy_engine::physics::{
    SdfPrimitive,
    SdfShape,
};
use joy_engine::prelude::*;
use joy_engine::rendering::form_mesh_asset;
use joy_engine::runtime::Stopwatch;
use joy_engine::serde_json::{
    Map,
    Value,
};

use super::config::sdf_collisions_config::PHYSICS;

/// How a prop's collider is built.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColliderSource {
    /// Bake the FORM triangles into a voxel distance grid.
    BakedMesh,
    /// An analytical torus (meters); the FORM mesh only draws it.
    Torus { major_radius: f32, minor_radius: f32 },
}

/// One authored prop.
pub struct PropDefinition {
    /// Display name.
    pub name: &'static str,
    /// FORM source.
    pub source: &'static str,
    /// Collider construction.
    pub collider: ColliderSource,
}

/// The authored props, in spawn-table order.
pub const PROPS: [PropDefinition; 5] = [
    PropDefinition {
        name: "Chair",
        source: include_str!("../assets/chair.form"),
        collider: ColliderSource::BakedMesh,
    },
    PropDefinition {
        name: "Jack",
        source: include_str!("../assets/jack.form"),
        collider: ColliderSource::BakedMesh,
    },
    PropDefinition {
        name: "Staple",
        source: include_str!("../assets/staple.form"),
        collider: ColliderSource::BakedMesh,
    },
    PropDefinition {
        name: "I-beam",
        source: include_str!("../assets/beam.form"),
        collider: ColliderSource::BakedMesh,
    },
    PropDefinition {
        name: "Ring",
        source: include_str!("../assets/ring.form"),
        collider: ColliderSource::Torus {
            major_radius: 0.36,
            minor_radius: 0.1,
        },
    },
];

/// A prop ready to spawn: its render mesh, SDF collider, bounding-box collider, and mass.
pub struct Prop {
    /// Display name.
    pub name: &'static str,
    /// Render mesh in the prop's local frame (the body frame of the SDF bin).
    pub asset: MeshAsset,
    /// Signed distance collider in the same local frame.
    pub shape: SdfShape,
    /// Center and half extents of the local bounding box, used as the comparison bin's box collider.
    pub box_center: Vec3,
    pub box_half_extents: Vec3,
    /// Mass in kilograms: the field's enclosed volume times the configured density, used in both bins.
    pub mass: f32,
    /// Source triangles, CPU milliseconds spent building the collider, and the collider's field memory in bytes.
    pub triangles: usize,
    pub collider_milliseconds: f64,
    pub field_bytes: usize,
}

/// Every prop, evaluated and baked once; owners borrow it.
pub struct PropLibrary {
    props: Vec<Prop>,
}

impl PropLibrary {
    /// Evaluate and bake every prop. Fails if a bundled FORM source or its geometry is invalid.
    pub fn load() -> AppResult<Self> {
        let props = PROPS.iter().map(build_prop).collect::<AppResult<Vec<Prop>>>()?;
        Ok(Self {
            props,
        })
    }

    /// The props in spawn-table order.
    pub fn props(&self) -> &[Prop] {
        &self.props
    }

    /// The prop at a spawn-table index.
    pub fn prop(&self, index: usize) -> &Prop {
        &self.props[index]
    }

    /// Number of props.
    pub fn len(&self) -> usize {
        self.props.len()
    }

    /// Field memory of every collider in bytes; bodies share their prop's field.
    pub fn field_bytes(&self) -> usize {
        self.props.iter().map(|prop| prop.field_bytes).sum()
    }
}

/// Evaluate one prop's FORM source and build its colliders.
fn build_prop(definition: &PropDefinition) -> AppResult<Prop> {
    let geometry = CompiledScript::parse(definition.source)?.evaluate(&Value::Object(Map::new()), None)?;
    let asset = form_mesh_asset(&geometry, ShadingModel::Layered)?;

    let stopwatch = Stopwatch::start();
    let shape = match definition.collider {
        ColliderSource::BakedMesh => {
            SdfShape::from_triangle_mesh(&positions(&geometry), &geometry.triangles, PHYSICS.bake)?
        }
        ColliderSource::Torus {
            major_radius,
            minor_radius,
        } => SdfShape::primitive(SdfPrimitive::Torus {
            major_radius,
            minor_radius,
        })?,
    };
    let collider_milliseconds = stopwatch.elapsed_milliseconds();

    let (minimum, maximum) = shape.local_bounds();
    Ok(Prop {
        name: definition.name,
        asset,
        box_center: (minimum + maximum) * 0.5,
        box_half_extents: (maximum - minimum) * 0.5,
        mass: shape.volume() * PHYSICS.density,
        triangles: geometry.triangles.len() / 3,
        collider_milliseconds,
        field_bytes: shape.field_bytes(),
        shape,
    })
}

/// FORM positions (script units, which these sources author in meters) as vectors.
fn positions(geometry: &GeometryData) -> Vec<Vec3> {
    let (positions, _) = geometry.positions.as_chunks::<3>();
    positions.iter().map(|xyz| Vec3::new(xyz[0] as f32, xyz[1] as f32, xyz[2] as f32)).collect()
}
