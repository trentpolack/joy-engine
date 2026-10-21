// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Geometry edits, attribute storage rules, topology edits, transforms, and allocation limits.

use super::geometry::{
    Column,
    DomainKind,
    Element,
    Geometry,
    MATERIAL_ATTRIBUTE,
    POSITION_ATTRIBUTE,
    STANDARD_ATTRIBUTES,
};
use crate::{
    builder::MeshArrays,
    math::hypot,
    value::{
        SharedBudget,
        integer_range,
    },
};
use joy_core::math::deterministic;
use serde_json::Value as Json;

/// Longest text attribute value, in characters.
const TEXT_ATTRIBUTE_LIMIT: usize = 256;

/// Most distinct materials one geometry's faces may reference.
const MATERIAL_TABLE_LIMIT: usize = 128;

/// Most attributes one domain may declare.
const ATTRIBUTE_LIMIT: usize = 64;

// Procedural allocation limits per evaluation.
const POINT_LIMIT: usize = 150_000;
const FACE_LIMIT: usize = 180_000;
const COMPONENT_LIMIT: usize = 2_000_000;

/// One evaluation's procedural allocations, counted against its limits.
pub struct Store {
    pub budget: SharedBudget,
    points: usize,
    faces: usize,
    components: usize,
}

impl Store {
    pub fn new(budget: SharedBudget) -> Self {
        Self {
            budget,
            points: 0,
            faces: 0,
            components: 0,
        }
    }

    pub fn reserve(&mut self, points: usize, faces: usize, components: usize) -> Result<(), String> {
        if points > POINT_LIMIT.saturating_sub(self.points)
            || faces > FACE_LIMIT.saturating_sub(self.faces)
            || components > COMPONENT_LIMIT.saturating_sub(self.components)
        {
            return Err("Procedural geometry allocation limit exceeded (150,000 points, 180,000 faces, 2,000,000 attribute components).".into());
        }
        self.points += points;
        self.faces += faces;
        self.components += components;
        Ok(())
    }

    pub fn add_point(&mut self, geometry: &mut Geometry, position: &[f64]) -> Result<usize, String> {
        validate_position(position)?;
        self.reserve(1, 0, geometry.points.iter().map(|(_, column)| column.size()).sum())?;
        let index = geometry.count(DomainKind::Points);
        for column in geometry.points.columns_mut() {
            column.push_defaults(1);
        }
        geometry
            .points
            .get_mut(POSITION_ATTRIBUTE)
            .expect("geometry always has positions")
            .set(index, &Element::Numbers(position.to_vec()));
        Ok(index)
    }

    pub fn add_face(&mut self, geometry: &mut Geometry, points: &[usize]) -> Result<usize, String> {
        if points.len() != 3 {
            return Err("Faces require three point indices.".into());
        }
        let point_count = geometry.count(DomainKind::Points);
        for point in points {
            integer_range(*point as f64, 0.0, point_count as f64 - 1.0)?;
        }
        let components = geometry.face_attributes.iter().map(|(_, column)| column.size()).sum::<usize>()
            + geometry.corners.iter().map(|(_, column)| column.size() * 3).sum::<usize>();
        self.reserve(0, 1, components)?;
        geometry.faces.push([points[0], points[1], points[2]]);
        for column in geometry.face_attributes.columns_mut() {
            column.push_defaults(1);
        }
        for column in geometry.corners.columns_mut() {
            column.push_defaults(3);
        }
        Ok(geometry.faces.len() - 1)
    }

    pub fn add_attribute(
        &mut self,
        geometry: &mut Geometry,
        domain: DomainKind,
        name: &str,
        value: &Element,
    ) -> Result<(), String> {
        validate_attribute(domain, name, value)?;
        let length = geometry.count(domain);
        let attributes = geometry.domain_mut(domain);
        if attributes.contains(name) {
            return Err(format!("{} attribute '{name}' already exists.", domain.name()));
        }
        if attributes.len() >= ATTRIBUTE_LIMIT {
            return Err("Attribute limit exceeded (64 per domain).".into());
        }
        self.reserve(0, 0, length * Column::filled(value, 0).size())?;
        attributes.insert(name, Column::filled(value, length));
        Ok(())
    }

