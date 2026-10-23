// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Procedural geometry built-ins, wrangles, retained instances, and named outputs.
//!
//! Geometry values are explicit: constructors return geometry, operations take it as their first argument, and only
//! `output` adds anything to the evaluation's result.

mod combine;
mod geometry;
mod output;
mod scatter;
mod shapes;
mod store;

pub use geometry::{
    Geometry,
    Instances,
};

use crate::{
    builder::{
        Builder,
        box_arrays,
        sphere_arrays,
    },
    named_output::{
        NamedOutput,
        OutputValue,
    },
    resources,
    value::{
        SharedBudget,
        SharedGeometry,
        SharedInstances,
        Value,
        integer_range,
    },
};
use geometry::{
    DomainKind,
    Element,
    MATERIAL_ATTRIBUTE,
    POSITION_ATTRIBUTE,
};
use serde_json::{
    Value as Json,
    json,
};
use std::{
    cell::{
        Ref,
        RefMut,
    },
    collections::HashSet,
};
use store::{
    Store,
    attribute_at,
};

/// Most named outputs and inspections one evaluation may declare.
const OUTPUT_LIMIT: usize = 16;

/// Most vertices all named output and inspection previews may realize together.
const PREVIEW_VERTEX_LIMIT: usize = 150_000;

/// One evaluation owns procedural allocations, wrangle context, and output snapshots.
pub struct Procedural {
    store: Store,
    current: Option<Wrangle>,
    positions_changed: bool,
    normals_written: HashSet<usize>,
    outputs: Vec<NamedOutput>,
    preview_vertices: usize,
}

/// Geometry a wrangle or attribute operation edits: geometry itself, or an instance collection's site points.
#[derive(Clone)]
enum Target {
    Geometry(SharedGeometry),
    Sites(SharedInstances),
}

impl Target {
    fn borrow(&self) -> Ref<'_, Geometry> {
        match self {
            Self::Geometry(geometry) => geometry.borrow(),
            Self::Sites(collection) => Ref::map(collection.borrow(), |collection| &collection.sites),
        }
    }

    fn borrow_mut(&self) -> Result<RefMut<'_, Geometry>, String> {
        match self {
            Self::Geometry(geometry) => geometry.borrow_mut(),
            Self::Sites(collection) => Ok(RefMut::map(collection.borrow_mut()?, |collection| &mut collection.sites)),
        }
    }
}

/// A geometry or instance collection, as outputs, copies, and function arguments take them.
enum ProceduralValue {
    Geometry(SharedGeometry),
    Instances(SharedInstances),
}

impl ProceduralValue {
    /// Points plus three components per face corner (times sites for instances), for operation budgets.
    fn size(&self) -> usize {
        match self {
            Self::Geometry(geometry) => geometry.borrow().size(),
            Self::Instances(collection) => collection.borrow().size(),
        }
    }
}

#[derive(Clone)]
struct Wrangle {
    target: Target,
    domain: DomainKind,
    index: usize,
}

impl Procedural {
    pub fn new(budget: SharedBudget) -> Self {
        Self {
            store: Store::new(budget),
            current: None,
            positions_changed: false,
            normals_written: HashSet::new(),
            outputs: vec![],
            preview_vertices: 0,
        }
    }

