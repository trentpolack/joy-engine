// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The cooked geometry record: one evaluation's [`GeometryData`] in a compact binary layout that release and browser
//! builds decode instead of evaluating FORM.
//!
//! Vertex data is stored losslessly, so decoded geometry draws, places objects, and feeds gameplay exactly as the
//! evaluation did. Each array picks the narrowest exact encoding: empty, one repeated value (alphas, single-material
//! palettes), `f32` when every value is exactly an `f32`, `u16` indices when every index fits, and otherwise the full
//! width. Materials, parameter descriptors, project metadata, and custom attributes are small next to the vertex data
//! and already serialize losslessly, so they travel as one JSON block.
//!
//! Named outputs and inspections ([`GeometryData::outputs`]) are left out: each carries another copy of its geometry
//! plus a snapshot record, which would roughly triple a game's download, and only authoring tools and hosts that split
//! results by output read them. Hosts that read outputs evaluate those scripts live.
//!
//! Layout (little-endian): the magic `JFGM`, the record version (`u32`), nine arrays (positions, normals, uvs, colors,
//! alphas, then triangles, points, triangle materials, and point materials), each an encoding tag (`u8`), an element
//! count (`u32`), and its payload, and finally the metadata JSON's byte length (`u32`) and UTF-8 text.

use std::collections::BTreeMap;

use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as Json;

use crate::geometry_data::{
    AttributeColumn,
    GeometryData,
};

/// First bytes of every cooked geometry record.
const RECORD_MAGIC: &[u8; 4] = b"JFGM";

/// Current record layout; decoders refuse other versions rather than guess.
pub const COOKED_GEOMETRY_VERSION: u32 = 1;

/// Most elements one array may claim. FORM's budgets stop evaluations far below it (150,000 vertices and 180,000
/// triangles); the bound keeps a damaged count from allocating gigabytes for a repeated value.
const MAXIMUM_ELEMENTS: usize = 1 << 24;

// Array encodings.
const ENCODING_EMPTY: u8 = 0;
const ENCODING_REPEATED: u8 = 1;
const ENCODING_NARROW: u8 = 2;
const ENCODING_FULL: u8 = 3;

/// Why cooked geometry or a cooked bundle could not be read.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CookError {
    /// The bytes are not a cooked record or bundle.
    #[error("not cooked FORM data")]
    NotCooked,
    /// The bytes use a layout version this build does not read.
    #[error("cooked FORM data uses layout version {found}; this build reads version {expected}")]
    UnsupportedVersion {
        /// Version in the data.
        found: u32,
        /// Version this build reads.
        expected: u32,
    },
    /// The data was cooked by a different FORM evaluator build, so its geometry may differ from live evaluation.
    #[error("cooked FORM data comes from evaluator {found}; this build is {expected}")]
    EvaluatorMismatch {
        /// Evaluator version in the data.
        found: String,
        /// This build's [`FORM_EVALUATOR_VERSION`](crate::FORM_EVALUATOR_VERSION).
        expected: String,
    },
    /// The data ends early, holds an unknown encoding, or carries malformed metadata.
    #[error("cooked FORM data is malformed: {0}")]
    Malformed(String),
}

/// Encode `geometry` as a cooked record, leaving out its named outputs (see the [module documentation](self)).
pub fn encode_geometry(geometry: &GeometryData) -> Vec<u8> {
    let mut writer = ByteWriter::default();
    writer.bytes(RECORD_MAGIC);
    writer.u32(COOKED_GEOMETRY_VERSION);

    // Vertex data, then indices, in the record's fixed order.
    for values in [&geometry.positions, &geometry.normals, &geometry.uvs, &geometry.colors, &geometry.alphas] {
        writer.numbers(values);
    }
    for indices in [&geometry.triangles, &geometry.points, &geometry.triangle_materials, &geometry.point_materials] {
        writer.indices(indices);
    }

    let metadata = MetadataView {
        materials: &geometry.materials,
        parameters: &geometry.parameters,
        project: &geometry.project,
        vertex_attributes: &geometry.vertex_attributes,
        triangle_attributes: &geometry.triangle_attributes,
    };
    // Evaluated JSON values and attribute columns always serialize; an empty block would only fail to decode.
    let text = serde_json::to_string(&metadata).unwrap_or_default();
    writer.u32(text.len() as u32);
    writer.bytes(text.as_bytes());
    writer.finish()
}

