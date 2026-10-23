// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runtime values and the shared per-evaluation work budget.

use crate::procedural::{
    Geometry,
    Instances,
};
use serde_json::Value as Json;
use std::{
    cell::{
        Cell,
        Ref,
        RefCell,
        RefMut,
    },
    rc::Rc,
};

pub type SharedObject = Rc<RefCell<Json>>;
pub type SharedGeometry = Rc<SharedValue<Geometry>>;
pub type SharedInstances = Rc<SharedValue<Instances>>;
pub type SharedBudget = Rc<RefCell<Budget>>;

const CONSTANT_RESOURCE_ERROR: &str =
    "Cannot modify a constant value; use copy() to create editable geometry or instances.";

/// One shared resource identity. Freezing protects every alias that shares it; copying its data creates a fresh mutable
/// identity.
#[derive(Debug)]
pub struct SharedValue<T> {
    value: RefCell<T>,
    frozen: Cell<bool>,
}

impl<T> SharedValue<T> {
    pub fn new(value: T) -> Self {
        Self {
            value: RefCell::new(value),
            frozen: Cell::new(false),
        }
    }

    pub fn borrow(&self) -> Ref<'_, T> {
        self.value.borrow()
    }

    pub fn borrow_mut(&self) -> Result<RefMut<'_, T>, String> {
        self.ensure_mutable()?;
        Ok(self.value.borrow_mut())
    }

    pub fn freeze(&self) {
        self.frozen.set(true);
    }

    pub fn is_frozen(&self) -> bool {
        self.frozen.get()
    }

    pub fn ensure_mutable(&self) -> Result<(), String> {
        if self.frozen.get() { Err(CONSTANT_RESOURCE_ERROR.into()) } else { Ok(()) }
    }
}

/// Most operations one evaluation may spend, counting statements, expressions, and per-element procedural work.
pub const OPERATION_LIMIT: usize = 2_000_000;

/// Most vertices one evaluation may hold in mesh resources, cumulatively.
pub const RESOURCE_VERTEX_LIMIT: usize = 150_000;

#[derive(Clone, Debug)]
pub enum Value {
    Number(f64),
    Bool(bool),
    Text(Rc<str>),
    /// An XYZ vector or an RGBA color.
    Array(Vec<f64>),
    /// An immutable record, list, or `null` built from literals.
    Data(DataRef),
    /// A mesh or material resource, distinguished by its `kind` field.
    Object(SharedObject),
    /// Editable geometry. Bindings share it; `copy` and function arguments duplicate it.
    Geometry(SharedGeometry),
    /// A retained instance collection.
    Instances(SharedInstances),
    /// The element a wrangle binding names.
    Element(usize),
    /// One member of a script-declared `enum`.
    Enum(EnumValue),
    /// A script-declared `enum` itself; its members are read with `Name.member`.
    EnumType(Rc<EnumDefinition>),
    /// A module bound with `use`; its functions and declarations are read with `alias.name`.
    Module(usize),
    /// What a call that returns nothing produces; it cannot be stored or used as an operand.
    Unit,
}

/// An `enum` declaration: its name and members in declaration order.
#[derive(Debug, PartialEq, Eq)]
pub struct EnumDefinition {
    pub name: String,
    pub members: Vec<String>,
}

/// One member of an enum. Members of the same declaration compare by position; text compares by member name.
#[derive(Clone, Debug)]
pub struct EnumValue {
    pub definition: Rc<EnumDefinition>,
    pub index: usize,
}
impl EnumValue {
    pub fn member(&self) -> &str {
        &self.definition.members[self.index]
    }
}

#[derive(Clone, Debug)]
pub struct DataRef {
    root: Rc<Json>,
    path: Vec<String>,
}
impl DataRef {
    pub fn new(value: Json) -> Self {
        Self {
            root: Rc::new(value),
            path: Vec::new(),
        }
    }
    pub fn child(&self, name: &str) -> Self {
        let mut path = self.path.clone();
        path.push(name.into());
        Self {
            root: self.root.clone(),
            path,
        }
    }
}
impl std::ops::Deref for DataRef {
    type Target = Json;
    fn deref(&self) -> &Json {
        let mut value = self.root.as_ref();
        for name in &self.path {
            value = if let Ok(index) = name.parse::<usize>()
                && value.is_array()
            {
                &value[index]
            } else {
                &value[name]
            };
        }
        value
    }
}
impl Value {
    pub fn data(value: Json) -> Self {
        Self::Data(DataRef::new(value))
    }
    pub fn text(value: impl AsRef<str>) -> Self {
        Self::Text(Rc::from(value.as_ref()))
    }
    pub fn object(value: Json) -> Self {
        Self::Object(Rc::new(RefCell::new(value)))
    }
    pub fn geometry(value: Geometry) -> Self {
        Self::Geometry(Rc::new(SharedValue::new(value)))
    }
    pub fn instances(value: Instances) -> Self {
        Self::Instances(Rc::new(SharedValue::new(value)))
    }

    /// Freeze mutable resources in place, so aliases of the constant cannot bypass it. `const` declarations go through
    /// `Procedural::constant_value`, which snapshots a resource another binding still shares before freezing it.
    pub fn freeze(&self) -> Result<(), String> {
        match self {
            Self::Geometry(value) => value.freeze(),
            Self::Instances(value) => value.freeze(),
            Self::Element(_) | Self::Module(_) => {
                return Err(format!(
                    "Cannot store {} in a constant; capture a field or exported value instead.",
                    self.describe()
                ));
            }
            _ => {}
        }
        Ok(())
    }

