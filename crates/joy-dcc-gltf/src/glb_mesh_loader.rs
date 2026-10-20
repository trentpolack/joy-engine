// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Decode one indexed triangle mesh from in-memory glTF 2.0 data.
//!
//! This module implements the supported subset of the glTF 2.0 specification directly:
//! - binary GLB containers (JSON chunk plus optional BIN chunk) and JSON glTF text;
//! - buffers stored in the GLB BIN chunk or embedded as base64 `data:` URIs (loading external buffers is the caller's
//!   job, like fetching the asset itself);
//! - accessors with buffer-view and accessor offsets, interleaved strides, sparse substitution, and accessors without
//!   a buffer view (zero-filled before sparse substitution).
//!
//! One mesh is selected (by exact name, or the first), and it must hold exactly one triangle
//! primitive with float XYZ positions and normals, unindexed triangles receive sequential indices, and scene/node
//! transforms, materials, animation, and other primitives are not interpreted. Error messages for authored-content
//! problems preserve the established asset contract; container and accessor errors are specific to this decoder.

use serde_json::{
    Map,
    Value,
};

use joy_core::math::{
    DVec3,
    Vec3,
};
use joy_render_types::Mesh;

/// `glTF` magic at the start of a binary container (little-endian).
const GLB_MAGIC: u32 = 0x4654_6C67;

/// The supported binary container and asset version.
const GLTF_VERSION: u32 = 2;

/// GLB header size in bytes: magic, version, and total length.
const GLB_HEADER_BYTES: usize = 12;

/// GLB chunk header size in bytes: length and type.
const CHUNK_HEADER_BYTES: usize = 8;

/// GLB chunk type of the JSON document (`JSON`).
const JSON_CHUNK: u32 = 0x4E4F_534A;

/// GLB chunk type of the binary buffer (`BIN\0`).
const BINARY_CHUNK: u32 = 0x004E_4942;

/// Primitive mode for triangle lists.
const TRIANGLES_MODE: u64 = 4;

/// Accessor component types.
const BYTE_COMPONENT: u64 = 5120;
const UNSIGNED_BYTE_COMPONENT: u64 = 5121;
const SHORT_COMPONENT: u64 = 5122;
const UNSIGNED_SHORT_COMPONENT: u64 = 5123;
const UNSIGNED_INT_COMPONENT: u64 = 5125;
const FLOAT_COMPONENT: u64 = 5126;

/// Components in a position or normal.
const VEC3_COMPONENTS: usize = 3;

/// Default mesh name for meshes without one.
const DEFAULT_MESH_NAME: &str = "mesh";

/// Prefix and marker of an embedded base64 buffer.
const DATA_URI_PREFIX: &str = "data:";
const BASE64_MARKER: &str = ";base64,";

/// Extensions that change how geometry is stored; required uses cannot be decoded here.
const GEOMETRY_EXTENSIONS: [&str; 3] =
    ["KHR_draco_mesh_compression", "EXT_meshopt_compression", "KHR_mesh_quantization"];