/// Decode a record written by [`encode_geometry`]. The result has no named outputs.
pub fn decode_geometry(bytes: &[u8]) -> Result<GeometryData, CookError> {
    let mut reader = ByteReader::new(bytes);
    if reader.take(RECORD_MAGIC.len())? != RECORD_MAGIC {
        return Err(CookError::NotCooked);
    }
    let version = reader.u32()?;
    if version != COOKED_GEOMETRY_VERSION {
        return Err(CookError::UnsupportedVersion {
            found: version,
            expected: COOKED_GEOMETRY_VERSION,
        });
    }

    let positions = reader.numbers()?;
    let normals = reader.numbers()?;
    let uvs = reader.numbers()?;
    let colors = reader.numbers()?;
    let alphas = reader.numbers()?;
    let triangles = reader.indices()?;
    let points = reader.indices()?;
    let triangle_materials = reader.indices()?;
    let point_materials = reader.indices()?;

    let length = reader.u32()? as usize;
    let text = std::str::from_utf8(reader.take(length)?).map_err(|error| CookError::Malformed(error.to_string()))?;
    let metadata: MetadataRecord =
        serde_json::from_str(text).map_err(|error| CookError::Malformed(format!("metadata: {error}")))?;
    if !reader.is_finished() {
        return Err(CookError::Malformed("bytes follow the record".into()));
    }

    Ok(GeometryData {
        positions,
        normals,
        uvs,
        colors,
        alphas,
        triangles,
        points,
        triangle_materials,
        point_materials,
        materials: metadata.materials,
        parameters: metadata.parameters,
        project: metadata.project,
        vertex_attributes: metadata.vertex_attributes,
        triangle_attributes: metadata.triangle_attributes,
        outputs: None,
    })
}

/// The JSON block of a record, borrowed for encoding: everything but the bulk arrays and the left-out outputs.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MetadataView<'a> {
    materials: &'a [Json],
    parameters: &'a [Json],
    project: &'a Option<Json>,
    vertex_attributes: &'a BTreeMap<String, AttributeColumn>,
    triangle_attributes: &'a BTreeMap<String, AttributeColumn>,
}

/// The JSON block of a record, owned after decoding.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MetadataRecord {
    materials: Vec<Json>,
    parameters: Vec<Json>,
    project: Option<Json>,
    vertex_attributes: BTreeMap<String, AttributeColumn>,
    triangle_attributes: BTreeMap<String, AttributeColumn>,
}

/// Appends little-endian values; shared with the bundle writer.
#[derive(Default)]
pub(crate) struct ByteWriter {
    bytes: Vec<u8>,
}

impl ByteWriter {
    pub(crate) fn bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// `values` in the narrowest exact encoding.
    fn numbers(&mut self, values: &[f64]) {
        let count = values.len() as u32;
        let repeated = values.first().filter(|first| values.iter().all(|value| value.to_bits() == first.to_bits()));
        let narrow = values.iter().all(|value| f64::from(*value as f32).to_bits() == value.to_bits());
        match repeated {
            _ if values.is_empty() => self.header(ENCODING_EMPTY, count),
            Some(value) => {
                self.header(ENCODING_REPEATED, count);
                self.bytes(&value.to_le_bytes());
            }
            None if narrow => {
                self.header(ENCODING_NARROW, count);
                for value in values {
                    self.bytes(&(*value as f32).to_le_bytes());
                }
            }
            None => {
                self.header(ENCODING_FULL, count);
                for value in values {
                    self.bytes(&value.to_le_bytes());
                }
            }
        }
    }