    /// Declare the faces `material` attribute: every face starts without a material, so realization uses the
    /// instance collection's material or the palette's Default.
    pub fn add_material_attribute(&mut self, geometry: &mut Geometry) -> Result<(), String> {
        if geometry.face_attributes.contains(MATERIAL_ATTRIBUTE) {
            return Ok(());
        }
        let length = geometry.count(DomainKind::Faces);
        self.reserve(0, 0, length)?;
        geometry.face_attributes.insert(MATERIAL_ATTRIBUTE, Column::Material(vec![None; length]));
        Ok(())
    }

    /// Assign `material` (a material resource record, `{"palette": name}`, or `null` for none) to one face, sharing
    /// table entries between faces that use identical materials.
    pub fn set_material(&mut self, geometry: &mut Geometry, face: usize, material: &Json) -> Result<(), String> {
        integer_range(face as f64, 0.0, geometry.count(DomainKind::Faces) as f64 - 1.0)?;
        self.add_material_attribute(geometry)?;
        let index = if material.is_null() { None } else { Some(intern_material(geometry, material)?) };
        geometry
            .face_attributes
            .get_mut(MATERIAL_ATTRIBUTE)
            .expect("the material attribute was just declared")
            .set(face, &Element::Material(index));
        Ok(())
    }

    /// Remove a declared attribute; positions are required and cannot be removed.
    pub fn remove_attribute(&mut self, geometry: &mut Geometry, domain: DomainKind, name: &str) -> Result<(), String> {
        if domain == DomainKind::Points && name == POSITION_ATTRIBUTE {
            return Err("Point positions cannot be removed.".into());
        }
        if geometry.domain_mut(domain).remove(name).is_none() {
            return Err(format!("Unknown {} attribute '{name}'.", domain.name()));
        }
        if domain == DomainKind::Faces && name == MATERIAL_ATTRIBUTE {
            geometry.materials.clear();
        }
        Ok(())
    }

    /// Set one element's attribute, declaring the attribute (default elsewhere) when it is new. Writing positions
    /// clears authored normals unless `defer_normals` (wrangles reconcile normals when they end).
    pub fn set(
        &mut self,
        geometry: &mut Geometry,
        domain: DomainKind,
        name: &str,
        index: usize,
        value: &Element,
        defer_normals: bool,
    ) -> Result<(), String> {
        let length = geometry.count(domain);
        integer_range(index as f64, 0.0, length as f64 - 1.0)?;
        validate_attribute(domain, name, value)?;
        if !geometry.domain(domain).contains(name) {
            if geometry.domain(domain).len() >= ATTRIBUTE_LIMIT {
                return Err("Attribute limit exceeded (64 per domain).".into());
            }
            let mut column = Column::filled(value, 0);
            self.reserve(0, 0, length * column.size())?;
            column.push_defaults(length);
            geometry.domain_mut(domain).insert(name, column);
        }
        let column = geometry.domain_mut(domain).get_mut(name).expect("the attribute exists");
        if column.kind() != value.kind() {
            return Err(format!("Attribute '{name}' must keep its type ({}).", column.kind().description()));
        }
        column.set(index, value);
        if domain == DomainKind::Points && name == POSITION_ATTRIBUTE && !defer_normals {
            clear_normals(geometry);
        }
        Ok(())
    }

    /// An independent copy, counted against the allocation limits.
    pub fn copy(&mut self, geometry: &Geometry) -> Result<Geometry, String> {
        self.reserve(geometry.count(DomainKind::Points), geometry.count(DomainKind::Faces), geometry.components())?;
        Ok(geometry.clone())
    }

    /// Editable geometry from a mesh resource. Vertices no triangle uses are left out: a mesh has no isolated points,
    /// and realizing them would draw stray points (a UV sphere's unused seam poles, for example).
    pub fn import_mesh(&mut self, mesh: &Json) -> Result<Geometry, String> {
        self.import_arrays(&crate::resources::mesh_arrays(mesh))
    }

