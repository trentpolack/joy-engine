// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Realizes editable geometry and instance collections into the output builder.
//!
//! Realization reads each vertex's shading from the most specific domain (corner, then face, then point) and splits
//! shared points only where corners or faces carry shading. Custom point and corner attributes become per-vertex
//! columns; custom face attributes become per-triangle columns.

use super::geometry::{
    Column,
    Domain,
    DomainKind,
    Element,
    Geometry,
    Instances,
    MATERIAL_ATTRIBUTE,
    POSITION_ATTRIBUTE,
    STANDARD_ATTRIBUTES,
};
use super::store::rotate;
use crate::{
    builder::{
        Builder,
        DEFAULT_TINT,
        OutputVertex,
    },
    geometry_data::AttributeValue,
    math::hypot,
};

/// Shading attributes whose corner or face values force a vertex split.
const SHADING_ATTRIBUTES: [&str; 3] = ["normal", "uv", "color"];

/// Realize editable geometry into `builder` as authored.
pub fn emit_geometry(geometry: &Geometry, builder: &mut Builder) -> Result<(), String> {
    emit_placed(geometry, builder, &Placement::default())
}

/// Realize every instance of a collection into `builder`.
pub fn emit_instances(collection: &Instances, builder: &mut Builder) -> Result<(), String> {
    let collection_material =
        if collection.material.is_null() { None } else { Some(builder.bind_material(&collection.material)?) };
    let sites = &collection.sites;
    let copied_attributes: Vec<(&str, &Column)> = site_attributes(&sites.points).collect();
    let site_vector = |name: &str, index: usize, fallback: [f64; 3]| -> [f64; 3] {
        sites.points.get(name).map_or(fallback, |column| {
            let components = column.numbers(index);
            [components[0], components[1], components[2]]
        })
    };
    for index in 0..sites.count(DomainKind::Points) {
        let scale = site_vector("scale", index, [1.0, 1.0, 1.0]);
        if scale.iter().any(|component| component.abs() < 1e-9) {
            return Err("Instance scale must be nonzero.".into());
        }
        let placement = Placement {
            transform: Some(Transform {
                position: sites.position(index),
                rotation: site_vector("rotation", index, [0.0, 0.0, 0.0]),
                scale,
            }),
            tint: sites.points.get("color").map(|color| color.numbers(index).to_vec()),
            attributes: copied_attributes
                .iter()
                .map(|(name, column)| ((*name).to_owned(), column.get(index)))
                .collect(),
            material: collection_material,
        };
        emit_placed(&collection.source, builder, &placement)?;
    }
    Ok(())
}

/// How one realization of a geometry is placed: an instance transform, a site tint and attributes, and an
/// overriding collection material. The default places geometry as authored.
#[derive(Default)]
struct Placement {
    transform: Option<Transform>,
    tint: Option<Vec<f64>>,
    attributes: Vec<(String, Element)>,
    material: Option<u32>,
}

struct Transform {
    position: [f64; 3],
    rotation: [f64; 3],
    scale: [f64; 3],
}

impl Transform {
    /// Whether the scale mirrors geometry, which reverses face winding.
    fn mirrors(&self) -> bool {
        (self.scale[0] * self.scale[1] * self.scale[2]) < 0.0
    }
}

/// The shading columns realization reads, resolved once per geometry.
struct Shading<'a> {
    corners: Option<&'a [f64]>,
    faces: Option<&'a [f64]>,
    points: Option<&'a [f64]>,
    size: usize,
}

impl<'a> Shading<'a> {
    fn new(geometry: &'a Geometry, name: &str) -> Self {
        let values = |domain: &'a Domain| -> Option<&'a [f64]> {
            match domain.get(name) {
                Some(Column::Number {
                    values,
                    ..
                }) => Some(values.as_slice()),
                _ => None,
            }
        };
        Self {
            corners: values(&geometry.corners),
            faces: values(&geometry.face_attributes),
            points: values(&geometry.points),
            size: if name == "color" { 4 } else { 3 },
        }
    }

    /// The value for one vertex, from the most specific domain storing it.
    fn read(&self, point: usize, corner: Option<usize>, face: Option<usize>) -> Option<&'a [f64]> {
        let element = |values: &'a [f64], index: usize| &values[(index * self.size)..((index + 1) * self.size)];
        if let (Some(values), Some(corner)) = (self.corners, corner) {
            return Some(element(values, corner));
        }
        if let (Some(values), Some(face)) = (self.faces, face) {
            return Some(element(values, face));
        }
        self.points.map(|values| element(values, point))
    }
}

struct Emission<'a> {
    geometry: &'a Geometry,
    placement: &'a Placement,
    normals: Shading<'a>,
    uvs: Shading<'a>,
    colors: Shading<'a>,
    point_attributes: Vec<(&'a str, &'a Column)>,
    corner_attributes: Vec<(&'a str, &'a Column)>,
}

