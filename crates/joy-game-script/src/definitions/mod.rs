// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Definition generation from the same callbacks and userdata declarations used by the Lua runtime.

mod bindings;
mod types;
mod userdata;

use std::collections::{
    BTreeMap,
    BTreeSet,
};
use std::fmt::Write;

pub use bindings::LuaBindings;
pub use types::{
    LuaType,
    LuaTypes,
};
#[doc(hidden)]
pub use userdata::{
    LuaFields,
    LuaMethods,
};

/// Names and documentation that Rust's callback types cannot carry. Types are inferred unless explicitly refined.
#[derive(Clone, Debug, Default)]
pub struct LuaSignature {
    names: Vec<String>,
    arguments: BTreeMap<usize, String>,
    returns: Option<Vec<String>>,
    documentation: String,
}

impl LuaSignature {
    /// Name the arguments in Rust tuple order; use `...` for a variadic tail.
    pub fn new(names: &[&str]) -> Self {
        Self {
            names: names.iter().map(|name| (*name).to_owned()).collect(),
            ..Self::default()
        }
    }
    /// Describe a dynamic argument's Lua contract beside its conversion code.
    pub fn argument(mut self, index: usize, kind: &str) -> Self {
        self.arguments.insert(index, kind.to_owned());
        self
    }
    /// Describe a dynamic single result, such as `Vec3|JoyNull` or a table record.
    pub fn returns(mut self, kind: &str) -> Self {
        self.returns = Some(vec![kind.to_owned()]);
        self
    }
    /// Document units, ownership, or other semantics that cannot be derived from the Rust types.
    pub fn doc(mut self, documentation: &str) -> Self {
        self.documentation = documentation.to_owned();
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Function {
    arguments: Vec<(String, String)>,
    returns: Vec<String>,
    documentation: String,
}
impl Function {
    fn describe<A: LuaTypes, R: LuaTypes>(
        signature: LuaSignature,
        definitions: &mut LuaDefinitions,
    ) -> mlua::Result<Self> {
        let types = A::lua_types(definitions);
        let inferred_returns = R::lua_types(definitions);
        if types.len() != signature.names.len() || signature.arguments.keys().any(|index| *index >= types.len()) {
            return Err(mlua::Error::runtime("Lua parameter names must match the Rust callback arity"));
        }
        let mut arguments = Vec::new();
        for (index, (name, kind)) in signature.names.into_iter().zip(types).enumerate() {
            if name != "..." && !identifier(&name) {
                return Err(mlua::Error::runtime(format!("invalid Lua parameter name `{name}`")));
            }
            if (name == "...") != kind.ends_with("...") {
                return Err(mlua::Error::runtime("variadic Lua callbacks must name their tail `...`"));
            }
            let kind =
                signature.arguments.get(&index).cloned().unwrap_or_else(|| kind.trim_end_matches("...").to_owned());
            arguments.push((name, kind));
        }
        if signature.returns.as_ref().is_some_and(|types| types.len() != inferred_returns.len()) {
            return Err(mlua::Error::runtime("Lua result metadata must match the Rust callback arity"));
        }
        Ok(Self {
            arguments,
            returns: signature.returns.unwrap_or(inferred_returns),
            documentation: signature.documentation,
        })
    }

    fn write(&self, output: &mut String, name: &str) {
        for line in self.documentation.lines() {
            writeln!(output, "---{line}").unwrap();
        }
        for (name, kind) in &self.arguments {
            writeln!(output, "---@param {name} {kind}").unwrap();
        }
        for kind in &self.returns {
            writeln!(output, "---@return {kind}").unwrap();
        }
        let arguments = self.arguments.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ");
        writeln!(output, "function {name}({arguments}) end\n").unwrap();
    }
}

/// A collected userdata description. Constructed by `lua_userdata!`, not a separate API manifest.
#[doc(hidden)]
#[derive(Clone, Default, PartialEq)]
pub struct LuaClass {
    fields: BTreeMap<String, String>,
    setters: BTreeMap<String, String>,
    userdata: bool,
    methods: BTreeMap<String, Function>,
    operators: Vec<(String, Function)>,
}

/// One deterministic description set. No VM, gameplay state, or callbacks are retained here.
#[derive(Default)]
pub struct LuaDefinitions {
    classes: BTreeMap<String, Option<LuaClass>>,
    class_types: BTreeMap<String, std::any::TypeId>,
    functions: BTreeMap<String, Function>,
    constants: BTreeMap<String, (String, String)>,
    errors: Vec<String>,
    registered: BTreeMap<String, mlua::Value>,
    removed: BTreeSet<String>,
}

impl LuaDefinitions {
    /// Include a host type and the userdata types referenced by its fields and methods.
    pub fn include<T: LuaType>(&mut self) {
        T::lua_type(self);
    }

    /// A named structural table contract for callbacks that build or consume dynamic Lua tables.
    pub fn record(&mut self, name: &str, fields: &[(&str, &str)]) {
        if self.classes.contains_key(name) {
            self.errors.push(format!("duplicate Lua class `{name}`"));
            return;
        }
        self.classes.insert(
            name.to_owned(),
            Some(LuaClass {
                fields: fields.iter().map(|(name, kind)| ((*name).to_owned(), (*kind).to_owned())).collect(),
                ..LuaClass::default()
            }),
        );
    }

    /// Collect one class exactly once, including recursive references back to that class.
    #[doc(hidden)]
    pub fn collect_class<T: 'static>(&mut self, name: &str, collect: impl FnOnce(&mut Self, &mut LuaClass)) -> String {
        let identity = std::any::TypeId::of::<T>();
        if self.classes.contains_key(name) && self.class_types.get(name) != Some(&identity) {
            self.errors.push(format!("conflicting Lua class name `{name}`"));
        }
        if !self.classes.contains_key(name) {
            self.class_types.insert(name.to_owned(), identity);
            self.classes.insert(name.to_owned(), None);
            let mut class = LuaClass {
                userdata: true,
                ..LuaClass::default()
            };
            collect(self, &mut class);
            self.classes.insert(name.to_owned(), Some(class));
        }
        name.to_owned()
    }

    /// Render a LuaLS metadata file. Invalid declarations fail instead of silently dropping editor coverage.
    pub fn render(&self) -> mlua::Result<String> {
        self.render_extension(&Self::default())
    }

    /// Render only additions to a shared API. Redefinitions are rejected so two library files cannot disagree.
    pub fn render_extension(&self, base: &Self) -> mlua::Result<String> {
        if !self.errors.is_empty() {
            return Err(mlua::Error::runtime(self.errors.join("\n")));
        }
        let mut output = String::from(
            "-- Copyright (c) 2026 Trent Polack. All Rights Reserved.\n-- Licensed under the MIT License.\n\n---@meta\n-- Generated from Rust Lua registrations. Do not edit or execute.\n\n",
        );
        for (name, class) in &self.classes {
            if let Some(original) = base.classes.get(name) {
                if original != class {
                    return Err(mlua::Error::runtime(format!("host redefines shared Lua class `{name}`")));
                }
                continue;
            }
            if !identifier(name) {
                return Err(mlua::Error::runtime(format!("invalid Lua class `{name}`")));
            }
            let class = class.as_ref().ok_or_else(|| mlua::Error::runtime(format!("incomplete Lua class `{name}`")))?;
            writeln!(output, "---@class {name}").unwrap();
            for (operator, function) in &class.operators {
                let arguments = function.arguments.iter().map(|(_, kind)| kind.as_str()).collect::<Vec<_>>().join(", ");
                writeln!(output, "---@operator {operator}({arguments}): {}", function.returns.join(", ")).unwrap();
            }
            for (field, kind) in &class.fields {
                let access = if class.userdata {
                    match class.setters.get(field) {
                        None => " # Read-only at runtime.".to_owned(),
                        Some(setter) if setter != kind => {
                            format!(" # Setter accepts {setter}; reads may also return other documented values.")
                        }
                        _ => String::new(),
                    }
                } else {
                    String::new()
                };
                writeln!(output, "---@field {field} {kind}{access}").unwrap();
            }
            // Local class tables describe userdata, without inventing constructors in the global namespace.
            writeln!(output, "local _JoyLua_{name} = {{}}\n").unwrap();
            for (method, function) in &class.methods {
                function.write(&mut output, &format!("_JoyLua_{name}:{method}"));
            }
        }
        for (name, (kind, value)) in &self.constants {
            if base.constants.get(name).is_some_and(|original| original != &(kind.clone(), value.clone())) {
                return Err(mlua::Error::runtime(format!("host redefines shared Lua constant `{name}`")));
            }
            if !base.constants.contains_key(name) {
                writeln!(output, "---@type {kind}\n{name} = {value}\n").unwrap();
            }
        }
        for (name, function) in &self.functions {
            if base.functions.get(name).is_some_and(|original| original != function) {
                return Err(mlua::Error::runtime(format!("host redefines shared Lua function `{name}`")));
            }
            if !base.functions.contains_key(name) {
                function.write(&mut output, name);
            }
        }
        Ok(output)
    }
}

fn identifier(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z' | 'A'..='Z' | '_'))
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !matches!(
            name,
            "and"
                | "break"
                | "do"
                | "else"
                | "elseif"
                | "end"
                | "false"
                | "for"
                | "function"
                | "goto"
                | "if"
                | "in"
                | "local"
                | "nil"
                | "not"
                | "or"
                | "repeat"
                | "return"
                | "then"
                | "true"
                | "until"
                | "while"
        )
}

/// Snapshot the public host namespace, reading frozen standard/enum tables through their private backing.
/// Only one namespace level is part of the registration API; gameplay records are not traversed.
pub(crate) fn snapshot(lua: &mlua::Lua) -> mlua::Result<BTreeMap<String, mlua::Value>> {
    let mut bindings = BTreeMap::new();
    for pair in lua.globals().pairs::<String, mlua::Value>() {
        let (name, value) = pair?;
        if name == "__joy_require" {
            continue;
        }
        if let mlua::Value::Table(table) = &value {
            // Raw proxy members take precedence over __index, including trusted-host writes that bypass freezing.
            // Inspect both layers in that order so a shadow cannot evade the API audit.
            for source in [crate::frozen::backing(lua, table)?, table.clone()] {
                for pair in source.pairs::<mlua::Value, mlua::Value>() {
                    let (key, value) = pair?;
                    if let mlua::Value::String(key) = key {
                        bindings.insert(format!("{name}.{}", key.to_str()?), value);
                    }
                }
            }
        }
        bindings.insert(name, value);
    }
    Ok(bindings)
}

impl LuaDefinitions {
    pub(crate) fn validate_bindings(
        &self,
        lua: &mlua::Lua,
        baseline: &BTreeMap<String, mlua::Value>,
    ) -> mlua::Result<()> {
        let mut expected = baseline.clone();
        expected.retain(|name, _| {
            !self.removed.iter().any(|removed| name == removed || name.starts_with(&format!("{removed}.")))
        });
        expected.extend(self.registered.clone());
        let observed = snapshot(lua)?;
        for (name, value) in &observed {
            if expected.get(name) != Some(value) {
                return Err(mlua::Error::runtime(format!(
                    "undocumented Lua binding `{name}`; register through ScriptEngine::bindings"
                )));
            }
        }
        for name in expected.keys() {
            if !observed.contains_key(name) {
                return Err(mlua::Error::runtime(format!("missing Lua binding `{name}`")));
            }
        }
        Ok(())
    }

    pub(crate) fn enum_table<T: LuaType>(
        &mut self,
        lua: &mlua::Lua,
        name: &str,
        table: &mlua::Table,
    ) -> mlua::Result<()> {
        let kind = T::lua_type(self);
        let mut class = LuaClass::default();
        for pair in crate::frozen::backing(lua, table)?.pairs::<String, mlua::Value>() {
            let (field, value) = pair?;
            class.fields.insert(field.clone(), kind.clone());
            self.registered.insert(format!("{name}.{field}"), value);
        }
        self.classes.insert(name.to_owned(), Some(class));
        self.constants.insert(name.to_owned(), (name.to_owned(), "{}".to_owned()));
        self.registered.insert(name.to_owned(), mlua::Value::Table(table.clone()));
        Ok(())
    }
}