    /// Run the procedural built-in `name`, or return `None` when `name` is not one.
    pub fn call(&mut self, name: &str, values: &[Value], output: &mut Builder) -> Result<Option<Value>, String> {
        let value = match name {
            // ---- Constructors.
            "box" => {
                arity(name, values, 3)?;
                let mesh = box_arrays(values[0].scalar()?, values[1].scalar()?, values[2].scalar()?)?;
                Value::geometry(self.store.import_arrays(&mesh)?)
            }
            "sphere" => {
                if values.len() != 1 && values.len() != 2 {
                    return Err("sphere expects a radius and optional segments.".into());
                }
                let segments = if values.len() == 2 { values[1].scalar()? } else { 20.0 };
                let mesh = sphere_arrays(values[0].scalar()?, segments)?;
                Value::geometry(self.store.import_arrays(&mesh)?)
            }
            "geometry" => {
                if values.len() > 1 {
                    return Err("geometry expects zero arguments or a mesh.".into());
                }
                if values.is_empty() {
                    Value::geometry(Geometry::default())
                } else if values[0].kind("mesh") {
                    Value::Geometry(self.geometry(&values[0])?)
                } else {
                    let geometry = self.geometry(&values[0])?;
                    let result = self.store.copy(&geometry.borrow())?;
                    Value::geometry(result)
                }
            }
            "grid" => {
                arity(name, values, 4)?;
                let [columns, rows, width, depth] =
                    [values[0].scalar()?, values[1].scalar()?, values[2].scalar()?, values[3].scalar()?];
                self.spend((columns + 1.0) * (rows + 1.0))?;
                Value::geometry(self.store.grid(columns, rows, width, depth)?)
            }
            "revolve" | "tube" => {
                arity(name, values, if name == "revolve" { 2 } else { 3 })?;
                let geometry = self.geometry(&values[0])?;
                let source = geometry.borrow();
                let sides = values[if name == "revolve" { 1 } else { 2 }].scalar()?;
                self.spend(source.count(DomainKind::Points) as f64 * sides * 20.0)?;
                let result = if name == "revolve" {
                    shapes::revolve(&mut self.store, &source, sides)?
                } else {
                    shapes::tube(&mut self.store, &source, values[1].scalar()?, sides)?
                };
                Value::geometry(result)
            }
            "place" => {
                if values.len() != 2 && values.len() != 4 {
                    return Err("place expects a geometry or mesh, a position, and optional rotation and scale.".into());
                }
                let position = values[1].vector()?;
                let rotation = if values.len() == 4 { values[2].vector()? } else { vec![0.0; 3] };
                let scale = if values.len() == 4 { values[3].vector()? } else { vec![1.0; 3] };
                Value::geometry(self.place(&values[0], &position, &rotation, &scale)?)
            }
            "merge" => {
                if values.is_empty() {
                    return Err("merge expects at least one geometry.".into());
                }
                let mut result = self.combinable(&values[0])?;
                self.store.reserve(
                    result.count(DomainKind::Points),
                    result.count(DomainKind::Faces),
                    result.components(),
                )?;
                for value in &values[1..] {
                    let source = self.combinable(value)?;
                    self.spend(source.size() as f64)?;
                    combine::append(&mut self.store, &mut result, source)?;
                }
                Value::geometry(result)
            }
            "realize" => {
                arity(name, values, 1)?;
                let Value::Instances(collection) = &values[0] else {
                    return Err(format!("Expected an instances value; found {}.", values[0].describe()));
                };
                let collection = collection.borrow();
                self.spend(collection.size() as f64)?;
                Value::geometry(combine::realize(&mut self.store, &collection)?)
            }
            "copy" => {
                arity(name, values, 1)?;
                let source = self.procedural(&values[0])?;
                self.spend(source.size() as f64)?;
                self.snapshot(&source)?
            }

            // ---- In-place edits that return the edited geometry.
            "translate" | "rotate" | "scale" => {
                arity(name, values, 2)?;
                // A mesh becomes new geometry here, so `translate(meshBox(…), offset)` returns a placed copy.
                let geometry = self.geometry(&values[0])?;
                self.spend(geometry.borrow().size() as f64)?;
                store::transform(&mut *geometry.borrow_mut()?, name, &values[1].vector()?)?;
                Value::Geometry(geometry)
            }
            "append" => {
                self.allow_topology()?;
                arity(name, values, 2)?;
                let target = self.editable(&values[0], name)?;
                let source = self.combinable(&values[1])?;
                self.spend(source.size() as f64)?;
                combine::append(&mut self.store, &mut *target.borrow_mut()?, source)?;
                Value::Geometry(target)
            }
            "color" => {
                arity(name, values, 2)?;
                let geometry = self.editable(&values[0], name)?;
                let color = match &values[1] {
                    Value::Array(components) if components.len() == 4 => components.clone(),
                    Value::Number(_) | Value::Array(_) => {
                        return Err("color expects geometry and an RGBA color value.".into());
                    }
                    other => return Err(format!("Expected a number, vector, or color; found {}.", other.describe())),
                };
                if self.current.is_some() {
                    return Err("Color whole geometry outside wrangles; inside one, assign 'point.color'.".into());
                }
                let mut geometry = geometry.borrow_mut()?;
                let length = geometry.count(DomainKind::Points);
                self.spend(length as f64)?;
                // Painting replaces any per-face or per-corner colors, so the whole geometry takes the color.
                for domain in [DomainKind::Faces, DomainKind::Corners] {
                    geometry.domain_mut(domain).remove("color");
                }
                let color = Element::Numbers(color);
                if geometry.points.contains("color") {
                    for index in 0..length {
                        self.store.set(&mut geometry, DomainKind::Points, "color", index, &color, false)?;
                    }
                } else {
                    self.store.add_attribute(&mut geometry, DomainKind::Points, "color", &color)?;
                }
                drop(geometry);
                Value::Geometry(self.editable(&values[0], name)?)
            }
            "material" => {
                arity(name, values, 2)?;
                let geometry = self.editable(&values[0], name)?;
                if self.current.is_some() {
                    return Err(
                        "Assign whole-geometry materials outside wrangles; inside one, assign 'face.material'.".into(),
                    );
                }
                let material = material_record(&values[1])?;
                let mut geometry_data = geometry.borrow_mut()?;
                let faces = geometry_data.count(DomainKind::Faces);
                self.spend(faces as f64)?;
                self.store.add_material_attribute(&mut geometry_data)?;
                for face in 0..faces {
                    self.store.set_material(&mut geometry_data, face, &material)?;
                }
                drop(geometry_data);
                Value::Geometry(geometry)
            }
            "smoothNormals" => {
                arity(name, values, 1)?;
                let geometry = self.geometry(&values[0])?;
                let source = geometry.borrow();
                self.spend(source.size() as f64)?;
                let mut result = self.store.copy(&source)?;
                shapes::smooth_normals(&mut self.store, &mut result)?;
                Value::geometry(result)
            }

            // ---- Topology.
            "addPoint" => {
                self.allow_topology()?;
                if values.len() != 2 && values.len() != 3 {
                    return Err(
                        "addPoint expects geometry, a position, and an optional record of attribute values.".into()
                    );
                }
                let geometry = self.editable(&values[0], name)?;
                let mut geometry = geometry.borrow_mut()?;
                let index = self.store.add_point(&mut geometry, &values[1].vector()?)?;
                if let Some(record) = values.get(2) {
                    self.set_point_record(&mut geometry, index, record)?;
                }
                Value::Number(index as f64)
            }
            "addFace" => {
                self.allow_topology()?;
                arity(name, values, 4)?;
                let geometry = self.editable(&values[0], name)?;
                let length = geometry.borrow().count(DomainKind::Points);
                let indices = values[1..]
                    .iter()
                    .map(|v| integer_range(v.scalar()?, 0.0, length as f64 - 1.0))
                    .collect::<Result<Vec<_>, _>>()?;
                let index = self.store.add_face(&mut *geometry.borrow_mut()?, &indices)?;
                Value::Number(index as f64)
            }
            "removePoint" | "removeFace" => {
                self.allow_topology()?;
                arity(name, values, 2)?;
                let geometry = self.editable(&values[0], name)?;
                let mut geometry = geometry.borrow_mut()?;
                self.spend(geometry.size() as f64)?;
                let domain = if name == "removePoint" { DomainKind::Points } else { DomainKind::Faces };
                let index = integer_range(values[1].scalar()?, 0.0, geometry.count(domain) as f64 - 1.0)?;
                if name == "removePoint" {
                    store::remove_point(&mut geometry, index)
                } else {
                    store::remove_face(&mut geometry, index)
                };
                Value::Unit
            }
            "pointCount" | "faceCount" => {
                arity(name, values, 1)?;
                let geometry = self.geometry(&values[0])?;
                let domain = if name == "pointCount" { DomainKind::Points } else { DomainKind::Faces };
                Value::Number(geometry.borrow().count(domain) as f64)
            }

            // ---- Attributes.
            "attribute" => {
                arity(name, values, 4)?;
                let geometry = self.geometry(&values[0])?;
                let geometry = geometry.borrow();
                read_attribute(
                    &geometry,
                    DomainKind::parse(text(&values[1])?)?,
                    text(&values[2])?,
                    values[3].scalar()?,
                )?
            }
            "hasAttribute" => {
                arity(name, values, 3)?;
                let geometry = self.geometry(&values[0])?;
                let domain = DomainKind::parse(text(&values[1])?)?;
                Value::Bool(geometry.borrow().domain(domain).contains(text(&values[2])?))
            }
            "addAttribute" => {
                if self.current.is_some() {
                    return Err("Declare attributes outside wrangles with addAttribute().".into());
                }
                arity(name, values, 4)?;
                let requested = text(&values[1])?;
                let attribute_name = text(&values[2])?;
                let (target, domain) = if requested == "instances" {
                    let Value::Instances(collection) = &values[0] else {
                        return Err("The instances domain requires an instance collection.".into());
                    };
                    (Target::Sites(collection.clone()), DomainKind::Points)
                } else {
                    (Target::Geometry(self.editable(&values[0], name)?), DomainKind::parse(requested)?)
                };
                let mut geometry = target.borrow_mut()?;
                self.spend(geometry.count(domain) as f64)?;
                if attribute_name == MATERIAL_ATTRIBUTE && domain == DomainKind::Faces {
                    self.store.add_material_attribute(&mut geometry)?;
                    if !matches!(&values[3], Value::Data(data) if data.is_null()) {
                        let material = material_record(&values[3])?;
                        for face in 0..geometry.count(DomainKind::Faces) {
                            self.store.set_material(&mut geometry, face, &material)?;
                        }
                    }
                } else {
                    self.store.add_attribute(&mut geometry, domain, attribute_name, &attribute_value(&values[3])?)?;
                }
                Value::Unit
            }
            "removeAttribute" => {
                if self.current.is_some() {
                    return Err("Remove attributes outside wrangles.".into());
                }
                arity(name, values, 3)?;
                let geometry = self.editable(&values[0], name)?;
                self.store.remove_attribute(
                    &mut *geometry.borrow_mut()?,
                    DomainKind::parse(text(&values[1])?)?,
                    text(&values[2])?,
                )?;
                Value::Unit
            }
            "setAttribute" => {
                if self.current.is_some() {
                    return Err("Use named element assignments inside wrangles, such as point.density = value.".into());
                }
                arity(name, values, 5)?;
                let geometry = self.editable(&values[0], name)?;
                let mut geometry = geometry.borrow_mut()?;
                let domain = DomainKind::parse(text(&values[1])?)?;
                let attribute_name = text(&values[2])?;
                let index = values[3].scalar()?;
                attribute_at(&geometry, domain, attribute_name, index)?;
                if attribute_name == MATERIAL_ATTRIBUTE && domain == DomainKind::Faces {
                    self.store.set_material(&mut geometry, index as usize, &material_record(&values[4])?)?;
                } else {
                    self.store.set(
                        &mut geometry,
                        domain,
                        attribute_name,
                        index as usize,
                        &attribute_value(&values[4])?,
                        false,
                    )?;
                }
                Value::Unit
            }

            // ---- Distribution and retained instances.
            "scatter" => {
                if values.len() != 3 && values.len() != 4 {
                    return Err("scatter expects geometry, count, seed, and optional weight attribute name.".into());
                }
                let geometry = self.geometry(&values[0])?;
                let source = geometry.borrow();
                let amount = values[1].scalar()?;
                self.spend(source.count(DomainKind::Faces) as f64 + amount * (8 + source.points.len() * 3) as f64)?;
                Value::geometry(scatter::scatter(
                    &mut self.store,
                    &source,
                    amount,
                    values[2].scalar()?,
                    if values.len() == 4 { text(&values[3])? } else { "" },
                )?)
            }
            "instances" => {
                if values.len() != 2 && values.len() != 3 {
                    return Err("instances expects source geometry/mesh, sites, and optional material.".into());
                }
                let source = self.geometry(&values[0])?;
                let sites = self.geometry(&values[1])?;
                let material = if values.len() == 3 { material_record(&values[2])? } else { Json::Null };
                self.spend(
                    (source.borrow().count(DomainKind::Points) + sites.borrow().count(DomainKind::Points)) as f64,
                )?;
                let source = self.store.copy(&source.borrow())?;
                let sites = self.store.copy(&sites.borrow())?;
                Value::instances(Instances {
                    source,
                    sites,
                    material,
                })
            }

            // ---- Results.
            "output" | "inspect" => {
                if self.current.is_some() {
                    return Err("Declare named outputs outside wrangles.".into());
                }
                arity(name, values, 2)?;
                let label = text(&values[0])?;
                if label.trim().is_empty()
                    || label.chars().count() > 64
                    || self.outputs.iter().any(|existing| existing.name == label)
                {
                    return Err("Output names must be unique, nonempty text of at most 64 characters.".into());
                }
                if self.outputs.len() >= OUTPUT_LIMIT {
                    return Err("Named output limit exceeded (16).".into());
                }
                let source = self.procedural(&values[1])?;
                self.spend(source.size() as f64)?;

                // Realize once with the evaluation's palette, then copy that realization into the result. The
                // snapshot counts against the allocation limits like the copy it records.
                let mut preview = output.realization();
                let snapshot = match &source {
                    ProceduralValue::Geometry(geometry) => {
                        let geometry = geometry.borrow();
                        self.reserve_copy(&geometry)?;
                        output::emit_geometry(&geometry, &mut preview)?;
                        OutputValue::geometry(geometry.clone())
                    }
                    ProceduralValue::Instances(collection) => {
                        let collection = collection.borrow();
                        self.reserve_copy(&collection.source)?;
                        self.reserve_copy(&collection.sites)?;
                        output::emit_instances(&collection, &mut preview)?;
                        OutputValue::instances(collection.clone())
                    }
                };
                self.preview_vertices += preview.data.positions.len() / 3;
                if self.preview_vertices > PREVIEW_VERTEX_LIMIT {
                    return Err("Named output preview vertex limit exceeded (150,000).".into());
                }
                if name == "output" {
                    output.append(&preview.data)?;
                }
                self.outputs.push(NamedOutput {
                    name: label.to_owned(),
                    visible: name == "output",
                    value: snapshot,
                    geometry: preview.data,
                });
                Value::Unit
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    /// Read the current wrangle element's attribute `name`.
    pub fn read(&self, name: &str) -> Result<Value, String> {
        let current = self.current.as_ref().ok_or("Attribute access requires a wrangle.")?;
        read_attribute(&current.target.borrow(), current.domain, name, current.index as f64)
    }

    /// Write the current wrangle element's attribute `name`, which must already exist.
    pub fn write(&mut self, name: &str, value: &Value) -> Result<(), String> {
        let current = self.current.clone().ok_or("Attribute assignment requires a wrangle.")?;
        self.read(name)?;
        let mut geometry = current.target.borrow_mut()?;
        if name == MATERIAL_ATTRIBUTE && current.domain == DomainKind::Faces {
            return self.store.set_material(&mut geometry, current.index, &material_record(value)?);
        }
        self.store.set(&mut geometry, current.domain, name, current.index, &attribute_value(value)?, true)?;
        if name == POSITION_ATTRIBUTE {
            self.positions_changed = true;
        }
        if name == "normal" {
            self.normals_written.insert(current.index);
        }
        Ok(())
    }

    /// Start a wrangle over `requested` (a domain, or `instances` for a collection's sites), returning its element
    /// count.
    pub fn begin_wrangle(&mut self, requested: &str, value: &Value) -> Result<usize, String> {
        if self.current.is_some() {
            return Err("Nested wrangles are not supported; query other geometry with attribute().".into());
        }
        let target = match (requested == "instances", value) {
            (true, Value::Instances(collection)) => Target::Sites(collection.clone()),
            (false, Value::Geometry(geometry)) => Target::Geometry(geometry.clone()),
            _ => {
                return Err("Wrangles require a geometry value, or an instance collection for the instances domain. Use geometry(mesh) to edit a mesh.".into());
            }
        };
        let domain = if requested == "instances" { DomainKind::Points } else { DomainKind::parse(requested)? };
        let length = target.borrow().count(domain);
        self.positions_changed = false;
        self.normals_written.clear();
        self.current = Some(Wrangle {
            target,
            domain,
            index: 0,
        });
        Ok(length)
    }

    pub fn set_index(&mut self, index: usize) {
        if let Some(current) = &mut self.current {
            current.index = index;
        }
    }

    /// Finish the current wrangle. Moved points invalidate face and corner normals, and point normals the wrangle
    /// did not write.
    pub fn end_wrangle(&mut self) -> Result<(), String> {
        if let Some(current) = self.current.take()
            && self.positions_changed
        {
            let mut geometry = current.target.borrow_mut()?;
            for domain in [DomainKind::Corners, DomainKind::Faces] {
                geometry.domain_mut(domain).remove("normal");
            }
            if self.normals_written.is_empty() {
                geometry.points.remove("normal");
            } else if let Some(normals) = geometry.points.get_mut("normal") {
                let zero = Element::Numbers(vec![0.0; 3]);
                for index in 0..normals.len() {
                    if !self.normals_written.contains(&index) {
                        normals.set(index, &zero);
                    }
                }
            }
        }
        Ok(())
    }

    pub fn finish(&mut self, output: &mut Builder) -> Result<(), String> {
        if !self.outputs.is_empty() {
            output.data.outputs = Some(std::mem::take(&mut self.outputs));
        }
        Ok(())
    }

    /// An independent copy of a geometry or instance collection, for a function argument passed by value. Other
    /// values are immutable and are shared as they are.
    pub fn copy_argument(&mut self, value: &Value) -> Result<Value, String> {
        if !matches!(value, Value::Geometry(_) | Value::Instances(_)) {
            return Ok(value.clone());
        }
        let source = self.procedural(value)?;
        self.spend(source.size() as f64)?;
        self.snapshot(&source)
    }

    /// Set point `index`'s attributes from a record's fields, declaring any attribute the geometry lacks with its
    /// type's default (zero, empty text, or false) on every other point.
    fn set_point_record(&mut self, geometry: &mut Geometry, index: usize, record: &Value) -> Result<(), String> {
        let Value::Data(data) = record else {
            return Err(format!("addPoint's attribute values must be a record; found {}.", record.describe()));
        };
        let fields = data.as_object().ok_or("addPoint's attribute values must be a record.")?;
        for (field, value) in fields {
            if field == POSITION_ATTRIBUTE {
                return Err("Give the point's position as addPoint's second argument, not in its record.".into());
            }
            let element = match value {
                Json::Number(number) => Element::Numbers(vec![number.as_f64().unwrap_or_default()]),
                Json::String(text) => Element::Text(text.clone()),
                Json::Bool(flag) => Element::Bool(*flag),
                Json::Array(components) if components.iter().all(Json::is_number) => {
                    Element::Numbers(components.iter().filter_map(Json::as_f64).collect())
                }
                _ => {
                    return Err(format!(
                        "Point attribute '{field}' must be a number, vector, color, text, or true/false; records and lists cannot be attributes."
                    ));
                }
            };
            if !geometry.points.contains(field) {
                let default = geometry::Column::filled(&element, 0).default_element();
                self.store.add_attribute(geometry, DomainKind::Points, field, &default)?;
            }
            self.store.set(geometry, DomainKind::Points, field, index, &element, false)?;
        }
        Ok(())
    }

    /// Geometry for reading: editable geometry itself, or a mesh resource imported as new geometry.
    fn geometry(&mut self, value: &Value) -> Result<SharedGeometry, String> {
        if let Value::Geometry(geometry) = value {
            return Ok(geometry.clone());
        }
        if value.kind("mesh") {
            let mesh = value.json();
            self.spend(
                (mesh["positions"].as_array().map_or(0, Vec::len) / 3
                    + mesh["indices"].as_array().map_or(0, Vec::len) / 3) as f64,
            )?;
            let Value::Geometry(geometry) = Value::geometry(self.store.import_mesh(&mesh)?) else {
                unreachable!("Value::geometry builds geometry")
            };
            return Ok(geometry);
        }
        Err(format!("Expected a geometry or mesh value; found {}.", value.describe()))
    }

    /// Geometry an operation edits in place. Meshes are immutable resources, so editing one would be lost.
    fn editable(&mut self, value: &Value, operation: &str) -> Result<SharedGeometry, String> {
        match value {
            Value::Geometry(geometry) => Ok(geometry.clone()),
            _ if value.kind("mesh") => {
                Err(format!("{operation} edits geometry in place; convert the mesh with geometry(mesh) first."))
            }
            _ => Err(format!("{operation} expects geometry; found {}.", value.describe())),
        }
    }

    /// Geometry to combine: a copy of geometry or a mesh, or a realized instance collection.
    fn combinable(&mut self, value: &Value) -> Result<Geometry, String> {
        if let Value::Instances(collection) = value {
            let collection = collection.borrow();
            self.spend(collection.size() as f64)?;
            return combine::realize(&mut self.store, &collection);
        }
        let geometry = self.geometry(value)?;
        Ok(geometry.borrow().clone())
    }

    fn procedural(&mut self, value: &Value) -> Result<ProceduralValue, String> {
        match value {
            Value::Instances(collection) => Ok(ProceduralValue::Instances(collection.clone())),
            _ => Ok(ProceduralValue::Geometry(self.geometry(value)?)),
        }
    }

    /// An independent copy of a geometry or instance collection, counted against the allocation limits.
    fn snapshot(&mut self, value: &ProceduralValue) -> Result<Value, String> {
        match value {
            ProceduralValue::Geometry(geometry) => Ok(Value::geometry(self.store.copy(&geometry.borrow())?)),
            ProceduralValue::Instances(collection) => {
                let collection = collection.borrow();
                Ok(Value::instances(Instances {
                    source: self.store.copy(&collection.source)?,
                    sites: self.store.copy(&collection.sites)?,
                    material: collection.material.clone(),
                }))
            }
        }
    }

    /// Count a copy of `geometry` against the allocation limits without making one.
    fn reserve_copy(&mut self, geometry: &Geometry) -> Result<(), String> {
        self.store.reserve(geometry.count(DomainKind::Points), geometry.count(DomainKind::Faces), geometry.components())
    }

    /// A placed copy of `source`: scaled, rotated (XYZ Euler radians), then translated. Meshes keep their exact
    /// vertex order, normals, and UVs.
    fn place(&mut self, source: &Value, position: &[f64], rotation: &[f64], scale: &[f64]) -> Result<Geometry, String> {
        if source.kind("mesh") {
            let mesh = resources::transform_mesh(&source.json(), position, rotation, scale)?;
            self.store.budget.borrow_mut().mesh(mesh["positions"].as_array().map_or(0, Vec::len) / 3)?;
            self.spend(mesh["positions"].as_array().map_or(0, Vec::len) as f64 / 3.0)?;
            return self.store.import_mesh(&mesh);
        }
        let geometry = self.geometry(source)?;
        let geometry = geometry.borrow();
        self.spend(geometry.size() as f64)?;
        let mut result = self.store.copy(&geometry)?;
        if scale.iter().any(|component| component.abs() < 1e-9) {
            return Err("Placement scale must be nonzero.".into());
        }
        store::transform(&mut result, "scale", scale)?;
        store::transform(&mut result, "rotate", rotation)?;
        store::transform(&mut result, "translate", position)?;
        Ok(result)
    }

    fn allow_topology(&self) -> Result<(), String> {
        if self.current.is_some() {
            Err("Edit topology outside wrangles so element indices remain stable.".into())
        } else {
            Ok(())
        }
    }

    fn spend(&self, amount: f64) -> Result<(), String> {
        if !amount.is_finite() || !(0.0..=2_000_000.0).contains(&amount) {
            return Err("Operation limit exceeded (2,000,000).".into());
        }
        self.store.budget.borrow_mut().spend(amount.ceil() as usize)
    }
}

fn arity(name: &str, values: &[Value], expected: usize) -> Result<(), String> {
    if values.len() != expected { Err(format!("{name} expects {expected} arguments.")) } else { Ok(()) }
}

fn text(value: &Value) -> Result<&str, String> {
    if let Value::Text(text) = value {
        Ok(text)
    } else {
        Err(format!("Expected quoted text; found {}.", value.describe()))
    }
}

/// The stored form of an attribute value: numbers and vectors as numbers, text and enum members as text, and
/// true/false as booleans.
fn attribute_value(value: &Value) -> Result<Element, String> {
    match value {
        Value::Number(number) => Ok(Element::Numbers(vec![*number])),
        Value::Array(components) => Ok(Element::Numbers(components.clone())),
        Value::Bool(flag) => Ok(Element::Bool(*flag)),
        Value::Text(text) => Ok(Element::Text(text.to_string())),
        Value::Enum(member) => Ok(Element::Text(member.member().to_owned())),
        other => Err(format!(
            "Attributes store numbers, vectors, colors, text, enum members, or true/false; found {}.",
            other.describe()
        )),
    }
}

/// The material table entry for a material resource, a palette material named by text, or `null` for none.
fn material_record(value: &Value) -> Result<Json, String> {
    match value {
        Value::Object(_) if value.kind("material") => Ok(value.json()),
        Value::Text(name) => Ok(json!({"palette": name.as_ref()})),
        Value::Data(data) if data.is_null() => Ok(Json::Null),
        other => Err(format!("Expected a material or a palette material name; found {}.", other.describe())),
    }
}

/// Read one attribute element as a FORM value; a face's material reads as its material resource or palette name.
fn read_attribute(geometry: &Geometry, domain: DomainKind, name: &str, index: f64) -> Result<Value, String> {
    Ok(match attribute_at(geometry, domain, name, index)? {
        Element::Material(Some(entry)) => {
            let record = &geometry.materials[entry];
            match record.get("palette").and_then(Json::as_str) {
                Some(palette) => Value::text(palette),
                None => Value::object(record.clone()),
            }
        }
        Element::Material(None) => Value::data(Json::Null),
        Element::Text(text) => Value::text(text),
        Element::Bool(flag) => Value::Bool(flag),
        Element::Numbers(components) if components.len() == 1 => Value::Number(components[0]),
        Element::Numbers(components) => Value::Array(components),
    })
}
