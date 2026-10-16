// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Statement and expression evaluation for one script run. Ported from `src/form/compiler.ts`.

use crate::form::{
    Diagnostic, GeometryData,
    ast::{ExprKind, Expression, Statement, StmtKind, Token},
    builder::Builder,
    materials::DEFAULT_MATERIAL_NAME,
    math, parameters,
    procedural::Procedural,
    resources, user_data,
    value::{Budget, SharedBudget, Value, arity, integer_range},
};
use serde_json::{Value as Json, json};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

type Scope = BTreeMap<String, Value>;
type Result<T> = std::result::Result<T, Failure>;

enum Failure {
    Message(String),
    Located(Diagnostic),
}
impl From<String> for Failure {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}
impl From<Diagnostic> for Failure {
    fn from(value: Diagnostic) -> Self {
        Self::Located(value)
    }
}
impl Failure {
    fn at(self, token: &Token) -> Diagnostic {
        match self {
            Self::Located(error) => error,
            Self::Message(message) => Diagnostic::at(message, token),
        }
    }
}

pub fn evaluate(
    statements: &[Statement],
    overrides: &Json,
    materials: Option<&Json>,
) -> std::result::Result<GeometryData, Diagnostic> {
    let root = Token { text: String::new(), line: 1, column: 1 };
    if !overrides.is_object() {
        return Err(Diagnostic::at("Parameter overrides must be an object.", &root));
    }
    let budget = Rc::new(RefCell::new(Budget::default()));
    let geometry = Builder::new(materials, budget.clone()).map_err(|message| Diagnostic::at(message, &root))?;
    let mut runtime =
        Runtime { overrides, geometry, random: 1, budget: budget.clone(), procedural: Procedural::new(budget) };
    let mut scope = Scope::from([
        ("pi".into(), Value::Number(std::f64::consts::PI)),
        ("tau".into(), Value::Number(std::f64::consts::TAU)),
        ("e".into(), Value::Number(std::f64::consts::E)),
    ]);
    runtime.execute(statements, &mut scope, true).map_err(|error| error.at(&root))?;
    runtime.procedural.finish(&mut runtime.geometry).map_err(|message| Diagnostic::at(message, &root))?;
    Ok(runtime.geometry.data)
}

struct Runtime<'a> {
    overrides: &'a Json,
    geometry: Builder,
    random: u32,
    budget: SharedBudget,
    procedural: Procedural,
}

