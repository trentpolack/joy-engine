// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Converting host records to script values and back through `serde`, so hosts share plain Rust records with scripts
//! without registering a getter and setter per field.
//!
//! Structs become object maps keyed by field name, enums become their variant name (unit variants) or a one-key map,
//! `Option::None` becomes `()`, and sequences become arrays. Integers become script integers and `f64` stays `f64`
//! bit for bit, so seeded state survives the round trip. Reading back is strict: a missing field (without a serde
//! default), an unknown variant, or a wrong type is a [`ScriptError::Conversion`] naming the field.
//!
//! Vectors need one annotation. glam serializes `DVec3` as a plain three-element array, which scripts could not do
//! vector math on, and a plain array of three numbers is not always a vector. Mark vector fields with
//! `#[serde(with = "script_vec3")]` (or `script_vec3::option` for `Option<DVec3>`): they then arrive in scripts as
//! `vec3` values, and fields scripts set to `vec3` read back into them. Unmarked `DVec3` fields stay arrays.

use joy_core::math::DVec3;
use rhai::Array;
use rhai::Dynamic;
use rhai::Map;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::ScriptError;

/// The one-key map a marked vector serializes to on its way to becoming a script `vec3`.
const VECTOR_MARKER: &str = "__joy_vec3";

/// Convert a host record into a script value. `context` names the value in errors (`"unit 12"`).
pub fn to_script_value<T: Serialize>(context: &str, value: &T) -> Result<Dynamic, ScriptError> {
    let value = rhai::serde::to_dynamic(value).map_err(|error| conversion_error(context, &error))?;
    Ok(markers_to_vectors(value))
}

/// Read a script value as a host record. `context` names the value in errors (`"data/parts PARTS"`).
pub fn from_script_value<T: DeserializeOwned>(context: &str, value: &Dynamic) -> Result<T, ScriptError> {
    let value = vectors_to_markers(value);
    rhai::serde::from_dynamic(&value).map_err(|error| conversion_error(context, &error))
}

/// `#[serde(with = "script_vec3")]` for `DVec3` fields shared with scripts (see the module documentation).
pub mod script_vec3 {
    use joy_core::math::DVec3;
    use serde::Deserialize;
    use serde::Deserializer;
    use serde::Serializer;
    use serde::ser::SerializeMap;

    use super::VECTOR_MARKER;

    /// Serialize as the vector marker the script conversion turns into `vec3`.
    pub fn serialize<S: Serializer>(vector: &DVec3, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(VECTOR_MARKER, &[vector.x, vector.y, vector.z])?;
        map.end()
    }

    /// Read a marked vector, or a plain `[x, y, z]` array (as in JSON documents).
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<DVec3, D::Error> {
        let [x, y, z] = VectorRepresentation::deserialize(deserializer)?.components();
        Ok(DVec3::new(x, y, z))
    }

    /// `#[serde(with = "script_vec3::option")]` for `Option<DVec3>` fields; `None` is `()` in scripts.
    pub mod option {
        use joy_core::math::DVec3;
        use serde::Deserialize;
        use serde::Deserializer;
        use serde::Serializer;

        use super::VectorRepresentation;

        /// Serialize `Some` as a vector marker and `None` as unit.
        pub fn serialize<S: Serializer>(vector: &Option<DVec3>, serializer: S) -> Result<S::Ok, S::Error> {
            match vector {
                Some(vector) => super::serialize(vector, serializer),
                None => serializer.serialize_none(),
            }
        }

        /// Read an optional vector.
        pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<DVec3>, D::Error> {
            let vector = Option::<VectorRepresentation>::deserialize(deserializer)?;
            Ok(vector.map(|vector| {
                let [x, y, z] = vector.components();
                DVec3::new(x, y, z)
            }))
        }
    }

    /// Either spelling of a vector.
    #[derive(Deserialize)]
    #[serde(untagged)]
    pub(super) enum VectorRepresentation {
        Marked {
            #[serde(rename = "__joy_vec3")]
            components: [f64; 3],
        },
        Plain([f64; 3]),
    }

    impl VectorRepresentation {
        pub(super) fn components(self) -> [f64; 3] {
            match self {
                VectorRepresentation::Marked {
                    components,
                } => components,
                VectorRepresentation::Plain(components) => components,
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------------------------
// Implementation
// -------------------------------------------------------------------------------------------------------------------

fn conversion_error(context: &str, error: &dyn std::fmt::Display) -> ScriptError {
    ScriptError::Conversion {
        context: context.to_string(),
        message: error.to_string(),
    }
}

/// Replace every vector marker map with a `vec3`, in place.
fn markers_to_vectors(value: Dynamic) -> Dynamic {
    if value.is_map() {
        let mut map = value.cast::<Map>();
        if map.len() == 1
            && let Some(components) = map.get(VECTOR_MARKER).and_then(vector_components)
        {
            return Dynamic::from(DVec3::from_array(components));
        }
        for entry in map.values_mut() {
            *entry = markers_to_vectors(std::mem::take(entry));
        }
        return Dynamic::from_map(map);
    }
    if value.is_array() {
        let array: Array = value.cast::<Array>().into_iter().map(markers_to_vectors).collect();
        return Dynamic::from_array(array);
    }
    value
}

/// A copy of `value` with every `vec3` replaced by a vector marker map, for deserializing.
fn vectors_to_markers(value: &Dynamic) -> Dynamic {
    if let Some(vector) = value.read_lock::<DVec3>() {
        let components: Array = vector.to_array().into_iter().map(Dynamic::from_float).collect();
        let mut map = Map::new();
        map.insert(VECTOR_MARKER.into(), Dynamic::from_array(components));
        return Dynamic::from_map(map);
    }
    if let Some(map) = value.read_lock::<Map>() {
        let copy: Map = map.iter().map(|(key, entry)| (key.clone(), vectors_to_markers(entry))).collect();
        return Dynamic::from_map(copy);
    }
    if let Some(array) = value.read_lock::<Array>() {
        return Dynamic::from_array(array.iter().map(vectors_to_markers).collect());
    }
    value.clone()
}

/// The three floats of a marker's component array.
fn vector_components(value: &Dynamic) -> Option<[f64; 3]> {
    let array = value.read_lock::<Array>()?;
    if array.len() != 3 {
        return None;
    }
    let component = |index: usize| array[index].as_float().ok();
    Some([component(0)?, component(1)?, component(2)?])
}