/// A glTF asset could not be decoded into an indexed mesh.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GlbError {
    /// The binary container header or chunk table was malformed.
    #[error("Invalid glTF container: {0}")]
    InvalidContainer(String),
    /// The JSON document could not be parsed.
    #[error("Invalid glTF JSON: {0}")]
    Json(String),
    /// The container or asset was not glTF 2.0.
    #[error("Only glTF 2.0 assets are supported.")]
    UnsupportedVersion,
    /// The asset requires a geometry extension this decoder does not implement.
    #[error("The glTF asset requires unsupported extension {0}.")]
    UnsupportedExtension(String),
    /// A buffer referenced external data.
    #[error("The glTF buffer \"{0}\" is not embedded; load external buffers before parsing.")]
    ExternalBuffer(String),
    /// An accessor, buffer view, or buffer reference was missing or out of bounds.
    #[error("Invalid glTF accessor data: {0}")]
    InvalidAccessor(String),
    /// The asset had no meshes.
    #[error("The glTF asset does not contain a mesh.")]
    MissingMesh,
    /// No mesh had the requested name.
    #[error("The glTF asset does not contain the requested mesh \"{0}\".")]
    MissingNamedMesh(String),
    /// The selected mesh did not have exactly one primitive.
    #[error("The glTF mesh \"{0}\" must contain exactly one primitive.")]
    PrimitiveCount(String),
    /// The primitive was not triangles with float XYZ positions and normals and optional unsigned indices.
    #[error("The glTF asset must contain a triangle mesh with float positions/normals and optional unsigned indices.")]
    UnsupportedPrimitive,
    /// The mesh had no vertices or no indices.
    #[error("The glTF mesh must contain at least one indexed triangle.")]
    EmptyMesh,
    /// Position and normal counts differed.
    #[error("The glTF mesh must contain matching position and normal counts.")]
    MismatchedNormals,
    /// The index count was not a multiple of three.
    #[error("The glTF mesh indices must describe complete triangles.")]
    IncompleteTriangles,
    /// A position or normal was NaN or infinite.
    #[error("The glTF mesh must contain finite position and normal values.")]
    NonFiniteValues,
    /// An index named a vertex past the end.
    #[error("The glTF mesh contains an index outside the vertex range.")]
    IndexOutOfRange,
}

/// Decode one indexed triangle mesh from binary (GLB) or JSON glTF 2.0 bytes.
///
/// `mesh_name` selects a mesh by exact name, so exporters that retain other scene objects still work; `None` (or an
/// empty name) selects the first mesh. Returns a copied model-space [`Mesh`] owned by the caller, with positions,
/// normals, and indices (one unnamed material slot); float positions widen to `f64` exactly.
pub fn parse_glb_mesh(bytes: &[u8], mesh_name: Option<&str>) -> Result<Mesh, GlbError> {
    let asset = GltfAsset::parse(bytes)?;
    asset.check_requirements()?;
    let mesh = asset.select_mesh(mesh_name.filter(|name| !name.is_empty()))?;
    let name = mesh.get("name").and_then(Value::as_str).unwrap_or(DEFAULT_MESH_NAME).to_owned();

    let primitives = mesh.get("primitives").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    let [primitive] = primitives else {
        return Err(GlbError::PrimitiveCount(name));
    };
    let attributes = primitive.get("attributes");
    let position = asset.accessor_layout(attributes.and_then(|attributes| attributes.get("POSITION")))?;
    let normal = asset.accessor_layout(attributes.and_then(|attributes| attributes.get("NORMAL")))?;
    let indices = match primitive.get("indices") {
        None => None,
        Some(reference) => asset.accessor_layout(Some(reference))?,
    };

    let triangles = primitive.get("mode").is_none_or(|mode| mode.as_u64() == Some(TRIANGLES_MODE));
    let unsigned_indices = indices.is_none_or(|layout| {
        [UNSIGNED_BYTE_COMPONENT, UNSIGNED_SHORT_COMPONENT, UNSIGNED_INT_COMPONENT].contains(&layout.component_type)
    });
    let (Some(position), Some(normal)) = (position, normal) else {
        return Err(GlbError::UnsupportedPrimitive);
    };
    if !triangles || !position.is_float_vec3() || !normal.is_float_vec3() || !unsigned_indices {
        return Err(GlbError::UnsupportedPrimitive);
    }

    let positions = asset.read_accessor(position)?;
    let normals = asset.read_accessor(normal)?;
    // FORM and other exporters may emit unindexed triangles; normalize them to the same owned indexed contract.
    let indices: Vec<u32> = match indices {
        // Unsigned 8-, 16-, and 32-bit components convert to u32 exactly.
        Some(layout) => asset.read_accessor(layout)?.into_iter().map(|index| index as u32).collect(),
        None => (0..(positions.len() / VEC3_COMPONENTS) as u32).collect(),
    };

    if positions.is_empty() || indices.is_empty() {
        return Err(GlbError::EmptyMesh);
    }
    if positions.len() != normals.len() {
        return Err(GlbError::MismatchedNormals);
    }
    if !indices.len().is_multiple_of(3) {
        return Err(GlbError::IncompleteTriangles);
    }
    if !positions.iter().chain(&normals).all(|value| value.is_finite()) {
        return Err(GlbError::NonFiniteValues);
    }
    let vertex_count = positions.len() / VEC3_COMPONENTS;
    if indices.iter().any(|index| *index as usize >= vertex_count) {
        return Err(GlbError::IndexOutOfRange);
    }
    let positions = positions.as_chunks::<3>().0.iter().map(|position| DVec3::from_array(*position)).collect();
    let normals =
        normals.as_chunks::<3>().0.iter().map(|[x, y, z]| Vec3::new(*x as f32, *y as f32, *z as f32)).collect();
    Ok(Mesh {
        name,
        normals: Some(normals),
        ..Mesh::new(positions, indices)
    })
}

