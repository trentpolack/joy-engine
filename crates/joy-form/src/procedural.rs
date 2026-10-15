// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Procedural geometry commands, wrangles, retained instances, and named outputs.
//! Ported from `src/form/procedural-runtime.ts`.

mod output;
mod scatter;
mod shapes;
mod store;

use crate::{
    builder::Builder,
    resources, user_data,
    value::{SharedBudget, SharedObject, Value, integer_range},
};
use serde_json::{Value as Json, json};
use std::collections::HashSet;
use store::{Store, attribute_at, count};

/// One evaluation owns procedural allocations, wrangle context, and output snapshots.
pub struct Procedural {
    store: Store,
    current: Option<Wrangle>,
    positions_changed: bool,
    normals_written: HashSet<usize>,
    outputs: Vec<Json>,
    preview_vertices: usize,
}

#[derive(Clone)]
struct Wrangle {
    object: SharedObject,
    instances: bool,
    domain: String,
    index: usize,
}
impl Wrangle {
    fn geometry<'a>(&self, object: &'a Json) -> &'a Json {
        if self.instances { &object["sites"] } else { object }
    }
    fn geometry_mut<'a>(&self, object: &'a mut Json) -> &'a mut Json {
        if self.instances { &mut object["sites"] } else { object }
    }
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

    pub fn call(&mut self, name: &str, values: &[Value], output: &mut Builder) -> Result<Option<Value>, String> {
        let value = match name {
            "box" if values.len() == 3 => {
                let mesh = resources::mesh_box(values[0].scalar()?, values[1].scalar()?, values[2].scalar()?)?;
                Value::object(self.store.import_mesh(&mesh)?)
            }
            "sphere" if values.len() == 1 || values.len() == 2 => {
                let mesh = resources::mesh_sphere(
                    values[0].scalar()?,
                    if values.len() == 2 { values[1].scalar()? } else { 20.0 },
                )?;
                Value::object(self.store.import_mesh(&mesh)?)
            }
            "translate" | "rotate" | "scale" => {
                arity(name, values, 2)?;
                let geometry = self.geometry(&values[0])?;
                self.spend((count(&geometry.borrow(), "points") + count(&geometry.borrow(), "faces") * 3) as f64)?;
                store::transform(&mut geometry.borrow_mut(), name, &values[1].vector()?)?;
                Value::Object(geometry)
            }
            "color" if values.len() == 2 && values[0].kind("geometry") => {
                let geometry = self.geometry(&values[0])?;
                let color = numeric(&values[1])?;
                if color.as_array().is_none_or(|a| a.len() != 4) {
                    return Err("color expects geometry and an RGBA color value.".into());
                }
                let length = count(&geometry.borrow(), "points");
                self.spend(length as f64)?;
                let mut geometry_data = geometry.borrow_mut();
                if geometry_data["attributes"]["points"].get("color").is_none() {
                    if self.current.is_some() {
                        return Err("Create point colors with color(geometry, rgba) before the wrangle.".into());
                    }
                    self.store.add_attribute(&mut geometry_data, "points", "color", &color)?;
                } else {
                    for index in 0..length {
                        self.store.set(&mut geometry_data, "points", "color", index, &color, false)?;
                    }
                }
                drop(geometry_data);
                Value::Object(geometry)
            }
            "geometry" => {
                if values.len() > 1 {
                    return Err("geometry expects zero arguments or a mesh.".into());
                }
                if values.is_empty() {
                    Value::object(store::create())
                } else if values[0].kind("mesh") {
                    Value::Object(self.geometry(&values[0])?)
                } else {
                    let geometry = self.geometry(&values[0])?;
                    let result = self.store.copy(&geometry.borrow())?;
                    Value::object(result)
                }
            }
            "grid" => {
                arity(name, values, 4)?;
                let [columns, rows, width, depth] =
                    [values[0].scalar()?, values[1].scalar()?, values[2].scalar()?, values[3].scalar()?];
                self.spend((columns + 1.0) * (rows + 1.0))?;
                Value::object(self.store.grid(columns, rows, width, depth)?)
            }
            "revolve" | "tube" => {
                arity(name, values, if name == "revolve" { 2 } else { 3 })?;
                let geometry = self.geometry(&values[0])?;
                let source = geometry.borrow();
                let sides = values[if name == "revolve" { 1 } else { 2 }].scalar()?;
                self.spend(count(&source, "points") as f64 * sides * 20.0)?;
                let result = if name == "revolve" {
                    shapes::revolve(&mut self.store, &source, sides)?
                } else {
                    shapes::tube(&mut self.store, &source, values[1].scalar()?, sides)?
                };
                Value::object(result)
            }
            "smoothNormals" => {
                arity(name, values, 1)?;
                let geometry = self.geometry(&values[0])?;
                let source = geometry.borrow();
                self.spend((count(&source, "points") + count(&source, "faces") * 3) as f64)?;
                let mut result = self.store.copy(&source)?;
                shapes::smooth_normals(&mut self.store, &mut result)?;
                Value::object(result)
            }
            "copy" => {
                arity(name, values, 1)?;
                let source = self.procedural(&values[0])?;
                let source = source.borrow();
                self.spend(size(&source) as f64)?;
                Value::object(self.snapshot(&source)?)
            }
            "addPoint" => {
                self.allow_topology()?;
                arity(name, values, 2)?;
                let geometry = self.geometry(&values[0])?;
                let index = self.store.add_point(&mut geometry.borrow_mut(), &values[1].vector()?)?;
                Value::Number(index as f64)
            }
            "addFace" => {
                self.allow_topology()?;
                arity(name, values, 4)?;
                let geometry = self.geometry(&values[0])?;
                let length = count(&geometry.borrow(), "points");
                let indices = values[1..]
                    .iter()
                    .map(|v| integer_range(v.scalar()?, 0.0, length as f64 - 1.0))
                    .collect::<Result<Vec<_>, _>>()?;
                let index = self.store.add_face(&mut geometry.borrow_mut(), &indices)?;
                Value::Number(index as f64)
            }
            "removePoint" | "removeFace" => {
                self.allow_topology()?;
                arity(name, values, 2)?;
                let geometry = self.geometry(&values[0])?;
                let mut geometry = geometry.borrow_mut();
                self.spend((count(&geometry, "points") + count(&geometry, "faces") * 3) as f64)?;
                let domain = if name == "removePoint" { "points" } else { "faces" };
                let index = integer_range(values[1].scalar()?, 0.0, count(&geometry, domain) as f64 - 1.0)?;
                if name == "removePoint" {
                    store::remove_point(&mut geometry, index)
                } else {
                    store::remove_face(&mut geometry, index)
                };
                Value::Number(0.0)
            }
            "pointCount" | "faceCount" => {
                arity(name, values, 1)?;
                let geometry = self.geometry(&values[0])?;
                let length = count(&geometry.borrow(), if name == "pointCount" { "points" } else { "faces" });
                Value::Number(length as f64)
            }
            "setUserData" => {
                arity(name, values, 3)?;
                let geometry = self.geometry(&values[0])?;
                let length = count(&geometry.borrow(), "points");
                let index = integer_range(values[1].scalar()?, 0.0, length as f64 - 1.0)?;
                let record = user_data::data_record(&values[2])?;
                self.store.set_user_data(&mut geometry.borrow_mut(), index, &record)?;
                Value::Number(0.0)
            }
            "userData" => {
                arity(name, values, 2)?;
                let geometry = self.geometry(&values[0])?;
                let geometry = geometry.borrow();
                let index = values[1].scalar()?;
                attribute_at(&geometry, "points", "position", index)?;
                Value::data(user_data::copy(&geometry["userData"][index as usize], &self.store.budget)?)
            }
            "attribute" => {
                arity(name, values, 4)?;
                let geometry = self.geometry(&values[0])?;
                let value = attribute_at(
                    &geometry.borrow(),
                    domain(text(&values[1])?)?,
                    text(&values[2])?,
                    values[3].scalar()?,
                )?;
                numeric_value(&value)
            }
            "addAttribute" => {
                if self.current.is_some() {
                    return Err("Declare attributes outside wrangles with addAttribute().".into());
                }
                arity(name, values, 4)?;
                let requested = text(&values[1])?;
                let instances = requested == "instances";
                let target = if instances {
                    if !values[0].kind("instances") {
                        return Err("The instances domain requires an instance collection.".into());
                    }
                    values[0].shared("instances")?
                } else {
                    if !values[0].kind("geometry") {
                        return Err("addAttribute requires editable geometry; use geometry(mesh) first.".into());
                    }
                    values[0].shared("geometry")?
                };
                let mut target = target.borrow_mut();
                let geometry = if instances { &mut target["sites"] } else { &mut *target };
                let target_domain = if instances { "points" } else { domain(requested)? };
                self.spend(count(geometry, target_domain) as f64)?;
                self.store.add_attribute(geometry, target_domain, text(&values[2])?, &numeric(&values[3])?)?;
                Value::Number(0.0)
            }
            "setAttribute" => {
                if self.current.is_some() {
                    return Err("Use named element assignments inside wrangles, such as point.density = value.".into());
                }
                arity(name, values, 5)?;
                let geometry = self.geometry(&values[0])?;
                let mut geometry = geometry.borrow_mut();
                let target_domain = domain(text(&values[1])?)?;
                let name = text(&values[2])?;
                let index = values[3].scalar()?;
                attribute_at(&geometry, target_domain, name, index)?;
                self.store.set(&mut geometry, target_domain, name, index as usize, &numeric(&values[4])?, false)?;
                Value::Number(0.0)
            }
            "scatter" => {
                if values.len() != 3 && values.len() != 4 {
                    return Err("scatter expects geometry, count, seed, and optional weight attribute name.".into());
                }
                let geometry = self.geometry(&values[0])?;
                let source = geometry.borrow();
                let amount = values[1].scalar()?;
                self.spend(
                    count(&source, "faces") as f64
                        + amount * (8 + source["attributes"]["points"].as_object().unwrap().len() * 3) as f64,
                )?;
                Value::object(scatter::scatter(
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
                if values.len() == 3 && !values[2].kind("material") {
                    return Err("instances expects a material value.".into());
                }
                self.spend((count(&source.borrow(), "points") + count(&sites.borrow(), "points")) as f64)?;
                let source = self.store.copy(&source.borrow())?;
                let sites = self.store.copy(&sites.borrow())?;
                Value::object(
                    json!({"kind":"instances","source":source,"sites":sites,"material":values.get(2).map(Value::json)}),
                )
            }
            "output" | "inspect" => {
                if self.current.is_some() {
                    return Err("Declare named outputs outside wrangles.".into());
                }
                arity(name, values, 2)?;
                let label = text(&values[0])?;
                if label.trim().is_empty()
                    || label.encode_utf16().count() > 64
                    || self.outputs.iter().any(|o| o["name"] == label)
                {
                    return Err("Output names must be unique, nonempty text of at most 64 characters.".into());
                }
                if self.outputs.len() >= 16 {
                    return Err("Named output limit exceeded (16).".into());
                }
                let source = self.procedural(&values[1])?;
                let source = source.borrow();
                self.spend(size(&source) as f64)?;
                let snapshot = self.snapshot(&source)?;
                let material_index = if snapshot["kind"] == "instances" && !snapshot["material"].is_null() {
                    Some(output.bind_material(&Value::object(snapshot["material"].clone()))?)
                } else {
                    None
                };
                let mut preview = Builder::new(None, self.store.budget.clone())?;
                preview.color = output.color.clone();
                preview.user_data = output.user_data.clone();
                preview.material_index = output.material_index;
                output::emit(&snapshot, &mut preview, material_index)?;
                self.preview_vertices += preview.data.positions.len() / 3;
                if self.preview_vertices > 150_000 {
                    return Err("Named output preview vertex limit exceeded (150,000).".into());
                }
                if name == "output" {
                    output::emit(&snapshot, output, material_index)?;
                }
                self.outputs
                    .push(json!({"name":label,"visible":name=="output","value":snapshot,"geometry":preview.data}));
                Value::Number(0.0)
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    pub fn read(&self, name: &str) -> Result<Value, String> {
        let current = self.current.as_ref().ok_or("Attribute access requires a wrangle.")?;
        let object = current.object.borrow();
        Ok(numeric_value(&attribute_at(current.geometry(&object), &current.domain, name, current.index as f64)?))
    }
    pub fn write(&mut self, name: &str, value: &Value) -> Result<(), String> {
        let current = self.current.clone().ok_or("Attribute assignment requires a wrangle.")?;
        self.read(name)?;
        self.store.set(
            current.geometry_mut(&mut current.object.borrow_mut()),
            &current.domain,
            name,
            current.index,
            &numeric(value)?,
            true,
        )?;
        if name == "position" {
            self.positions_changed = true;
        }
        if name == "normal" {
            self.normals_written.insert(current.index);
        }
        Ok(())
    }
    pub fn begin_wrangle(&mut self, requested: &str, value: &Value) -> Result<usize, String> {
        if self.current.is_some() {
            return Err("Nested wrangles are not supported; query other geometry with attribute().".into());
        }
        let instances = requested == "instances";
        let kind = if instances { "instances" } else { "geometry" };
        if !value.kind(kind) {
            return Err("Wrangles require a geometry value, or an instance collection for the instances domain. Use geometry(mesh) to edit a mesh.".into());
        }
        let target_domain = domain(if instances { "points" } else { requested })?;
        let object = value.shared(kind)?;
        let current = Wrangle { object, instances, domain: target_domain.into(), index: 0 };
        let length = count(current.geometry(&current.object.borrow()), target_domain);
        self.positions_changed = false;
        self.normals_written.clear();
        self.current = Some(current);
        Ok(length)
    }
    pub fn set_index(&mut self, index: usize) {
        if let Some(current) = &mut self.current {
            current.index = index;
        }
    }
    pub fn end_wrangle(&mut self) -> Result<(), String> {
        if let Some(current) = self.current.take()
            && self.positions_changed
        {
            let mut object = current.object.borrow_mut();
            let geometry = current.geometry_mut(&mut object);
            for domain in ["corners", "faces"] {
                geometry["attributes"][domain].as_object_mut().unwrap().shift_remove("normal");
            }
            if self.normals_written.is_empty() {
                geometry["attributes"]["points"].as_object_mut().unwrap().shift_remove("normal");
            } else if let Some(normals) = geometry["attributes"]["points"].get_mut("normal") {
                for (index, normal) in normals["values"].as_array_mut().unwrap().iter_mut().enumerate() {
                    if !self.normals_written.contains(&index) {
                        *normal = json!([0.0, 0.0, 0.0]);
                    }
                }
            }
        }
        Ok(())
    }
    pub fn finish(&mut self, output: &mut Builder) -> Result<(), String> {
        if !self.outputs.is_empty() {
            for named in &mut self.outputs {
                named["geometry"]["materials"] = json!(output.data.materials);
            }
            output.data.outputs = Some(std::mem::take(&mut self.outputs));
        }
        Ok(())
    }
    fn geometry(&mut self, value: &Value) -> Result<SharedObject, String> {
        if value.kind("geometry") {
            return value.shared("geometry");
        }
        if value.kind("mesh") {
            let mesh = value.json();
            self.spend(
                (mesh["positions"].as_array().unwrap().len() / 3 + mesh["indices"].as_array().unwrap().len() / 3)
                    as f64,
            )?;
            return Value::object(self.store.import_mesh(&mesh)?).shared("geometry");
        }
        Err("Expected a geometry or mesh value.".into())
    }
    fn procedural(&mut self, value: &Value) -> Result<SharedObject, String> {
        if value.kind("instances") { value.shared("instances") } else { self.geometry(value) }
    }
    fn snapshot(&mut self, value: &Json) -> Result<Json, String> {
        if value["kind"] == "geometry" {
            return self.store.copy(value);
        }
        Ok(
            json!({"kind":"instances","source":self.store.copy(&value["source"])?,"sites":self.store.copy(&value["sites"])?,"material":value["material"].clone()}),
        )
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
fn size(value: &Json) -> usize {
    if value["kind"] == "geometry" {
        count(value, "points") + count(value, "faces") * 3
    } else {
        count(&value["sites"], "points") * (count(&value["source"], "points") + count(&value["source"], "faces") * 3)
    }
}
fn arity(name: &str, values: &[Value], expected: usize) -> Result<(), String> {
    if values.len() != expected { Err(format!("{name} expects {expected} arguments.")) } else { Ok(()) }
}
fn text(value: &Value) -> Result<&str, String> {
    if let Value::Text(text) = value { Ok(text) } else { Err("Expected quoted text.".into()) }
}
fn domain(value: &str) -> Result<&str, String> {
    if ["points", "faces", "corners"].contains(&value) {
        Ok(value)
    } else {
        Err("Expected points, faces, or corners attribute domain.".into())
    }
}
fn numeric(value: &Value) -> Result<Json, String> {
    if matches!(value, Value::Number(_) | Value::Array(_)) {
        Ok(value.json())
    } else {
        Err("Attributes require scalar, vector, or color values.".into())
    }
}
fn numeric_value(value: &Json) -> Value {
    if value.is_array() { Value::Array(store::numbers(value)) } else { Value::Number(store::number(value)) }
}