    /// Editable geometry from primitive arrays: point positions, normals, and UVs, and faces over the vertices
    /// triangles use.
    pub fn import_arrays(&mut self, mesh: &MeshArrays) -> Result<Geometry, String> {
        let vertex_count = mesh.positions.len() / 3;
        let mut used = vec![false; vertex_count];
        for index in &mesh.indices {
            used[*index as usize] = true;
        }
        let kept = used.iter().filter(|used| **used).count();
        self.reserve(kept, mesh.indices.len() / 3, kept * 9)?;

        let mut remap = vec![0usize; vertex_count];
        let mut positions = Vec::with_capacity(kept * 3);
        let mut normals = Vec::with_capacity(kept * 3);
        let mut uvs = Vec::with_capacity(kept * 3);
        for vertex in (0..vertex_count).filter(|vertex| used[*vertex]) {
            remap[vertex] = positions.len() / 3;
            positions.extend_from_slice(&mesh.positions[(vertex * 3)..((vertex + 1) * 3)]);
            normals.extend_from_slice(&mesh.normals[(vertex * 3)..((vertex + 1) * 3)]);
            uvs.extend([mesh.uvs[vertex * 2], mesh.uvs[(vertex * 2) + 1], 0.0]);
        }
        let mut geometry = Geometry::default();
        geometry.points.get_mut(POSITION_ATTRIBUTE).expect("geometry always has positions").extend(&Column::Number {
            size: 3,
            values: positions,
        });
        geometry.points.insert(
            "normal",
            Column::Number {
                size: 3,
                values: normals,
            },
        );
        geometry.points.insert(
            "uv",
            Column::Number {
                size: 3,
                values: uvs,
            },
        );
        geometry.faces = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| triangle.map(|index| remap[index as usize]))
            .collect();
        Ok(geometry)
    }

    pub fn grid(&mut self, columns: f64, rows: f64, width: f64, depth: f64) -> Result<Geometry, String> {
        let columns = integer_range(columns, 1.0, 256.0)?;
        let rows = integer_range(rows, 1.0, 256.0)?;
        if !width.is_finite() || !depth.is_finite() || width <= 0.0 || depth <= 0.0 {
            return Err("Grid dimensions must be positive finite numbers.".into());
        }
        let point_count = (columns + 1) * (rows + 1);
        let face_count = columns * rows * 2;
        self.reserve(point_count, face_count, point_count * 6)?;
        let mut positions = Vec::with_capacity(point_count * 3);
        let mut uvs = Vec::with_capacity(point_count * 3);
        for z in 0..=rows {
            for x in 0..=columns {
                let u = x as f64 / columns as f64;
                let v = z as f64 / rows as f64;
                positions.extend([(u - 0.5) * width, 0.0, (v - 0.5) * depth]);
                uvs.extend([u, v, 0.0]);
            }
        }
        let mut geometry = Geometry::default();
        geometry.points.get_mut(POSITION_ATTRIBUTE).expect("geometry always has positions").extend(&Column::Number {
            size: 3,
            values: positions,
        });
        geometry.points.insert(
            "uv",
            Column::Number {
                size: 3,
                values: uvs,
            },
        );
        for z in 0..rows {
            for x in 0..columns {
                let a = z * (columns + 1) + x;
                let b = a + columns + 1;
                geometry.faces.push([a, b, a + 1]);
                geometry.faces.push([a + 1, b, b + 1]);
            }
        }
        Ok(geometry)
    }
}

/// Read one element's attribute value.
pub fn attribute_at(geometry: &Geometry, domain: DomainKind, name: &str, index: f64) -> Result<Element, String> {
    let index = integer_range(index, 0.0, geometry.count(domain) as f64 - 1.0)?;
    let column =
        geometry.domain(domain).get(name).ok_or_else(|| format!("Unknown {} attribute '{name}'.", domain.name()))?;
    Ok(column.get(index))
}