// ---------------------------------------------------------------------------------------------------------------------
// Container and document
// ---------------------------------------------------------------------------------------------------------------------

/// A parsed glTF document with its embedded binary data.
struct GltfAsset {
    document: Map<String, Value>,

    // Buffers by index, resolved from the GLB chunk or data URIs. A buffer that cannot be resolved keeps its error,
    // which surfaces only if an accessor the mesh needs reads from it.
    buffers: Vec<Result<Vec<u8>, GlbError>>,
}

/// Resolved accessor metadata, checked before any data is read.
#[derive(Clone, Copy, Debug, PartialEq)]
struct AccessorLayout {
    index: usize,
    component_type: u64,
    components: usize,
    count: usize,
}

impl AccessorLayout {
    /// Whether the accessor holds float XYZ vectors (positions and normals).
    fn is_float_vec3(&self) -> bool {
        self.component_type == FLOAT_COMPONENT && self.components == VEC3_COMPONENTS
    }
}

impl GltfAsset {
    /// Split a GLB container (or read JSON glTF text) and resolve embedded buffers.
    fn parse(bytes: &[u8]) -> Result<Self, GlbError> {
        let (json, binary) =
            if bytes.len() >= 4 && read_u32(bytes, 0) == Some(GLB_MAGIC) { split_glb(bytes)? } else { (bytes, None) };

        // Exporters occasionally pad the JSON chunk with NUL bytes instead of spaces.
        let end = json.iter().rposition(|byte| *byte != 0).map_or(0, |last| last + 1);
        let value: Value = serde_json::from_slice(&json[..end]).map_err(|error| GlbError::Json(error.to_string()))?;
        let Value::Object(document) = value else {
            return Err(GlbError::Json("the document is not an object".to_owned()));
        };

        let declared = document.get("buffers").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        let buffers =
            declared.iter().enumerate().map(|(index, buffer)| resolve_buffer(buffer, index, binary)).collect();
        Ok(Self { document, buffers })
    }

    /// Reject assets that are not glTF 2.0 or that require geometry extensions.
    fn check_requirements(&self) -> Result<(), GlbError> {
        let version = self.document.get("asset").and_then(|asset| asset.get("version")).and_then(Value::as_str);
        if version.is_some_and(|version| version.split('.').next() != Some("2")) {
            return Err(GlbError::UnsupportedVersion);
        }
        let required = self.document.get("extensionsRequired").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        if let Some(extension) =
            required.iter().filter_map(Value::as_str).find(|name| GEOMETRY_EXTENSIONS.contains(name))
        {
            return Err(GlbError::UnsupportedExtension(extension.to_owned()));
        }
        Ok(())
    }

