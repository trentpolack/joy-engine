// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Named output and inspection results: what one `output(name, value)` or `inspect(name, value)` declared.
//!
//! An evaluation keeps each declared value in its native form and converts it to the documented snapshot record only
//! when a reader asks for it (or the result is serialized), so scripts that output large geometry do not pay for a
//! JSON copy nobody reads.

use serde::{
    Deserialize,
    Deserializer,
    Serialize,
    Serializer,
};
use serde_json::Value as Json;

use crate::geometry_data::GeometryData;
use crate::procedural::{
    Geometry,
    Instances,
};

/// One named output or inspection.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NamedOutput {
    /// The name the script gave it: unique, nonempty, and at most 64 characters.
    pub name: String,
    /// Whether it is part of the evaluation's result (`output`) or only inspected (`inspect`).
    pub visible: bool,
    /// The declared geometry or instance collection as it was when declared.
    pub value: OutputValue,
    /// `value` alone realized with the evaluation's palette.
    pub geometry: GeometryData,
}

/// A declared geometry or instance collection. Read it with [`OutputValue::json`], which returns the documented
/// snapshot record: `{kind: "geometry", faces, attributes: {points, faces, corners}, materials?}` or
/// `{kind: "instances", source, sites, material}`.
#[derive(Clone, Debug)]
pub struct OutputValue(Snapshot);

#[derive(Clone, Debug)]
enum Snapshot {
    Geometry(Box<Geometry>),
    Instances(Box<Instances>),
    /// A record read back from serialized results.
    Record(Json),
}

impl OutputValue {
    pub(crate) fn geometry(geometry: Geometry) -> Self {
        Self(Snapshot::Geometry(Box::new(geometry)))
    }

    pub(crate) fn instances(collection: Instances) -> Self {
        Self(Snapshot::Instances(Box::new(collection)))
    }

    /// The snapshot record. Converting large geometry allocates; read it once and keep the result.
    pub fn json(&self) -> Json {
        match &self.0 {
            Snapshot::Geometry(geometry) => geometry.json(),
            Snapshot::Instances(collection) => collection.json(),
            Snapshot::Record(record) => record.clone(),
        }
    }

    /// Whether the value is an instance collection rather than geometry.
    pub fn is_instances(&self) -> bool {
        match &self.0 {
            Snapshot::Geometry(_) => false,
            Snapshot::Instances(_) => true,
            Snapshot::Record(record) => record["kind"] == "instances",
        }
    }
}

impl PartialEq for OutputValue {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Snapshot::Geometry(a), Snapshot::Geometry(b)) => a == b,
            (Snapshot::Instances(a), Snapshot::Instances(b)) => a == b,
            _ => self.json() == other.json(),
        }
    }
}

impl Serialize for OutputValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for OutputValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self(Snapshot::Record(Json::deserialize(deserializer)?)))
    }
}