/// Remove authored normals from every domain, as moving points invalidates them.
pub fn clear_normals(geometry: &mut Geometry) {
    for domain in DomainKind::ALL {
        geometry.domain_mut(domain).remove("normal");
    }
}

pub fn remove_face(geometry: &mut Geometry, index: usize) {
    geometry.faces.remove(index);
    for column in geometry.face_attributes.columns_mut() {
        column.remove_range(index, index + 1);
    }
    for column in geometry.corners.columns_mut() {
        column.remove_range(index * 3, (index * 3) + 3);
    }
}

pub fn remove_point(geometry: &mut Geometry, index: usize) {
    for face in (0..geometry.faces.len()).rev() {
        if geometry.faces[face].contains(&index) {
            remove_face(geometry, face);
        } else {
            for point in &mut geometry.faces[face] {
                if *point > index {
                    *point -= 1;
                }
            }
        }
    }
    for column in geometry.points.columns_mut() {
        column.remove_range(index, index + 1);
    }
}

/// Translate, rotate (XYZ Euler radians), or scale every point in place. Rotation and scale also transform authored
/// normals (scale by its inverse, then renormalized), and a mirroring scale reverses face winding.
pub fn transform(geometry: &mut Geometry, operation: &str, value: &[f64]) -> Result<(), String> {
    if operation == "scale" && value.contains(&0.0) {
        return Err("scale components must be nonzero.".into());
    }
    if let Some(Column::Number {
        values,
        ..
    }) = geometry.points.get_mut(POSITION_ATTRIBUTE)
    {
        for position in values.as_chunks_mut::<3>().0 {
            let result = match operation {
                "translate" => [position[0] + value[0], position[1] + value[1], position[2] + value[2]],
                "rotate" => {
                    let rotated = rotate(position, value);
                    [rotated[0], rotated[1], rotated[2]]
                }
                _ => [position[0] * value[0], position[1] * value[1], position[2] * value[2]],
            };
            validate_position(&result)?;
            *position = result;
        }
    }
    if operation == "translate" {
        return Ok(());
    }
    let minimum_scale =
        if operation == "scale" { value.iter().map(|v| v.abs()).fold(f64::INFINITY, f64::min) } else { 1.0 };
    for domain in DomainKind::ALL {
        let Some(Column::Number {
            values,
            ..
        }) = geometry.domain_mut(domain).get_mut("normal")
        else {
            continue;
        };
        for normal in values.as_chunks_mut::<3>().0 {
            let transformed = if operation == "rotate" {
                rotate(normal, value)
            } else {
                normal.iter().zip(value).map(|(component, scale)| component * minimum_scale / scale).collect()
            };
            if transformed.iter().any(|component| !component.is_finite()) {
                return Err("Transformed normal components must be finite.".into());
            }
            let length = hypot(&transformed);
            if !length.is_finite() {
                return Err("Transformed normal length must be finite.".into());
            }
            for (axis, component) in transformed.iter().enumerate() {
                normal[axis] = if length > 0.0 { component / length } else { 0.0 };
            }
        }
    }
    if operation == "scale" && value[0] * value[1] * value[2] < 0.0 {
        for face in &mut geometry.faces {
            face.swap(1, 2);
        }
        let face_count = geometry.faces.len();
        for column in geometry.corners.columns_mut() {
            for face in 0..face_count {
                column.swap((face * 3) + 1, (face * 3) + 2);
            }
        }
    }
    Ok(())
}

pub fn rotate(value: &[f64], angles: &[f64]) -> Vec<f64> {
    let [x, y, z] = [value[0], value[1], value[2]];
    let [rx, ry, rz] = [angles[0], angles[1], angles[2]];
    let y1 = y * deterministic::cos(rx) - z * deterministic::sin(rx);
    let z1 = y * deterministic::sin(rx) + z * deterministic::cos(rx);
    let x2 = x * deterministic::cos(ry) + z1 * deterministic::sin(ry);
    let z2 = -x * deterministic::sin(ry) + z1 * deterministic::cos(ry);
    vec![
        x2 * deterministic::cos(rz) - y1 * deterministic::sin(rz),
        x2 * deterministic::sin(rz) + y1 * deterministic::cos(rz),
        z2,
    ]
}

