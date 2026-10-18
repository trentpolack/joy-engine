// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Joy Engine's foundation: shared data and algorithms with no simulation, content, rendering, or platform
//! dependencies. Every other engine crate builds on this one.
//!
//! Coordinate conventions used across the engine:
//! - World space is Y-up and right-handed, measured in meters (or project world units).
//! - Rotations are XYZ Euler radians applied X, then Y, then Z (the matrix `Rz * Ry * Rx`).
//! - Transforms apply scale, then rotation, then translation.
//! - Colors are linear RGB(A) unless a function says otherwise.
//!
//! [`color`], [`config_values`], [`primitive`], [`math`], [`random`], and [`types`] define shared engine data and algorithms.
//! [`js_math`] preserves numerical semantics used by recorded simulation fixtures, [`mod@format`] supplies portable
//! number formatting for text and JSON writers, and [`base64`] handles the `data:` URL encoding authored documents embed.
//! [`scripting`] describes the engine's script languages (FORM and JOYFX) for editors and language tools.

pub mod base64;
pub mod color;
pub mod config_values;
pub mod format;
pub mod js_math;
pub mod math;
pub mod post_processing;
pub mod primitive;
pub mod random;
pub mod scripting;
pub mod transform;
pub mod types;

pub use base64::{
    decode_base64,
    encode_base64,
};
pub use color::{
    Rgb,
    Rgba,
    parse_hex_color,
    with_alpha,
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
pub use format::{
    format_js_number,
    json_number,
};
pub use math::{
    Direction2,
    TAU,
    clamp,
    direction_between,
    distance_between,
    distance_to_rectangle_edge,
    js_hypot,
    random_integer,
    random_range,
    smoothstep,
};
pub use post_processing::{
    PostProcessingConfig,
    PostProcessingError,
    PostProcessingProfile,
};
pub use primitive::PrimitiveShape;
pub use random::{
    RandomSource,
    SeededRandom,
};
pub use transform::{
    Transform,
    euler_from_quat,
    quat_from_euler,
};
pub use types::{
    AffineTransform3D,
    Color,
    GeometryBounds,
    IndexedMesh3D,
    MaterialAlphaMode,
    MeshAssetData,
    MovingBody,
    PbrMaterial,
    PbrMaterialTextures,
    PbrMesh,
    ShadedMesh3D,
    ShadedTriangle3D,
    Vector2,
    Vector3,
    Velocity2,
};

/// Free-form JSON value used for authored data the engine preserves without interpreting (TypeScript `JsonValue`).
pub type JsonValue = serde_json::Value;

/// Free-form JSON record used for authored component data that the engine preserves without interpreting.
pub type JsonRecord = serde_json::Map<String, serde_json::Value>;