impl Emission<'_> {
    fn vertex(
        &self,
        builder: &mut Builder,
        point: usize,
        corner: Option<usize>,
        face: Option<usize>,
    ) -> Result<u32, String> {
        let mut position = self.geometry.position(point);
        let mut normal = self.normals.read(point, corner, face).map_or([0.0; 3], |n| [n[0], n[1], n[2]]);
        let largest = normal.iter().map(|component| component.abs()).fold(0.0, f64::max);
        if largest != 0.0 {
            for component in &mut normal {
                *component /= largest;
            }
        }
        let length = hypot(&normal);
        if length != 0.0 {
            for component in &mut normal {
                *component /= length;
            }
        }
        if let Some(transform) = &self.placement.transform {
            let scaled: Vec<f64> = position.iter().zip(&transform.scale).map(|(a, b)| a * b).collect();
            let rotated = rotate(&scaled, &transform.rotation);
            position = [
                rotated[0] + transform.position[0],
                rotated[1] + transform.position[1],
                rotated[2] + transform.position[2],
            ];
            let scaled: Vec<f64> = normal.iter().zip(&transform.scale).map(|(a, b)| a / b).collect();
            let rotated = rotate(&scaled, &transform.rotation);
            let length = hypot(&rotated);
            for (axis, component) in rotated.iter().enumerate() {
                normal[axis] = if length != 0.0 { component / length } else { 0.0 };
            }
        }

        // Explicit colors multiply (a site tint over the part's own color); only a part without any uses the tint.
        let own = self.colors.read(point, corner, face);
        let mut color = DEFAULT_TINT;
        match (&self.placement.tint, own) {
            (Some(tint), Some(own)) => {
                for channel in 0..4 {
                    color[channel] = tint[channel] * own[channel];
                }
            }
            (Some(tint), None) => color.copy_from_slice(&tint[..4]),
            (None, Some(own)) => color.copy_from_slice(own),
            (None, None) => {}
        }
        let uv = self.uvs.read(point, corner, face).map_or([0.0; 2], |uv| [uv[0], uv[1]]);

        // Corner values override point values of the same name, and instance site values override both.
        let mut custom: Vec<(&str, AttributeValue)> = Vec::new();
        for (name, column) in &self.point_attributes {
            custom.push((name, column.value(point)));
        }
        if let Some(corner) = corner {
            for (name, column) in &self.corner_attributes {
                set_named(&mut custom, name, column.value(corner));
            }
        }
        for (name, value) in &self.placement.attributes {
            set_named(&mut custom, name, value.value());
        }

        builder.vertex(
            &OutputVertex {
                position,
                normal,
                uv,
                color,
            },
            &custom,
        )
    }
}

fn emit_placed(geometry: &Geometry, builder: &mut Builder, placement: &Placement) -> Result<(), String> {
    let emission = Emission {
        geometry,
        placement,
        normals: Shading::new(geometry, "normal"),
        uvs: Shading::new(geometry, "uv"),
        colors: Shading::new(geometry, "color"),
        point_attributes: custom_attributes(&geometry.points).collect(),
        corner_attributes: custom_attributes(&geometry.corners).collect(),
    };
    let face_attributes: Vec<(&str, &Column)> = custom_attributes(&geometry.face_attributes).collect();

    // Bind each material the faces use once; a collection material overrides them all.
    let mut bound_materials = Vec::new();
    if placement.material.is_none() && geometry.face_attributes.contains(MATERIAL_ATTRIBUTE) {
        for material in &geometry.materials {
            bound_materials.push(builder.bind_material(material)?);
        }
    }
    let face_materials = match geometry.face_attributes.get(MATERIAL_ATTRIBUTE) {
        Some(Column::Material(indices)) => Some(indices),
        _ => None,
    };
    let face_material = |face: usize| -> u32 {
        if let Some(material) = placement.material {
            return material;
        }
        face_materials.and_then(|indices| indices[face]).map_or(0, |index| bound_materials[index])
    };

    let point_count = geometry.count(DomainKind::Points);
    let mut used = vec![false; point_count];
    for face in &geometry.faces {
        for point in face {
            used[*point] = true;
        }
    }
    let split = [&geometry.corners, &geometry.face_attributes]
        .iter()
        .any(|domain| SHADING_ATTRIBUTES.iter().any(|name| domain.contains(name)))
        || !emission.corner_attributes.is_empty();
    let first = (builder.data.positions.len() / 3) as u32;
    if !split {
        for point in 0..point_count {
            emission.vertex(builder, point, None, None)?;
        }
    }
    let mirrors = placement.transform.as_ref().is_some_and(Transform::mirrors);
    for (face_index, face) in geometry.faces.iter().enumerate() {
        let mut indices = [0; 3];
        for (corner, point) in face.iter().enumerate() {
            indices[corner] = if split {
                emission.vertex(builder, *point, Some((face_index * 3) + corner), Some(face_index))?
            } else {
                first + *point as u32
            };
        }
        if mirrors {
            indices.swap(1, 2);
        }
        let values: Vec<(&str, AttributeValue)> =
            face_attributes.iter().map(|(name, column)| (*name, column.value(face_index))).collect();
        builder.face(indices, face_material(face_index), &values)?;
    }
    for point in (0..point_count).filter(|point| !used[*point]) {
        let index = if split { emission.vertex(builder, point, None, None)? } else { first + point as u32 };
        builder.point(index, placement.material.unwrap_or(0));
    }
    Ok(())
}

/// A domain's custom attributes: everything realization does not read as shading, position, or material. Point
/// `rotation` and `scale` are custom here (hosts read them from placements); instance sites consume them instead.
fn custom_attributes(domain: &Domain) -> impl Iterator<Item = (&str, &Column)> {
    domain.iter().filter(|(name, _)| {
        *name != POSITION_ATTRIBUTE && !SHADING_ATTRIBUTES.contains(name) && *name != MATERIAL_ATTRIBUTE
    })
}

/// An instance collection's site attributes copied onto each instance: everything but the site transform and tint.
fn site_attributes(sites: &Domain) -> impl Iterator<Item = (&str, &Column)> {
    sites.iter().filter(|(name, _)| *name != POSITION_ATTRIBUTE && !STANDARD_ATTRIBUTES.contains(name))
}

fn set_named<'a>(values: &mut Vec<(&'a str, AttributeValue<'a>)>, name: &'a str, value: AttributeValue<'a>) {
    match values.iter_mut().find(|(existing, _)| *existing == name) {
        Some(entry) => entry.1 = value,
        None => values.push((name, value)),
    }
}