/// Add `material` to the geometry's material table unless an identical entry exists, returning its index.
pub fn intern_material(geometry: &mut Geometry, material: &Json) -> Result<usize, String> {
    if let Some(index) = geometry.materials.iter().position(|entry| entry == material) {
        return Ok(index);
    }
    if geometry.materials.len() >= MATERIAL_TABLE_LIMIT {
        return Err(format!("Material limit exceeded ({MATERIAL_TABLE_LIMIT} per geometry)."));
    }
    geometry.materials.push(material.clone());
    Ok(geometry.materials.len() - 1)
}

/// Attribute names are identifiers of at most 64 characters; JavaScript prototype keys are reserved.
pub fn validate_attribute_name(name: &str) -> Result<(), String> {
    let bytes = name.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 64
        || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        || bytes.iter().any(|byte| !byte.is_ascii_alphanumeric() && *byte != b'_')
        || ["__proto__", "constructor", "prototype"].contains(&name)
    {
        return Err("Attribute names require 1–64 identifier characters.".into());
    }
    Ok(())
}

fn validate_attribute(domain: DomainKind, name: &str, value: &Element) -> Result<(), String> {
    validate_attribute_name(name)?;
    match value {
        Element::Material(_) => {
            Err("Assign materials with material(geometry, material) or 'face.material = …' in a faces wrangle.".into())
        }
        _ if name == MATERIAL_ATTRIBUTE => {
            Err("Assign materials with material(geometry, material) or 'face.material = …' in a faces wrangle.".into())
        }
        Element::Text(text) => {
            if STANDARD_ATTRIBUTES.contains(&name) {
                return Err(format!("Attribute '{name}' holds numbers, not text."));
            }
            if text.chars().count() > TEXT_ATTRIBUTE_LIMIT {
                return Err(format!("Text attributes hold at most {TEXT_ATTRIBUTE_LIMIT} characters."));
            }
            Ok(())
        }
        Element::Bool(_) => {
            if STANDARD_ATTRIBUTES.contains(&name) {
                return Err(format!("Attribute '{name}' holds numbers, not true/false."));
            }
            Ok(())
        }
        Element::Numbers(values) => validate_numeric_attribute(domain, name, values),
    }
}

fn validate_numeric_attribute(domain: DomainKind, name: &str, values: &[f64]) -> Result<(), String> {
    if ![1, 3, 4].contains(&values.len()) || values.iter().any(|value| !value.is_finite()) {
        return Err("Attributes require finite numbers, XYZ vectors, RGBA colors, text, or true/false.".into());
    }
    let size = if ["position", "normal", "uv", "rotation", "scale"].contains(&name) {
        3
    } else if name == "color" {
        4
    } else {
        values.len()
    };
    if size != values.len() || (name == POSITION_ATTRIBUTE && domain != DomainKind::Points) {
        return Err(format!(
            "Attribute '{name}' requires {size} components{}.",
            if name == POSITION_ATTRIBUTE { " on points" } else { "" }
        ));
    }
    if name == POSITION_ATTRIBUTE && values.iter().any(|v| v.abs() > 1e6) {
        return Err("Coordinates must be within ±1,000,000.".into());
    }
    if name == "color" && !valid_color(values) {
        return Err("Color attributes require finite, nonnegative RGB and alpha in 0–1.".into());
    }
    Ok(())
}

/// Whether color attribute components are finite, nonnegative RGB (HDR allowed) with alpha, when present, in 0–1.
fn valid_color(values: &[f64]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(index, channel)| channel.is_finite() && *channel >= 0.0 && (index != 3 || *channel <= 1.0))
}

fn validate_position(position: &[f64]) -> Result<(), String> {
    if position.len() != 3 || position.iter().any(|v| !v.is_finite() || v.abs() > 1e6) {
        Err("Coordinates must be finite and within ±1,000,000.".into())
    } else {
        Ok(())
    }
}
