// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Converting host records to script values and back through `serde`, so hosts share plain Rust records with scripts
//! without registering a getter and setter per field.
//!
//! Structs become object maps keyed by field name, enums become their variant name (unit variants) or a one-key map,
//! `Option::None` becomes `null`, and sequences become arrays. Integers become script integers and `f64` stays `f64`
//! bit for bit, so seeded state survives the round trip. Reading back is strict: a missing field (without a serde
//! default), an unknown variant, or a wrong type is a [`ScriptError::Conversion`] naming the field.
//!
//! Vectors need one annotation. glam serializes `DVec3` as a plain three-element array, which scripts could not do
//! vector math on, and a plain array of three numbers is not always a vector. Mark vector fields with
//! `#[serde(with = "script_vec3")]` (or `script_vec3::option` for `Option<DVec3>`): they then arrive in scripts as
//! `vec3` values, and fields scripts set to `vec3` read back into them. Unmarked `DVec3` fields stay arrays.

use joy_core::math::DVec3;
use mlua::Lua;
use mlua::LuaSerdeExt;
use mlua::Value;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeSet;

use crate::error::ScriptError;
use crate::standard_library::ScriptVec3;

/// The one-key map a marked vector serializes to on its way to becoming a script `vec3`.
const VECTOR_MARKER: &str = "__joy_vec3";

/// Convert a host record into a script value. `context` names the value in errors (`"unit 12"`).
pub fn to_script_value<T: Serialize>(lua: &Lua, context: &str, value: &T) -> Result<Value, ScriptError> {
    let value = lua.to_value(value).map_err(|error| conversion_error(context, &error))?;
    convert(lua, &value, true, 0, &mut BTreeSet::new()).map_err(|error| conversion_error(context, &error))
}

/// Read a script value as a host record. Rejects foreign VM handles, cycles, and nesting deeper than 64 tables.
/// Repeated references without a cycle are read as independent host records. `null` preserves nullable array slots.
pub fn from_script_value<T: DeserializeOwned>(lua: &Lua, context: &str, value: &Value) -> Result<T, ScriptError> {
    crate::engine::ensure_owner(lua, value).map_err(|error| conversion_error(context, &error))?;
    let value =
        convert(lua, value, false, 0, &mut BTreeSet::new()).map_err(|error| conversion_error(context, &error))?;
    lua.from_value(value).map_err(|error| conversion_error(context, &error))
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

    /// `#[serde(with = "script_vec3::option")]` for `Option<DVec3>` fields; `None` is `null` in scripts.
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

/// Bound recursion before traversing authored values; a graph containing a cycle is not a serde record.
fn convert(
    lua: &Lua,
    value: &Value,
    to_vectors: bool,
    depth: usize,
    ancestors: &mut BTreeSet<usize>,
) -> mlua::Result<Value> {
    crate::engine::ensure_owner(lua, value)?;
    if depth > 64 {
        return Err(mlua::Error::runtime("script record exceeds 64 levels of nesting"));
    }
    if !to_vectors
        && let Value::UserData(userdata) = value
        && let Ok(vector) = userdata.borrow::<ScriptVec3>()
    {
        let marker = lua.create_table()?;
        marker.raw_set(VECTOR_MARKER, lua.create_sequence_from(vector.0.to_array())?)?;
        return Ok(Value::Table(marker));
    }
    let Value::Table(table) = value else {
        return Ok(value.clone());
    };
    let table = crate::frozen::backing(lua, table)?;
    let pointer = table.to_pointer() as usize;
    if !ancestors.insert(pointer) {
        return Err(mlua::Error::runtime("cyclic table cannot be converted to a host record"));
    }
    let result = (|| {
        if to_vectors
            && table.pairs::<Value, Value>().count() == 1
            && let Value::Table(components) = table.raw_get::<Value>(VECTOR_MARKER)?
        {
            let components =
                [components.raw_get::<f64>(1)?, components.raw_get::<f64>(2)?, components.raw_get::<f64>(3)?];
            return Ok(Value::UserData(lua.create_userdata(ScriptVec3(DVec3::from_array(components)))?));
        }
        let copy = lua.create_table()?;
        if table.metatable().is_some_and(|metatable| metatable.to_pointer() == lua.array_metatable().to_pointer()) {
            copy.set_metatable(Some(lua.array_metatable()))?;
        }
        for pair in table.pairs::<Value, Value>() {
            let (key, entry) = pair?;
            if matches!(key, Value::Table(_) | Value::UserData(_) | Value::Function(_) | Value::Thread(_)) {
                return Err(mlua::Error::runtime("host records require scalar table keys"));
            }
            let entry = convert(lua, &entry, to_vectors, depth + 1, ancestors)?;
            copy.raw_set(key, entry)?;
        }
        Ok(Value::Table(copy))
    })();
    ancestors.remove(&pointer);
    result
}