    /// `indices` in the narrowest exact encoding.
    fn indices(&mut self, indices: &[u32]) {
        let count = indices.len() as u32;
        let repeated = indices.first().filter(|first| indices.iter().all(|index| index == *first));
        let narrow = indices.iter().all(|index| *index <= u32::from(u16::MAX));
        match repeated {
            _ if indices.is_empty() => self.header(ENCODING_EMPTY, count),
            Some(index) => {
                self.header(ENCODING_REPEATED, count);
                self.u32(*index);
            }
            None if narrow => {
                self.header(ENCODING_NARROW, count);
                for index in indices {
                    self.bytes(&(*index as u16).to_le_bytes());
                }
            }
            None => {
                self.header(ENCODING_FULL, count);
                for index in indices {
                    self.u32(*index);
                }
            }
        }
    }

    fn header(&mut self, encoding: u8, count: u32) {
        self.bytes.push(encoding);
        self.u32(count);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

/// Reads little-endian values, failing instead of panicking when the data ends early; shared with the bundle reader.
pub(crate) struct ByteReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> ByteReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            offset: 0,
        }
    }

    pub(crate) fn offset(&self) -> usize {
        self.offset
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.offset == self.bytes.len()
    }

    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8], CookError> {
        let end = self.offset.checked_add(length).filter(|end| *end <= self.bytes.len());
        let Some(end) = end else {
            return Err(CookError::Malformed("data ends early".into()));
        };
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, CookError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], CookError> {
        let mut array = [0; N];
        array.copy_from_slice(self.take(N)?);
        Ok(array)
    }

    fn numbers(&mut self) -> Result<Vec<f64>, CookError> {
        let (encoding, count) = self.header()?;
        match encoding {
            ENCODING_EMPTY if count == 0 => Ok(Vec::new()),
            ENCODING_REPEATED => Ok(vec![f64::from_le_bytes(self.array()?); count]),
            ENCODING_NARROW => {
                self.elements(count, 4, |bytes| f64::from(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
            }
            ENCODING_FULL => self.elements(count, 8, |bytes| {
                f64::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]])
            }),
            _ => Err(CookError::Malformed(format!("unknown number encoding {encoding}"))),
        }
    }

    fn indices(&mut self) -> Result<Vec<u32>, CookError> {
        let (encoding, count) = self.header()?;
        match encoding {
            ENCODING_EMPTY if count == 0 => Ok(Vec::new()),
            ENCODING_REPEATED => Ok(vec![self.u32()?; count]),
            ENCODING_NARROW => self.elements(count, 2, |bytes| u32::from(u16::from_le_bytes([bytes[0], bytes[1]]))),
            ENCODING_FULL => {
                self.elements(count, 4, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            }
            _ => Err(CookError::Malformed(format!("unknown index encoding {encoding}"))),
        }
    }

    /// An array's encoding tag and element count, refusing counts no evaluation could produce.
    fn header(&mut self) -> Result<(u8, usize), CookError> {
        let encoding = self.array::<1>()?[0];
        let count = self.u32()? as usize;
        if count > MAXIMUM_ELEMENTS {
            return Err(CookError::Malformed(format!("an array claims {count} elements")));
        }
        Ok((encoding, count))
    }

    /// `count` elements of `width` bytes each; the length check runs before allocating, so a corrupt count cannot
    /// reserve more memory than the data holds.
    fn elements<T>(&mut self, count: usize, width: usize, read: impl Fn(&[u8]) -> T) -> Result<Vec<T>, CookError> {
        let length = count.checked_mul(width).ok_or_else(|| CookError::Malformed("array length overflows".into()))?;
        Ok(self.take(length)?.chunks_exact(width).map(read).collect())
    }
}

#[cfg(test)]
#[path = "cooked_geometry.test.rs"]
mod tests;
