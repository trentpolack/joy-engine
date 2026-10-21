// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Output assembly: realized vertices, triangles, isolated points, custom attribute columns, and material binding.
//!
//! The builder holds no drawing state. Every vertex arrives with its own position, normal, UV, and color, and every
//! triangle with its material, so the output depends only on the geometry values a script outputs.

use crate::{
    GeometryData,
    geometry_data::{
        AttributeColumn,
        AttributeValue,
    },
    materials::read_materials,
    value::integer_range,
};
use joy_core::math::deterministic;
use serde_json::{
    Value as Json,
    json,
};
use std::collections::{
    BTreeMap,
    HashMap,
};

/// Most vertices one evaluation may emit.
pub const MAX_VERTICES: usize = 150_000;

/// Most triangles one evaluation may emit.
pub const MAX_TRIANGLES: usize = 180_000;

/// FORM's default vertex color (linear RGBA): the warm orange that geometry without a `color` attribute realizes
/// with, so unstyled shapes read as FORM output rather than as missing data.
pub const DEFAULT_TINT: [f64; 4] = [0.929, 0.424, 0.224, 1.0];

/// One realized vertex. A zero normal asks renderers for the triangle's flat face normal.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputVertex {
    pub position: [f64; 3],
    pub normal: [f64; 3],
    pub uv: [f64; 2],
    /// Linear RGB (HDR allowed) and alpha in 0–1.
    pub color: [f64; 4],
}

/// Owned indexed Y-up output geometry.
pub struct Builder {
    pub data: GeometryData,
    // Palette index of each script material already bound, keyed by its serialized record, so identical materials
    // share one palette entry.
    material_indices: HashMap<String, u32>,
}

impl Builder {
    pub fn new(materials: Option<&Json>) -> Result<Self, String> {
        Ok(Self {
            data: GeometryData {
                materials: read_materials(materials)?,
                ..GeometryData::default()
            },
            material_indices: HashMap::new(),
        })
    }

    /// An empty builder sharing this builder's palette and bound materials, for realizing one named output that is
    /// later copied in with [`Builder::append`].
    pub fn realization(&self) -> Self {
        Self {
            data: GeometryData {
                materials: self.data.materials.clone(),
                ..GeometryData::default()
            },
            material_indices: self.material_indices.clone(),
        }
    }