    /// The mesh with an exact name, or the first mesh.
    fn select_mesh(&self, mesh_name: Option<&str>) -> Result<&Value, GlbError> {
        let meshes = self.document.get("meshes").and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
        match mesh_name {
            None => meshes.first().ok_or(GlbError::MissingMesh),
            Some(name) => meshes
                .iter()
                .find(|mesh| mesh.get("name").and_then(Value::as_str) == Some(name))
                .ok_or_else(|| GlbError::MissingNamedMesh(name.to_owned())),
        }
    }

    /// Accessor metadata for an accessor reference, or `None` when the reference is absent.
    fn accessor_layout(&self, reference: Option<&Value>) -> Result<Option<AccessorLayout>, GlbError> {
        let Some(reference) = reference else {
            return Ok(None);
        };
        let index = as_index(Some(reference)).ok_or_else(|| invalid("accessor reference is not an index"))?;
        let accessor = self.indexed("accessors", index)?;
        let component_type =
            accessor.get("componentType").and_then(Value::as_u64).ok_or_else(|| invalid("missing componentType"))?;
        let components = accessor
            .get("type")
            .and_then(Value::as_str)
            .and_then(element_components)
            .ok_or_else(|| invalid("unknown accessor type"))?;
        let count = as_index(accessor.get("count")).ok_or_else(|| invalid("missing count"))?;
        Ok(Some(AccessorLayout { index, component_type, components, count }))
    }

    /// Decode every component of an accessor, applying strides and sparse substitution.
    fn read_accessor(&self, layout: AccessorLayout) -> Result<Vec<f64>, GlbError> {
        let accessor = self.indexed("accessors", layout.index)?;
        let component_bytes = component_size(layout.component_type).ok_or_else(|| invalid("unknown componentType"))?;
        let element_bytes = component_bytes * layout.components;
        let total = layout.count.checked_mul(layout.components).ok_or_else(|| invalid("accessor is too large"))?;

        let mut values = vec![0.0; total];
        if let Some(view_index) = as_index(accessor.get("bufferView")) {
            let view = self.buffer_view(view_index)?;
            let stride = view.stride.unwrap_or(element_bytes);
            let start = view.offset + as_index(accessor.get("byteOffset")).unwrap_or(0);
            for element in 0..layout.count {
                let element_start = start + (element * stride);
                let slice = view.slice(element_start, element_bytes)?;
                decode_components(
                    slice,
                    layout.component_type,
                    &mut values[element * layout.components..][..layout.components],
                );
            }
        }

        if let Some(sparse) = accessor.get("sparse") {
            self.apply_sparse(sparse, layout, element_bytes, &mut values)?;
        }
        Ok(values)
    }

    /// Replace the elements a sparse block lists.
    fn apply_sparse(
        &self,
        sparse: &Value,
        layout: AccessorLayout,
        element_bytes: usize,
        values: &mut [f64],
    ) -> Result<(), GlbError> {
        let count = as_index(sparse.get("count")).ok_or_else(|| invalid("sparse count"))?;
        let indices = sparse.get("indices").ok_or_else(|| invalid("sparse indices"))?;
        let substitutes = sparse.get("values").ok_or_else(|| invalid("sparse values"))?;
        let index_type =
            indices.get("componentType").and_then(Value::as_u64).ok_or_else(|| invalid("sparse index type"))?;
        let index_bytes = match index_type {
            UNSIGNED_BYTE_COMPONENT | UNSIGNED_SHORT_COMPONENT | UNSIGNED_INT_COMPONENT => {
                component_size(index_type).unwrap_or(1)
            }
            _ => return Err(invalid("sparse indices must be unsigned")),
        };
        let index_view =
            self.buffer_view(as_index(indices.get("bufferView")).ok_or_else(|| invalid("sparse index view"))?)?;
        let value_view =
            self.buffer_view(as_index(substitutes.get("bufferView")).ok_or_else(|| invalid("sparse value view"))?)?;
        let index_start = index_view.offset + as_index(indices.get("byteOffset")).unwrap_or(0);
        let value_start = value_view.offset + as_index(substitutes.get("byteOffset")).unwrap_or(0);

        let mut target = [0.0];
        for entry in 0..count {
            decode_components(
                index_view.slice(index_start + (entry * index_bytes), index_bytes)?,
                index_type,
                &mut target,
            );
            let element = target[0] as usize;
            if element >= layout.count {
                return Err(invalid("sparse index outside the accessor"));
            }
            let slice = value_view.slice(value_start + (entry * element_bytes), element_bytes)?;
            decode_components(
                slice,
                layout.component_type,
                &mut values[element * layout.components..][..layout.components],
            );
        }
        Ok(())
    }

