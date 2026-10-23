// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Combining editable geometry: appending one geometry to another and realizing instance collections.
//!
//! Appending keeps both inputs' attributes. A custom attribute missing from one side is filled with its default, and
//! a shading attribute (`normal`, `uv`, or `color`) stored in different domains is moved to face corners first, so
//! neither input changes appearance. Material tables are merged and face indices remapped.

use super::geometry::{
    Column,
    DomainKind,
    Element,
    Geometry,
    Instances,
    MATERIAL_ATTRIBUTE,
    POSITION_ATTRIBUTE,
    STANDARD_ATTRIBUTES,
};
use super::store::{
    self,
    Store,
};
use crate::builder::DEFAULT_TINT;

/// Shading attributes realization reads from the most specific domain that stores them.
const SHADING_ATTRIBUTES: [&str; 3] = ["normal", "uv", "color"];

/// Append `source` to `target` in place: points, faces, attributes, and materials.
pub fn append(store: &mut Store, target: &mut Geometry, mut source: Geometry) -> Result<(), String> {
    for name in SHADING_ATTRIBUTES {
        reconcile_shading_domain(target, &mut source, name);
    }
    for domain in DomainKind::ALL {
        reconcile_custom_attributes(target, &source, domain)?;
    }

    // Reserve before mutating, so a failed append leaves the target's topology unchanged.
    store.reserve(source.count(DomainKind::Points), source.count(DomainKind::Faces), source.components())?;
    let point_offset = target.count(DomainKind::Points);
    let material_remap = merge_material_tables(target, &source)?;
    for domain in DomainKind::ALL {
        let length = source.count(domain);
        for name in target.domain(domain).names() {
            let column = target.domain_mut(domain).get_mut(&name).expect("the attribute exists");
            match source.domain(domain).get(&name) {
                Some(Column::Material(indices)) => {
                    let remapped: Vec<Option<usize>> =
                        indices.iter().map(|index| index.map(|index| material_remap[index])).collect();
                    column.extend(&Column::Material(remapped));
                }
                Some(other) => column.extend(other),
                None => {
                    let fill = fill_element(&name, column);
                    for _ in 0..length {
                        column.push(&fill);
                    }
                }
            }
        }
    }
    target.faces.extend(source.faces.iter().map(|face| face.map(|point| point + point_offset)));
    Ok(())
}

/// Copy each instance's source into one geometry: scaled, rotated, translated, tinted by the site color, tagged with
/// the site's custom attributes, and assigned the collection's material.
pub fn realize(store: &mut Store, collection: &Instances) -> Result<Geometry, String> {
    let mut result = Geometry::default();
    let sites = &collection.sites;
    for site in 0..sites.count(DomainKind::Points) {
        let mut copy = store.copy(&collection.source)?;
        let read = |name: &str, fallback: [f64; 3]| -> Vec<f64> {
            sites.points.get(name).map_or_else(|| fallback.to_vec(), |column| column.numbers(site).to_vec())
        };
        let scale = read("scale", [1.0, 1.0, 1.0]);
        if scale.iter().any(|component| component.abs() < 1e-9) {
            return Err("Instance scale must be nonzero.".into());
        }
        store::transform(&mut copy, "scale", &scale)?;
        store::transform(&mut copy, "rotate", &read("rotation", [0.0, 0.0, 0.0]))?;
        store::transform(&mut copy, "translate", &sites.position(site))?;
        if let Some(tint) = sites.points.get("color") {
            tint_geometry(store, &mut copy, tint.numbers(site))?;
        }
        for (name, column) in sites.points.iter() {
            if name == POSITION_ATTRIBUTE || STANDARD_ATTRIBUTES.contains(&name) {
                continue;
            }
            let value = column.get(site);
            copy.points.remove(name);
            if copy.count(DomainKind::Points) > 0 {
                store.add_attribute(&mut copy, DomainKind::Points, name, &value)?;
            }
        }
        if !collection.material.is_null() {
            for face in 0..copy.count(DomainKind::Faces) {
                store.set_material(&mut copy, face, &collection.material)?;
            }
        }
        append(store, &mut result, copy)?;
    }
    Ok(result)
}