    /// Reject write access to a frozen resource, including when a writable variable aliases it.
    pub fn ensure_mutable(&self) -> Result<(), String> {
        match self {
            Self::Geometry(value) => value.ensure_mutable(),
            Self::Instances(value) => value.ensure_mutable(),
            _ => Ok(()),
        }
    }

    /// A short description of the value's type for diagnostics, such as "text" or "a vector".
    pub fn describe(&self) -> String {
        match self {
            Self::Number(_) => "a number".into(),
            Self::Bool(_) => "true/false".into(),
            Self::Text(_) => "text".into(),
            Self::Array(components) if components.len() == 3 => "a vector".into(),
            Self::Array(_) => "a color".into(),
            Self::Data(data) if data.is_array() => "a list".into(),
            Self::Data(data) if data.is_null() => "null".into(),
            Self::Data(_) => "a record".into(),
            Self::Object(object) => {
                let kind = object.borrow().get("kind").and_then(Json::as_str).unwrap_or("resource").to_owned();
                format!("a {kind}")
            }
            Self::Geometry(_) => "geometry".into(),
            Self::Instances(_) => "an instance collection".into(),
            Self::Element(_) => "a wrangle element".into(),
            Self::Enum(value) => format!("{} member", value.definition.name),
            Self::EnumType(definition) => format!("the enum {}", definition.name),
            Self::Module(_) => "a module".into(),
            Self::Unit => "nothing".into(),
        }
    }

    pub fn scalar(&self) -> Result<f64, String> {
        match self {
            Self::Number(n) if n.is_finite() => Ok(*n),
            Self::Number(_) => Err("Expected a finite number.".into()),
            Self::Bool(_) => {
                Err("Expected a number; found true/false. Use number(flag) to convert it to 1 or 0.".into())
            }
            other => Err(format!("Expected a number; found {}.", other.describe())),
        }
    }
    pub fn condition(&self) -> Result<bool, String> {
        match self {
            Self::Bool(flag) => Ok(*flag),
            Self::Number(_) => {
                Err("Conditions require true or false; compare the number explicitly, such as 'count > 0'.".into())
            }
            other => Err(format!("Conditions require true or false; found {}.", other.describe())),
        }
    }
    pub fn vector(&self) -> Result<Vec<f64>, String> {
        match self {
            Self::Array(v) if v.len() == 3 => Ok(v.clone()),
            other => Err(format!("Expected an XYZ vector; found {}.", other.describe())),
        }
    }
    pub fn json(&self) -> Json {
        match self {
            Self::Number(n) => serde_json::json!(n),
            Self::Bool(flag) => Json::Bool(*flag),
            Self::Text(s) => serde_json::json!(s.as_ref()),
            Self::Array(v) => serde_json::json!(v),
            Self::Data(v) => (**v).clone(),
            Self::Object(v) => v.borrow().clone(),
            Self::Geometry(geometry) => geometry.borrow().json(),
            Self::Instances(instances) => instances.borrow().json(),
            Self::Element(i) => serde_json::json!({"kind":"element","index":i}),
            Self::Enum(value) => Json::String(value.member().to_owned()),
            Self::EnumType(_) | Self::Module(_) | Self::Unit => Json::Null,
        }
    }
    /// Whether the value is a resource (`"mesh"` or `"material"`), geometry (`"geometry"`), or an instance
    /// collection (`"instances"`) of the expected kind.
    pub fn kind(&self, expected: &str) -> bool {
        match self {
            Self::Object(v) => v.borrow().get("kind").and_then(Json::as_str) == Some(expected),
            Self::Geometry(_) => expected == "geometry",
            Self::Instances(_) => expected == "instances",
            _ => false,
        }
    }
    pub fn shared(&self, expected: &str) -> Result<SharedObject, String> {
        match self {
            Self::Object(v) if self.kind(expected) => Ok(v.clone()),
            other => Err(format!("Expected a {expected} value; found {}.", other.describe())),
        }
    }
}
#[derive(Default, Debug)]
pub struct Budget {
    pub operations: usize,
    pub resource_vertices: usize,
    pub user_data_bytes: usize,
    pub user_data_values: usize,
}
impl Budget {
    pub fn spend(&mut self, amount: usize) -> Result<(), String> {
        if amount > OPERATION_LIMIT.saturating_sub(self.operations) {
            return Err("Operation limit exceeded (2,000,000).".into());
        }
        self.operations += amount;
        Ok(())
    }
    pub fn mesh(&mut self, vertices: usize) -> Result<(), String> {
        if vertices > RESOURCE_VERTEX_LIMIT.saturating_sub(self.resource_vertices) {
            return Err("Mesh resource vertex limit exceeded (150,000 per evaluation).".into());
        }
        self.resource_vertices += vertices;
        Ok(())
    }
}
pub fn arity(name: &str, actual: usize, expected: usize) -> Result<(), String> {
    if actual != expected { Err(format!("{name} expects {expected} arguments; received {actual}.")) } else { Ok(()) }
}
pub fn integer_range(value: f64, min: f64, max: f64) -> Result<usize, String> {
    if !value.is_finite() || value.fract() != 0.0 || value < min || value > max {
        Err(format!("Expected an integer between {min} and {max}."))
    } else {
        Ok(value as usize)
    }
}