impl Runtime<'_> {
    fn execute(&mut self, statements: &[Statement], scope: &mut Scope, top_level: bool) -> Result<()> {
        for statement in statements {
            self.spend(&statement.token)?;
            self.statement(statement, scope, top_level)
                .map_err(|error| Failure::Located(error.at(&statement.token)))?;
        }
        Ok(())
    }
    fn statement(&mut self, statement: &Statement, scope: &mut Scope, top_level: bool) -> Result<()> {
        match &statement.kind {
            StmtKind::Project { fields: project, .. } => {
                if !top_level || self.geometry.data.project.is_some() {
                    return Err("Declare project metadata once, outside repeat blocks.".to_string().into());
                }
                self.geometry.data.project = Some(project.clone());
            }
            StmtKind::WithUserData(expression, body) => {
                let record = user_data::data_record(&self.expression(expression, scope)?)?;
                let previous = self.geometry.user_data.clone();
                self.geometry.user_data = if record.is_null() { None } else { Some(record) };
                let result = self.execute(body, &mut scope.clone(), false);
                self.geometry.user_data = previous;
                result?;
            }
            StmtKind::Wrangle { domain, expression, binding, body } => {
                let value = self.expression(expression, scope)?;
                let count = self.procedural.begin_wrangle(domain, &value)?;
                let result = (|| -> Result<()> {
                    for index in 0..count {
                        self.budget.borrow_mut().spend(1)?;
                        self.procedural.set_index(index);
                        let mut inner = scope.clone();
                        inner.insert("index".into(), Value::Number(index as f64));
                        if let Some(binding) = binding {
                            inner.insert(binding.clone(), Value::Element(index));
                        }
                        self.execute(body, &mut inner, false)?;
                    }
                    Ok(())
                })();
                let end = self.procedural.end_wrangle();
                result?;
                end?;
            }
            StmtKind::BindingAttribute { binding, name, expression } => {
                if !matches!(scope.get(binding), Some(Value::Element(_))) {
                    return Err(format!("Unknown wrangle binding '{binding}'.").into());
                }
                let value = self.expression(expression, scope)?;
                self.procedural.write(name, &value)?;
            }
            StmtKind::If(expression, body) => {
                if self.expression(expression, scope)?.scalar()? != 0.0 {
                    self.execute(body, &mut scope.clone(), false)?;
                }
            }
            StmtKind::Let(name, expression) => {
                let value = self.expression(expression, scope)?;
                scope.insert(name.clone(), value);
            }
            StmtKind::Param { name, values, metadata } => {
                if !top_level {
                    return Err("Parameters must be declared outside repeat blocks.".to_string().into());
                }
                if self.geometry.data.parameters.iter().any(|p| p["name"] == name.as_str()) {
                    return Err(format!("Duplicate parameter '{name}'.").into());
                }
                let mut defaults = Vec::new();
                for expression in values {
                    defaults.push(parameters::read(&self.expression(expression, scope)?)?);
                }
                let mut evaluated_metadata = Vec::new();
                for (key, expression) in metadata {
                    evaluated_metadata.push((key.clone(), parameters::read(&self.expression(expression, scope)?)?));
                }
                let supplied = self.overrides.get(name).cloned().unwrap_or_else(|| defaults[0].json());
                let (parameter, value) = parameters::create(name, &defaults, &evaluated_metadata, &supplied)?;
                if value.kind("mesh") {
                    self.track_mesh(&value.json())?;
                }
                self.geometry.data.parameters.push(parameter);
                scope.insert(name.clone(), value);
            }
            StmtKind::Repeat { name, expression, body } => {
                let count = integer_range(self.expression(expression, scope)?.scalar()?, 0.0, 150_000.0)?;
                for index in 0..count {
                    self.spend(&statement.token)?;
                    let mut inner = scope.clone();
                    inner.insert(name.clone(), Value::Number(index as f64));
                    self.execute(body, &mut inner, false)?;
                }
            }
            StmtKind::Command(expression) => self.command(expression, scope)?,
        }
        Ok(())
    }

    fn expression(&mut self, expression: &Expression, scope: &Scope) -> Result<Value> {
        self.spend(&expression.token)?;
        let value =
            match &expression.kind {
                ExprKind::Number(number) => Value::Number(*number),
                ExprKind::Text(text) => return Ok(Value::text(text)),
                ExprKind::Literal(value) => return Ok(Value::data(value.clone())),
                ExprKind::Record(fields) => {
                    let mut record = serde_json::Map::new();
                    for (name, field) in fields {
                        record.insert(name.clone(), user_data::data_value(&self.expression(field, scope)?)?);
                    }
                    return Ok(Value::data(user_data::copy(&Json::Object(record), &self.budget)?));
                }
                ExprKind::Array(items) => {
                    let mut values = Vec::new();
                    for item in items {
                        values.push(user_data::data_value(&self.expression(item, scope)?)?);
                    }
                    return Ok(Value::data(user_data::copy(&Json::Array(values), &self.budget)?));
                }
                ExprKind::Variable(name) => scope
                    .get(name)
                    .cloned()
                    .ok_or_else(|| Diagnostic::at(format!("Unknown variable '{name}'."), &expression.token))?,
                ExprKind::Component(_, _) => self.component_chain(expression, scope)?,
                ExprKind::Unary(operator, operand) => {
                    let value = self.expression(operand, scope)?;
                    let sign = if operator == "-" { -1.0 } else { 1.0 };
                    match value {
                        Value::Array(values) => Value::Array(values.into_iter().map(|v| v * sign).collect()),
                        _ => Value::Number(sign * value.scalar()?),
                    }
                }
                ExprKind::Binary(_, _, _) => self.binary_chain(expression, scope)?,
                ExprKind::Call(name, expressions) => {
                    if name == "deform" {
                        return self.deform(expressions, scope);
                    }
                    let values = self.arguments(expressions, scope)?;
                    if let Some(value) = math::vector_function(name, &values)? {
                        value
                    } else if let Some(value) = self.procedural.call(name, &values, &mut self.geometry)? {
                        return Ok(value);
                    } else if let Some(value) = self.resource(name, &values)? {
                        return Ok(value);
                    } else {
                        let args = scalars(&values)?;
                        match name.as_str() {
                            "vector" | "rgba" => {
                                arity(name, args.len(), if name == "vector" { 3 } else { 4 })?;
                                Value::Array(args)
                            }
                            "rand" => {
                                arity(name, args.len(), 0)?;
                                self.random = self.random.wrapping_mul(1664525).wrapping_add(1013904223);
                                Value::Number(self.random as f64 / 4294967296.0)
                            }
                            _ => Value::Number(math::function(name, &args)?.ok_or_else(|| {
                                Diagnostic::at(format!("Unknown function '{name}'."), &expression.token)
                            })?),
                        }
                    }
                }
            };
        if matches!(&value,Value::Number(n) if !n.is_finite())
            || matches!(&value,Value::Array(v) if v.iter().any(|n|!n.is_finite()))
        {
            return Err(Diagnostic::at("Expression must produce a finite number.", &expression.token).into());
        }
        Ok(value)
    }

    // The parser intentionally accepts flat chains up to its token budget.
    // Evaluate their left spines iteratively rather than borrowing native stack.
    fn binary_chain(&mut self, expression: &Expression, scope: &Scope) -> Result<Value> {
        let mut frames = Vec::new();
        let mut current = expression;
        while let ExprKind::Binary(operator, left, right) = &current.kind {
            frames.push((operator.as_str(), right.as_ref(), &current.token));
            if matches!(left.kind, ExprKind::Binary(_, _, _)) {
                self.spend(&left.token)?;
            }
            current = left;
        }
        let mut value = self.expression(current, scope)?;
        while let Some((operator, right, token)) = frames.pop() {
            let right = self.expression(right, scope)?;
            value = math::arithmetic(operator, &value, &right)?;
            if matches!(&value,Value::Number(n) if !n.is_finite())
                || matches!(&value,Value::Array(v) if v.iter().any(|n|!n.is_finite()))
            {
                return Err(Diagnostic::at("Expression must produce a finite number.", token).into());
            }
        }
        Ok(value)
    }

    fn component_chain(&mut self, expression: &Expression, scope: &Scope) -> Result<Value> {
        let mut frames = Vec::new();
        let mut current = expression;
        while let ExprKind::Component(parent, name) = &current.kind {
            frames.push((name.as_str(), &current.token));
            if matches!(parent.kind, ExprKind::Component(_, _)) {
                self.spend(&parent.token)?;
            }
            current = parent;
        }
        let mut value = self.expression(current, scope)?;
        while let Some((name, token)) = frames.pop() {
            value = match value {
                Value::Element(index) => {
                    if name == "index" {
                        Value::Number(index as f64)
                    } else {
                        self.procedural.read(name)?
                    }
                }
                Value::Data(record) => {
                    let field = record
                        .as_object()
                        .and_then(|map| map.get(name))
                        .ok_or_else(|| Diagnostic::at(format!("Unknown user data field '{name}'."), token))?;
                    if let Some(number) = field.as_f64() {
                        Value::Number(number)
                    } else if let Some(text) = field.as_str() {
                        Value::text(text)
                    } else {
                        Value::Data(record.child(name))
                    }
                }
                Value::Array(components) => {
                    let names: &[&str] = if components.len() == 3 { &["x", "y", "z"] } else { &["r", "g", "b", "a"] };
                    let index = names
                        .iter()
                        .position(|value| *value == name)
                        .ok_or_else(|| Diagnostic::at("Invalid vector or color component.", token))?;
                    Value::Number(components[index])
                }
                _ => return Err(Diagnostic::at("Invalid vector or color component.", token).into()),
            };
        }
        Ok(value)
    }

    fn command(&mut self, expression: &Expression, scope: &Scope) -> Result<()> {
        let ExprKind::Call(name, expressions) = &expression.kind else {
            return Err("Expected a geometry command.".to_string().into());
        };
        if name == "material" {
            if expressions.len() > 1 {
                return Err("material expects one material value/name, or no arguments.".to_string().into());
            }
            if let Some(argument) = expressions.first()
                && !matches!(&argument.kind,ExprKind::Variable(name) if !scope.contains_key(name))
            {
                let value = self.expression(argument, scope)?;
                self.geometry.material_index = self.geometry.bind_material(&value)?;
                return Ok(());
            }
            let material_name = if let Some(Expression { kind: ExprKind::Variable(name), .. }) = expressions.first() {
                name.as_str()
            } else {
                DEFAULT_MATERIAL_NAME
            };
            let index =
                self.geometry.data.materials.iter().position(|material| material["name"] == material_name).ok_or_else(
                    || format!("Unknown material '{material_name}'. Add it in Materials or import MaterialX."),
                )?;
            self.geometry.material_index = index as u32;
            return Ok(());
        }
        if name == "surface" {
            return self.surface(expressions, scope);
        }
        let values = self.arguments(expressions, scope)?;
        if name == "userData" && values.len() <= 1 {
            let data = if values.is_empty() {
                crate::form::value::DataRef::new(Json::Null)
            } else {
                user_data::data_record(&values[0])?
            };
            self.geometry.user_data = if data.is_null() { None } else { Some(data) };
            return Ok(());
        }
        if self.procedural.call(name, &values, &mut self.geometry)?.is_some() {
            return Ok(());
        }
        if name == "instance" {
            self.geometry.instance(&values)?;
            return Ok(());
        }
        if name == "uv" {
            let args = scalars(&values)?;
            arity(name, args.len(), 2)?;
            self.geometry.uv = args;
            return Ok(());
        }
        if name == "color"
            && values.len() == 1
            && let Value::Array(color) = &values[0]
            && color.len() == 4
        {
            self.geometry.color = color.iter().map(|v| v.clamp(0.0, 1.0)).collect();
            return Ok(());
        }
        let args = scalars(&values)?;
        match name.as_str() {
            "normal" => {
                if args.is_empty() {
                    self.geometry.normal = vec![0.0; 3];
                    return Ok(());
                }
                arity(name, args.len(), 3)?;
                let scale = args.iter().map(|v| v.abs()).fold(0.0, f64::max);
                if scale == 0.0 {
                    return Err("normal expects a nonzero direction; use normal() for flat shading."
                        .to_string()
                        .into());
                }
                let direction: Vec<f64> = args.iter().map(|v| v / scale).collect();
                let length = math::hypot(&direction);
                self.geometry.normal = direction.into_iter().map(|v| v / length).collect();
            }
            "color" => {
                if args.len() != 3 && args.len() != 4 {
                    return Err("color expects RGB, RGBA, or a color value.".to_string().into());
                }
                self.geometry.color = args[..3]
                    .iter()
                    .chain(std::iter::once(args.get(3).unwrap_or(&1.0)))
                    .map(|v| v.clamp(0.0, 1.0))
                    .collect();
            }
            "seed" => {
                arity(name, args.len(), 1)?;
                if args[0].fract() != 0.0 {
                    return Err("Random seed must be a finite integer.".to_string().into());
                }
                self.random = math::to_u32(args[0]);
            }
            "point" => {
                arity(name, args.len(), 3)?;
                let index = self.geometry.vertex(args[0], args[1], args[2])?;
                self.geometry.data.points.push(index);
                self.geometry.data.point_materials.push(self.geometry.material_index);
            }
            "box" => {
                arity(name, args.len(), 6)?;
                self.geometry.box_geometry(&args)?;
            }
            "sphere" => {
                if args.len() != 4 && args.len() != 5 {
                    return Err("sphere expects 4 or 5 arguments.".to_string().into());
                }
                self.geometry.sphere(&args)?;
            }
            "triangle" => {
                arity(name, args.len(), 9)?;
                let a = self.geometry.vertex(args[0], args[1], args[2])?;
                let b = self.geometry.vertex(args[3], args[4], args[5])?;
                let c = self.geometry.vertex(args[6], args[7], args[8])?;
                self.geometry.face(a, b, c)?;
            }
            _ => return Err(format!("Unknown command '{name}'.").into()),
        }
        Ok(())
    }

    fn resource(&mut self, name: &str, values: &[Value]) -> Result<Option<Value>> {
        if name == "materialPbr" {
            arity(name, values.len(), 3)?;
            let Value::Array(color) = &values[0] else {
                return Err("materialPbr expects RGBA color, metallic, roughness.".to_string().into());
            };
            if color.len() != 4 {
                return Err("materialPbr expects RGBA color, metallic, roughness.".to_string().into());
            }
            return Ok(Some(Value::object(resources::material_pbr(color, values[1].scalar()?, values[2].scalar()?)?)));
        }
        let mesh = match name {
            "meshBox" => {
                arity(name, values.len(), 3)?;
                resources::mesh_box(values[0].scalar()?, values[1].scalar()?, values[2].scalar()?)?
            }
            "meshSphere" => {
                if values.len() != 1 && values.len() != 2 {
                    return Err("meshSphere expects radius and optional segments.".to_string().into());
                }
                resources::mesh_sphere(values[0].scalar()?, if values.len() == 2 { values[1].scalar()? } else { 20.0 })?
            }
            "transformMesh" => {
                arity(name, values.len(), 4)?;
                let mesh = values[0].shared("mesh")?;

                resources::transform_mesh(
                    &mesh.borrow(),
                    &values[1].vector()?,
                    &values[2].vector()?,
                    &values[3].vector()?,
                )?
            }
            _ => return Ok(None),
        };
        self.track_mesh(&mesh)?;
        Ok(Some(Value::object(mesh)))
    }
    fn track_mesh(&mut self, mesh: &Json) -> Result<()> {
        self.budget.borrow_mut().mesh(mesh["positions"].as_array().map_or(0, Vec::len) / 3)?;
        Ok(())
    }
    fn deform(&mut self, expressions: &[Expression], scope: &Scope) -> Result<Value> {
        arity("deform", expressions.len(), 4)?;
        let source = self.expression(&expressions[0], scope)?.shared("mesh")?;
        let source = source.borrow().clone();
        let mut result = resources::read_mesh(&source)?;
        self.track_mesh(&result)?;
        let positions = source["positions"].as_array().ok_or_else(|| "Expected a mesh value.".to_string())?;
        let uvs = source["uvs"].as_array().ok_or_else(|| "Expected a mesh value.".to_string())?;
        let mut inner = scope.clone();
        for index in 0..positions.len() / 3 {
            for (axis, name) in ["x", "y", "z"].iter().enumerate() {
                inner.insert((*name).into(), Value::Number(positions[index * 3 + axis].as_f64().unwrap()));
            }
            inner.insert("u".into(), Value::Number(uvs[index * 2].as_f64().unwrap()));
            inner.insert("v".into(), Value::Number(uvs[index * 2 + 1].as_f64().unwrap()));
            inner.insert("index".into(), Value::Number(index as f64));
            for axis in 0..3 {
                result["positions"][index * 3 + axis] =
                    json!(self.expression(&expressions[axis + 1], &inner)?.scalar()?);
            }
        }
        result["normals"] = json!(vec![0.0; positions.len()]);
        Ok(Value::object(resources::read_mesh(&result)?))
    }
    fn surface(&mut self, expressions: &[Expression], scope: &Scope) -> Result<()> {
        arity("surface", expressions.len(), 5)?;
        let mut values = Vec::new();
        for expression in &expressions[..4] {
            values.push(self.expression(expression, scope)?.scalar()?);
        }
        let columns = integer_range(values[0], 1.0, 256.0)?;
        let rows = integer_range(values[1], 1.0, 256.0)?;
        let width = values[2];
        let depth = values[3];
        if width <= 0.0 || depth <= 0.0 {
            return Err("Surface dimensions must be positive.".to_string().into());
        }
        let first = self.geometry.data.positions.len() / 3;
        let mut inner = scope.clone();
        for j in 0..=rows {
            for i in 0..=columns {
                let u = i as f64 / columns as f64;
                let v = j as f64 / rows as f64;
                let x = (u - 0.5) * width;
                let z = (v - 0.5) * depth;
                for (name, value) in [("x", x), ("z", z), ("u", u), ("v", v)] {
                    inner.insert(name.into(), Value::Number(value));
                }
                self.geometry.uv = vec![u, v];
                let y = self.expression(&expressions[4], &inner)?.scalar()?;
                self.geometry.vertex(x, y, z)?;
            }
        }
        for j in 0..rows {
            for i in 0..columns {
                let a = (first + j * (columns + 1) + i) as u32;
                let b = a + columns as u32 + 1;
                self.geometry.face(a, b, a + 1)?;
                self.geometry.face(a + 1, b, b + 1)?;
            }
        }
        Ok(())
    }
    fn arguments(&mut self, expressions: &[Expression], scope: &Scope) -> Result<Vec<Value>> {
        expressions.iter().map(|expression| self.expression(expression, scope)).collect()
    }
    fn spend(&self, token: &Token) -> Result<()> {
        self.budget.borrow_mut().spend(1).map_err(|message| Failure::Located(Diagnostic::at(message, token)))
    }
}
fn scalars(values: &[Value]) -> std::result::Result<Vec<f64>, String> {
    values.iter().map(Value::scalar).collect()
}