/// Multiply every stored color by `tint`; geometry without colors takes `tint` itself as its point color.
pub fn tint_geometry(store: &mut Store, geometry: &mut Geometry, tint: &[f64]) -> Result<(), String> {
    let mut tinted = false;
    for domain in DomainKind::ALL {
        if let Some(Column::Number {
            values,
            ..
        }) = geometry.domain_mut(domain).get_mut("color")
        {
            for color in values.as_chunks_mut::<4>().0 {
                for (channel, factor) in color.iter_mut().zip(tint) {
                    *channel *= factor;
                }
            }
            tinted = true;
        }
    }
    if !tinted && geometry.count(DomainKind::Points) > 0 {
        store.add_attribute(geometry, DomainKind::Points, "color", &Element::Numbers(tint.to_vec()))?;
    }
    Ok(())
}

/// Store `name` in the same domain on both sides. Different domains move to face corners, which every face can
/// represent; a missing side is filled with the shading default when the attributes are appended.
fn reconcile_shading_domain(target: &mut Geometry, source: &mut Geometry, name: &str) {
    match (shading_domain(target, name), shading_domain(source, name)) {
        (Some(a), Some(b)) if a != b => {
            promote_to_corners(target, name, a);
            promote_to_corners(source, name, b);
        }
        (Some(domain), None) => add_shading_default(source, name, domain),
        (None, Some(domain)) => add_shading_default(target, name, domain),
        _ => {}
    }
}

/// The most specific domain storing `name`.
fn shading_domain(geometry: &Geometry, name: &str) -> Option<DomainKind> {
    DomainKind::ALL.iter().rev().copied().find(|domain| geometry.domain(*domain).contains(name))
}

/// Copy `name` from `domain` onto face corners. Point values stay in place for isolated points.
fn promote_to_corners(geometry: &mut Geometry, name: &str, domain: DomainKind) {
    if domain == DomainKind::Corners {
        return;
    }
    let source = geometry.domain(domain).get(name).expect("the attribute exists").clone();
    let mut corners = Column::filled(&source.default_element(), 0);
    for (face_index, face) in geometry.faces.iter().enumerate() {
        for point in face {
            let element = if domain == DomainKind::Faces { face_index } else { *point };
            corners.push(&source.get(element));
        }
    }
    geometry.corners.remove(name);
    geometry.corners.insert(name, corners);
    if domain == DomainKind::Faces {
        geometry.face_attributes.remove(name);
    }
}

/// Declare `name` in `domain` with the value an unshaded element realizes with: the FORM tint for colors, zero
/// otherwise.
fn add_shading_default(geometry: &mut Geometry, name: &str, domain: DomainKind) {
    let length = geometry.count(domain);
    let element =
        if name == "color" { Element::Numbers(DEFAULT_TINT.to_vec()) } else { Element::Numbers(vec![0.0; 3]) };
    geometry.domain_mut(domain).insert(name, Column::filled(&element, length));
}

/// Declare on `target` every attribute `source` has in `domain`, checking that shared names agree on type.
fn reconcile_custom_attributes(target: &mut Geometry, source: &Geometry, domain: DomainKind) -> Result<(), String> {
    let length = target.count(domain);
    for (name, column) in source.domain(domain).iter() {
        if let Some(existing) = target.domain(domain).get(name) {
            if existing.kind() != column.kind() {
                return Err(format!(
                    "Cannot combine {} attribute '{name}': it is {} in one geometry and {} in the other.",
                    domain.name(),
                    existing.kind().description(),
                    column.kind().description()
                ));
            }
            continue;
        }
        if target.domain(domain).len() >= 64 {
            return Err("Attribute limit exceeded (64 per domain).".into());
        }
        let fill = fill_element(name, column);
        target.domain_mut(domain).insert(name, Column::filled(&fill, length));
    }
    Ok(())
}

/// The value elements that never had `name` receive when geometries are combined: the FORM tint for colors, so an
/// uncolored part keeps the look it realizes with, and the attribute's default otherwise.
fn fill_element(name: &str, column: &Column) -> Element {
    if name == "color"
        && matches!(
            column,
            Column::Number {
                size: 4,
                ..
            }
        )
    {
        Element::Numbers(DEFAULT_TINT.to_vec())
    } else if name == MATERIAL_ATTRIBUTE {
        Element::Material(None)
    } else {
        column.default_element()
    }
}

/// Add `source`'s materials to `target`'s table, returning each source index's position in the merged table.
fn merge_material_tables(target: &mut Geometry, source: &Geometry) -> Result<Vec<usize>, String> {
    source.materials.iter().map(|material| store::intern_material(target, material)).collect()
}