    /// Emit one vertex with its custom attribute values (numbers, vectors, colors, text, or true/false).
    pub fn vertex(&mut self, vertex: &OutputVertex, attributes: &[(&str, AttributeValue<'_>)]) -> Result<u32, String> {
        if self.data.positions.len() >= MAX_VERTICES * 3 {
            return Err("Vertex limit exceeded (150,000).".into());
        }
        if vertex.position.iter().any(|value| !value.is_finite() || value.abs() > 1e6) {
            return Err("Coordinates must be finite and within ±1,000,000.".into());
        }
        let index = (self.data.positions.len() / 3) as u32;
        push_attributes(&mut self.data.vertex_attributes, index as usize, attributes)?;
        self.data.positions.extend(vertex.position);
        self.data.normals.extend(vertex.normal);
        self.data.uvs.extend(vertex.uv);
        self.data.colors.extend(&vertex.color[..3]);
        self.data.alphas.push(vertex.color[3]);
        Ok(index)
    }

    /// Emit one triangle in palette slot `material`, with its custom face attribute values.
    pub fn face(
        &mut self,
        corners: [u32; 3],
        material: u32,
        attributes: &[(&str, AttributeValue<'_>)],
    ) -> Result<(), String> {
        if self.data.triangles.len() >= MAX_TRIANGLES * 3 {
            return Err("Triangle limit exceeded (180,000).".into());
        }
        let index = self.data.triangles.len() / 3;
        push_attributes(&mut self.data.triangle_attributes, index, attributes)?;
        self.data.triangles.extend(corners);
        self.data.triangle_materials.push(material);
        Ok(())
    }

    /// Draw an emitted vertex as an isolated point in palette slot `material`.
    pub fn point(&mut self, vertex: u32, material: u32) {
        self.data.points.push(vertex);
        self.data.point_materials.push(material);
    }

    /// The palette slot for a material table entry: a script material resource (`{"kind": "material", …}`), added to
    /// the palette once per distinct record, or `{"palette": name}` naming an existing palette material.
    pub fn bind_material(&mut self, material: &Json) -> Result<u32, String> {
        if let Some(name) = material.get("palette").and_then(Json::as_str) {
            return self
                .data
                .materials
                .iter()
                .position(|entry| entry["name"] == name)
                .map(|index| index as u32)
                .ok_or_else(|| format!("Unknown material '{name}'. Add it in Materials or import MaterialX."));
        }
        let record =
            material.get("material").filter(|record| record.is_object()).ok_or("Expected a material value.")?;
        let key = serde_json::to_string(record).map_err(|error| error.to_string())?;
        if let Some(index) = self.material_indices.get(&key) {
            return Ok(*index);
        }
        let mut suffix = self.data.materials.len();
        let name = loop {
            let candidate = format!("ScriptMaterial_{suffix}");
            if self.data.materials.iter().all(|material| material["name"] != candidate) {
                break candidate;
            }
            suffix += 1;
        };
        let mut named = record.clone();
        named["name"] = json!(name);
        let mut materials = self.data.materials.clone();
        materials.push(named);
        let next = read_materials(Some(&json!(materials)))?;
        let index = (next.len() - 1) as u32;
        self.data.materials = next;
        self.material_indices.insert(key, index);
        Ok(index)
    }

    /// Append already realized geometry (a named output's realization), remapping its material slots into this
    /// palette and extending every attribute column.
    pub fn append(&mut self, other: &GeometryData) -> Result<(), String> {
        let vertex_offset = (self.data.positions.len() / 3) as u32;
        if self.data.positions.len() + other.positions.len() > MAX_VERTICES * 3 {
            return Err("Vertex limit exceeded (150,000).".into());
        }
        if self.data.triangles.len() + other.triangles.len() > MAX_TRIANGLES * 3 {
            return Err("Triangle limit exceeded (180,000).".into());
        }
        let mut slots = Vec::with_capacity(other.materials.len());
        for material in &other.materials {
            // Palette names are unique, and a realization starts from this palette, so names identify shared
            // entries; materials the realization bound itself are added by content.
            let slot = match self.data.materials.iter().position(|existing| existing["name"] == material["name"]) {
                Some(index) => index as u32,
                None => {
                    let mut record = material.clone();
                    record.as_object_mut().map(|fields| fields.remove("name"));
                    self.bind_material(&json!({"kind": "material", "material": record}))?
                }
            };
            slots.push(slot);
        }

        let vertex_count = self.data.positions.len() / 3;
        let triangle_count = self.data.triangles.len() / 3;
        merge_columns(
            &mut self.data.vertex_attributes,
            &other.vertex_attributes,
            vertex_count,
            other.positions.len() / 3,
        )?;
        merge_columns(
            &mut self.data.triangle_attributes,
            &other.triangle_attributes,
            triangle_count,
            other.triangles.len() / 3,
        )?;
        self.data.positions.extend(&other.positions);
        self.data.normals.extend(&other.normals);
        self.data.uvs.extend(&other.uvs);
        self.data.colors.extend(&other.colors);
        self.data.alphas.extend(&other.alphas);
        self.data.triangles.extend(other.triangles.iter().map(|index| index + vertex_offset));
        self.data.points.extend(other.points.iter().map(|index| index + vertex_offset));
        self.data.triangle_materials.extend(other.triangle_materials.iter().map(|slot| slots[*slot as usize]));
        self.data.point_materials.extend(other.point_materials.iter().map(|slot| slots[*slot as usize]));
        Ok(())
    }

    /// A centered box: 8 shared corners with zero (flat-shaded) normals, zero UVs, and the default tint.
    pub fn box_geometry(&mut self, width: f64, height: f64, depth: f64) -> Result<(), String> {
        self.mesh_arrays(&box_arrays(width, height, depth)?)
    }

    /// A centered UV sphere with radial normals, seam-duplicated vertices, and the default tint.
    pub fn sphere(&mut self, radius: f64, segments: f64) -> Result<(), String> {
        self.mesh_arrays(&sphere_arrays(radius, segments)?)
    }

    /// Emit primitive arrays with the default tint and the Default material.
    fn mesh_arrays(&mut self, mesh: &MeshArrays) -> Result<(), String> {
        let first = (self.data.positions.len() / 3) as u32;
        for vertex in 0..mesh.positions.len() / 3 {
            let at =
                |values: &[f64], size: usize| -> Vec<f64> { values[(vertex * size)..((vertex + 1) * size)].to_vec() };
            let [position, normal, uv] = [at(&mesh.positions, 3), at(&mesh.normals, 3), at(&mesh.uvs, 2)];
            self.vertex(
                &OutputVertex {
                    position: [position[0], position[1], position[2]],
                    normal: [normal[0], normal[1], normal[2]],
                    uv: [uv[0], uv[1]],
                    color: DEFAULT_TINT,
                },
                &[],
            )?;
        }
        for triangle in mesh.indices.as_chunks::<3>().0 {
            self.face([first + triangle[0], first + triangle[1], first + triangle[2]], 0, &[])?;
        }
        Ok(())
    }
}

/// Flat primitive mesh data: XYZ positions and normals, UV pairs, and triangle indices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshArrays {
    pub positions: Vec<f64>,
    pub normals: Vec<f64>,
    pub uvs: Vec<f64>,
    pub indices: Vec<u32>,
}

/// A centered box: 8 shared corners with zero (flat-shaded) normals and zero UVs.
pub fn box_arrays(width: f64, height: f64, depth: f64) -> Result<MeshArrays, String> {
    if width <= 0.0 || height <= 0.0 || depth <= 0.0 {
        return Err("Box sizes must be positive.".into());
    }
    let mut mesh = MeshArrays::default();
    for [dx, dy, dz] in BOX_CORNERS {
        let position = [dx * width / 2.0, dy * height / 2.0, dz * depth / 2.0];
        if position.iter().any(|value| !value.is_finite() || value.abs() > 1e6) {
            return Err("Coordinates must be finite and within ±1,000,000.".into());
        }
        mesh.positions.extend(position);
        mesh.normals.extend([0.0; 3]);
        mesh.uvs.extend([0.0; 2]);
    }
    mesh.indices.extend(BOX_TRIANGLES.iter().flatten());
    Ok(mesh)
}

/// A centered UV sphere with radial normals and seam-duplicated vertices: `ceil(segments / 2)` rows of
/// `segments + 1` columns; pole rows emit one triangle per column.
pub fn sphere_arrays(radius: f64, segments: f64) -> Result<MeshArrays, String> {
    let segments = integer_range(segments, 3.0, 96.0)?;
    if radius <= 0.0 {
        return Err("Sphere radius must be positive.".into());
    }
    let mut mesh = MeshArrays::default();
    let rows = segments.div_ceil(2);
    for row in 0..=rows {
        let latitude = std::f64::consts::PI * row as f64 / rows as f64;
        for column in 0..=segments {
            let angle = std::f64::consts::PI * 2.0 * column as f64 / segments as f64;
            let normal = [
                deterministic::sin(latitude) * deterministic::cos(angle),
                deterministic::cos(latitude),
                deterministic::sin(latitude) * deterministic::sin(angle),
            ];
            // Adding to a zero origin turns a negative-zero coordinate into zero, as sphere positions always have.
            let origin = 0.0;
            let position = [origin + radius * normal[0], origin + radius * normal[1], origin + radius * normal[2]];
            if position.iter().any(|value| !value.is_finite() || value.abs() > 1e6) {
                return Err("Coordinates must be finite and within ±1,000,000.".into());
            }
            mesh.positions.extend(position);
            mesh.normals.extend(normal);
            mesh.uvs.extend([column as f64 / segments as f64, row as f64 / rows as f64]);
        }
    }
    for row in 0..rows {
        for column in 0..segments {
            let a = (row * (segments + 1) + column) as u32;
            let b = a + segments as u32 + 1;
            if row > 0 {
                mesh.indices.extend([a, a + 1, b]);
            }
            if row < rows - 1 {
                mesh.indices.extend([a + 1, b + 1, b]);
            }
        }
    }
    Ok(mesh)
}

/// Unit box corners, in emission order.
const BOX_CORNERS: [[f64; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

/// Box triangles over [`BOX_CORNERS`], wound outward.
const BOX_TRIANGLES: [[u32; 3]; 12] = [
    [0, 2, 1],
    [0, 3, 2],
    [4, 5, 6],
    [4, 6, 7],
    [0, 1, 5],
    [0, 5, 4],
    [3, 7, 6],
    [3, 6, 2],
    [0, 4, 7],
    [0, 7, 3],
    [1, 2, 6],
    [1, 6, 5],
];

/// Extend `columns` (holding `length` elements) with `other`'s `added` elements, defaulting whatever either side lacks.
fn merge_columns(
    columns: &mut BTreeMap<String, AttributeColumn>,
    other: &BTreeMap<String, AttributeColumn>,
    length: usize,
    added: usize,
) -> Result<(), String> {
    for (name, column) in other {
        if !columns.contains_key(name) {
            columns.insert(name.clone(), column.defaults_like(length));
        }
    }
    for (name, column) in columns.iter_mut() {
        match other.get(name) {
            Some(values) => {
                for index in 0..added {
                    column.push(values.value_at(index)).map_err(|expected| {
                        format!(
                            "Output attribute '{name}' holds {expected} in one output and a different type in another."
                        )
                    })?;
                }
            }
            None => {
                for _ in 0..added {
                    column.push_default();
                }
            }
        }
    }
    Ok(())
}

/// Append one element's values to `columns`, creating a column (backfilled with defaults) for a new name and filling
/// columns this element lacks.
fn push_attributes(
    columns: &mut BTreeMap<String, AttributeColumn>,
    element: usize,
    attributes: &[(&str, AttributeValue<'_>)],
) -> Result<(), String> {
    for (name, value) in attributes {
        if !columns.contains_key(*name) {
            columns.insert((*name).to_owned(), AttributeColumn::for_value(*value, element));
        }
        columns.get_mut(*name).unwrap().push(*value).map_err(|expected| {
            format!("Output attribute '{name}' holds {expected} in one part and a different type in another.")
        })?;
    }
    for column in columns.values_mut() {
        if column.len() == element {
            column.push_default();
        }
    }
    Ok(())
}
