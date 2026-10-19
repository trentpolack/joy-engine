// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine's foundation: shared data and algorithms with no simulation, content, rendering, or platform
//! dependencies. Every other engine crate builds on this one.
//!
//! Coordinate conventions used across the engine:
//! - World space is Y-up and right-handed, measured in meters (or project world units).
//! - Rotations are XYZ Euler radians applied X, then Y, then Z (the matrix `Rz * Ry * Rx`).
//! - Transforms apply scale, then rotation, then translation.
//! - [`Color`] is sRGB-encoded and [`LinearColor`] is linear light; both carry straight alpha.
//!
//! [`math`] owns the engine's vector, matrix, and quaternion types and the [`math::deterministic`] functions seeded
//! simulations use; [`transform`], [`random`], and [`fixed_step`] build on it. [`color`], [`light`], [`mesh_data`],
//! [`material`], [`config_values`], and [`primitive`] define shared engine records, [`mod@format`] writes numbers for text and JSON,
//! and [`base64`] handles the `data:` URL encoding authored documents embed. [`scripting`] describes the engine's script
//! languages (FORM and JOYFX) for editors and language tools.

pub mod base64;
pub mod color;
pub mod config_values;
pub mod fixed_step;
pub mod format;
pub mod light;
pub mod material;
pub mod math;
pub mod mesh_data;
pub mod primitive;
pub mod random;
pub mod scripting;
pub mod transform;

pub use base64::{
    decode_base64,
    encode_base64,
};
pub use color::{
    Color,
    LinearColor,
};
pub use config_values::{
    CONFIG_DOCUMENT_VERSION,
    ConfigApplyMode,
    ConfigError,
    ConfigField,
    ConfigValues,
    create_config_document_values,
    create_config_schema,
    create_config_values,
};
pub use fixed_step::FixedStepLoop;
pub use format::{
    format_number,
    json_number,
};
pub use light::{
    DirectionalLight,
    PointLight,
    SUN_ANGULAR_RADIUS,
    Skylight,
};
pub use material::{
    MaterialAlphaMode,
    PbrMaterial,
    PbrMaterialTextures,
};
pub use math::{
    TAU,
    clamp,
    distance_to_rectangle_edge,
    smoothstep,
};
pub use mesh_data::{
    IndexedMesh3D,
    MeshAssetData,
    PbrMesh,
    ShadedMesh3D,
    ShadedTriangle3D,
};
pub use primitive::PrimitiveShape;
pub use random::{
    RandomSource,
    SeededRandom,
    random_integer,
    random_range,
};
pub use transform::{
    Transform,
    euler_from_quat,
    quat_from_euler,
};

/// Free-form JSON value used for authored data the engine preserves without interpreting.
pub type JsonValue = serde_json::Value;

/// Free-form JSON record used for authored component data that the engine preserves without interpreting.
pub type JsonRecord = serde_json::Map<String, serde_json::Value>;
