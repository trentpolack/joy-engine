// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Runtime values and the shared per-evaluation work budget.

use serde_json::Value as Json;
use std::{cell::RefCell, rc::Rc};

pub type SharedObject = Rc<RefCell<Json>>;
pub type SharedBudget = Rc<RefCell<Budget>>;

#[derive(Clone, Debug)]
pub enum Value {
    Number(f64),
    Text(Rc<str>),
    Array(Vec<f64>),
    Data(DataRef),
    Object(SharedObject),
    Element(usize),
}
#[derive(Clone, Debug)]
pub struct DataRef {
    root: Rc<Json>,
    path: Vec<String>,
}
impl DataRef {
    pub fn new(value: Json) -> Self {
        Self { root: Rc::new(value), path: Vec::new() }
    }
    pub fn child(&self, name: &str) -> Self {
        let mut path = self.path.clone();
        path.push(name.into());
        Self { root: self.root.clone(), path }
    }
}
impl std::ops::Deref for DataRef {
    type Target = Json;
    fn deref(&self) -> &Json {
        let mut value = self.root.as_ref();
        for name in &self.path {
            value = &value[name];
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
    pub fn scalar(&self) -> Result<f64, String> {
        match self {
            Self::Number(n) if n.is_finite() => Ok(*n),
            _ => Err("Expected a finite scalar; parameter metadata requires min, max, and step.".into()),
        }
    }
    pub fn vector(&self) -> Result<Vec<f64>, String> {
        match self {
            Self::Array(v) if v.len() == 3 => Ok(v.clone()),
            _ => Err("Expected an XYZ vector.".into()),
        }
    }
    pub fn json(&self) -> Json {
        match self {
            Self::Number(n) => serde_json::json!(n),
            Self::Text(s) => serde_json::json!(s.as_ref()),
            Self::Array(v) => serde_json::json!(v),
            Self::Data(v) => (**v).clone(),
            Self::Object(v) => v.borrow().clone(),
            Self::Element(i) => serde_json::json!({"kind":"element","index":i}),
        }
    }
    pub fn kind(&self, expected: &str) -> bool {
        matches!(self, Self::Object(v) if v.borrow().get("kind").and_then(Json::as_str)==Some(expected))
    }
    pub fn shared(&self, expected: &str) -> Result<SharedObject, String> {
        match self {
            Self::Object(v) if self.kind(expected) => Ok(v.clone()),
            _ => Err(format!("Expected a {expected} value.")),
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
        if amount > 2_000_000usize.saturating_sub(self.operations) {
            return Err("Operation limit exceeded (2,000,000).".into());
        }
        self.operations += amount;
        Ok(())
    }
    pub fn mesh(&mut self, vertices: usize) -> Result<(), String> {
        if vertices > 150_000usize.saturating_sub(self.resource_vertices) {
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
