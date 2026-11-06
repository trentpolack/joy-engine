// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Install Lua values and their editor descriptions together.

use super::{
    Function,
    LuaDefinitions,
    LuaSignature,
    LuaTypes,
    identifier,
};
use mlua::{
    FromLuaMulti,
    IntoLuaMulti,
    Lua,
    Table,
    Value,
};

/// A short borrow of a VM and its API descriptions. Functions are ordinary mlua callbacks; no wrapper runs per call.
pub struct LuaBindings<'a> {
    lua: &'a Lua,
    definitions: &'a mut LuaDefinitions,
}
impl<'a> LuaBindings<'a> {
    pub(crate) fn new(lua: &'a Lua, definitions: &'a mut LuaDefinitions) -> Self {
        Self {
            lua,
            definitions,
        }
    }

    /// Register a global function, or a function in an existing namespace such as `math.sin`.
    pub fn function<F, A, R>(&mut self, name: &str, signature: LuaSignature, function: F) -> mlua::Result<()>
    where
        F: Fn(&Lua, A) -> mlua::Result<R> + 'static,
        A: FromLuaMulti + LuaTypes,
        R: IntoLuaMulti + LuaTypes,
    {
        let description = Function::describe::<A, R>(signature, self.definitions)?;
        let (table, key) = self.destination(name)?;
        let function = self.lua.create_function(function)?;
        table.raw_set(key, function.clone())?;
        self.definitions.registered.insert(name.to_owned(), Value::Function(function));
        self.definitions.functions.insert(name.to_owned(), description);
        Ok(())
    }

    /// Expose the same callback in another namespace, carrying its existing signature with it.
    pub(crate) fn alias(&mut self, name: &str, original: &str) -> mlua::Result<()> {
        let description = self
            .definitions
            .functions
            .get(original)
            .cloned()
            .ok_or_else(|| mlua::Error::runtime(format!("no described Lua function `{original}`")))?;
        let (source, key) = self.destination(original)?;
        let value = source.raw_get::<Value>(key)?;
        let (target, key) = self.destination(name)?;
        target.raw_set(key, value.clone())?;
        self.definitions.registered.insert(name.to_owned(), value);
        self.definitions.functions.insert(name.to_owned(), description);
        Ok(())
    }

    /// Remove an unavailable standard binding while recording the sandbox's API boundary.
    pub(crate) fn remove(&mut self, name: &str) -> mlua::Result<()> {
        let (table, key) = self.destination(name)?;
        table.raw_set(key, Value::Nil)?;
        self.definitions.removed.insert(name.to_owned());
        Ok(())
    }

    /// Register a constant whose Lua type cannot be inferred, such as the non-nil `null` sentinel.
    pub(crate) fn constant(&mut self, name: &str, kind: &str, value: Value) -> mlua::Result<()> {
        let (table, key) = self.destination(name)?;
        table.raw_set(key, value.clone())?;
        self.definitions.registered.insert(name.to_owned(), value);
        self.definitions.constants.insert(name.to_owned(), (kind.to_owned(), "{}".to_owned()));
        Ok(())
    }

    fn destination<'b>(&self, name: &'b str) -> mlua::Result<(Table, &'b str)> {
        if name.split('.').count() > 2 {
            return Err(mlua::Error::runtime("Lua bindings support globals and one namespace level"));
        }
        let mut names = name.split('.').peekable();
        let mut table = self.lua.globals();
        while let Some(key) = names.next() {
            table = crate::frozen::backing(self.lua, &table)?;
            if !identifier(key) {
                return Err(mlua::Error::runtime(format!("invalid Lua binding `{name}`")));
            }
            if names.peek().is_none() {
                return Ok((table, key));
            }
            table = table.raw_get(key)?;
        }
        Err(mlua::Error::runtime("empty Lua binding name"))
    }
}