    /// A buffer view with its resolved buffer bytes.
    fn buffer_view(&self, index: usize) -> Result<BufferView<'_>, GlbError> {
        let view = self.indexed("bufferViews", index)?;
        let buffer_index = as_index(view.get("buffer")).ok_or_else(|| invalid("buffer view without buffer"))?;
        let data = match self.buffers.get(buffer_index) {
            None => return Err(invalid("buffer index out of range")),
            Some(Err(error)) => return Err(error.clone()),
            Some(Ok(data)) => data.as_slice(),
        };
        let offset = as_index(view.get("byteOffset")).unwrap_or(0);
        let length = as_index(view.get("byteLength")).ok_or_else(|| invalid("buffer view without byteLength"))?;
        let end = offset
            .checked_add(length)
            .filter(|end| *end <= data.len())
            .ok_or_else(|| invalid("buffer view outside its buffer"))?;
        Ok(BufferView { data, offset, end, stride: as_index(view.get("byteStride")).filter(|stride| *stride > 0) })
    }

    /// An object from a top-level array, by index.
    fn indexed(&self, collection: &str, index: usize) -> Result<&Value, GlbError> {
        self.document
            .get(collection)
            .and_then(Value::as_array)
            .and_then(|items| items.get(index))
            .ok_or_else(|| invalid(&format!("{collection}[{index}] is missing")))
    }
}

/// A bounded window into one buffer.
struct BufferView<'a> {
    data: &'a [u8],
    offset: usize,
    end: usize,
    stride: Option<usize>,
}

impl<'a> BufferView<'a> {
    /// `length` bytes starting at absolute buffer offset `start`, which must lie inside the view.
    fn slice(&self, start: usize, length: usize) -> Result<&'a [u8], GlbError> {
        let end = start.checked_add(length).ok_or_else(|| invalid("element offset overflow"))?;
        if start < self.offset || end > self.end {
            return Err(invalid("element outside its buffer view"));
        }
        Ok(&self.data[start..end])
    }
}

/// Split a GLB container into its JSON chunk and optional first BIN chunk.
fn split_glb(bytes: &[u8]) -> Result<(&[u8], Option<&[u8]>), GlbError> {
    let header = |offset: usize| {
        read_u32(bytes, offset).ok_or_else(|| GlbError::InvalidContainer("truncated header".to_owned()))
    };
    if header(4)? != GLTF_VERSION {
        return Err(GlbError::UnsupportedVersion);
    }
    let length = header(8)? as usize;
    if length < GLB_HEADER_BYTES || length > bytes.len() {
        return Err(GlbError::InvalidContainer("declared length does not match the data".to_owned()));
    }
    let bytes = &bytes[..length];

    let mut json = None;
    let mut binary = None;
    let mut offset = GLB_HEADER_BYTES;
    while offset + CHUNK_HEADER_BYTES <= bytes.len() {
        let chunk_length = read_u32(bytes, offset).unwrap_or(0) as usize;
        let chunk_type = read_u32(bytes, offset + 4).unwrap_or(0);
        let start = offset + CHUNK_HEADER_BYTES;
        let end = start
            .checked_add(chunk_length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| GlbError::InvalidContainer("chunk extends past the container".to_owned()))?;
        match chunk_type {
            JSON_CHUNK if json.is_none() => json = Some(&bytes[start..end]),
            BINARY_CHUNK if binary.is_none() => binary = Some(&bytes[start..end]),
            _ => {}
        }
        if json.is_none() {
            return Err(GlbError::InvalidContainer("the first chunk must be JSON".to_owned()));
        }
        offset = end;
    }
    let json = json.ok_or_else(|| GlbError::InvalidContainer("missing JSON chunk".to_owned()))?;
    Ok((json, binary))
}

