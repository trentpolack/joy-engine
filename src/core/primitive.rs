// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Primitive shape vocabulary shared by render, physics, and level components.

use serde::{
    Deserialize,
    Serialize,
};

/// Primitive shapes recognized by `render` and `physics` components. Serialized spellings match the TypeScript engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrimitiveShape {
    /// Axis-aligned box; render size and physics half extents describe its dimensions.
    Box,
    /// Sphere; physics radius describes its size.
    Sphere,
    /// Infinite static plane facing +Y before entity rotation.
    Plane,
}

impl PrimitiveShape {
    /// Serialized name used in authored documents.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Box => "box",
            Self::Sphere => "sphere",
            Self::Plane => "plane",
        }
    }

    /// Parse an authored shape name, returning `None` for unknown values.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "box" => Some(Self::Box),
            "sphere" => Some(Self::Sphere),
            "plane" => Some(Self::Plane),
            _ => None,
        }
    }
}