/// Resolve one declared buffer from the BIN chunk or a base64 data URI. Errors are kept per buffer so assets with
/// unused external buffers still decode.
fn resolve_buffer(buffer: &Value, index: usize, binary: Option<&[u8]>) -> Result<Vec<u8>, GlbError> {
    match buffer.get("uri").and_then(Value::as_str) {
        None if index == 0 => binary.map(<[u8]>::to_vec).ok_or_else(|| invalid("buffer 0 has no BIN chunk")),
        None => Err(invalid(&format!("buffer {index} has no data"))),
        Some(uri) => {
            let payload = uri
                .strip_prefix(DATA_URI_PREFIX)
                .and_then(|rest| rest.split_once(BASE64_MARKER))
                .map(|(_, payload)| payload);
            match payload {
                Some(payload) => {
                    decode_base64(payload).ok_or_else(|| invalid(&format!("buffer {index} has invalid base64")))
                }
                None => Err(GlbError::ExternalBuffer(uri.to_owned())),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Primitive helpers
// ---------------------------------------------------------------------------------------------------------------------

/// Decode little-endian components into `target` (one value per slot).
fn decode_components(bytes: &[u8], component_type: u64, target: &mut [f64]) {
    let size = component_size(component_type).unwrap_or(1);
    for (slot, chunk) in target.iter_mut().zip(bytes.chunks_exact(size)) {
        *slot = match component_type {
            BYTE_COMPONENT => f64::from(i8::from_le_bytes([chunk[0]])),
            UNSIGNED_BYTE_COMPONENT => f64::from(chunk[0]),
            SHORT_COMPONENT => f64::from(i16::from_le_bytes([chunk[0], chunk[1]])),
            UNSIGNED_SHORT_COMPONENT => f64::from(u16::from_le_bytes([chunk[0], chunk[1]])),
            UNSIGNED_INT_COMPONENT => f64::from(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])),
            _ => f64::from(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])),
        };
    }
}

/// Bytes per component for a glTF component type.
fn component_size(component_type: u64) -> Option<usize> {
    match component_type {
        BYTE_COMPONENT | UNSIGNED_BYTE_COMPONENT => Some(1),
        SHORT_COMPONENT | UNSIGNED_SHORT_COMPONENT => Some(2),
        UNSIGNED_INT_COMPONENT | FLOAT_COMPONENT => Some(4),
        _ => None,
    }
}

/// Components per element for a glTF accessor type.
fn element_components(kind: &str) -> Option<usize> {
    match kind {
        "SCALAR" => Some(1),
        "VEC2" => Some(2),
        "VEC3" => Some(3),
        "VEC4" | "MAT2" => Some(4),
        "MAT3" => Some(9),
        "MAT4" => Some(16),
        _ => None,
    }
}

/// A nonnegative JSON integer as an index.
fn as_index(value: Option<&Value>) -> Option<usize> {
    value.and_then(Value::as_u64).and_then(|index| usize::try_from(index).ok())
}

/// Read a little-endian `u32`.
fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let word: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(word))
}

/// Decode standard or URL-safe base64, ignoring whitespace and stopping at padding.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity((text.len() * 3) / 4);
    let mut accumulator: u32 = 0;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((accumulator >> bits) as u8);
            accumulator &= (1 << bits) - 1;
        }
    }
    Some(output)
}

/// Accessor-data error with a description.
fn invalid(message: &str) -> GlbError {
    GlbError::InvalidAccessor(message.to_owned())
}

#[cfg(test)]
#[path = "glb_mesh_loader.test.rs"]
mod tests;
