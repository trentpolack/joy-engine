// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Readonly table graphs whose backing storage never crosses the authored-code boundary.

use mlua::Function;
use mlua::Lua;
use mlua::LuaSerdeExt;
use mlua::Table;

const REGISTRY_KEY: &str = "joy.frozen_tables";

pub(crate) fn register(lua: &Lua) -> mlua::Result<()> {
    // Only this trusted closure receives raw metatable access. Authored getmetatable/setmetatable keep
    // respecting __metatable protection; serde's protected array marker retains its exact identity.
    let get_metatable = lua.create_function(|_, table: Table| Ok(table.metatable()))?;
    let set_metatable = lua.create_function(|_, (table, metatable): (Table, Option<Table>)| {
        table.set_metatable(metatable)?;
        Ok(table)
    })?;
    let operations = lua.load(include_str!("frozen.lua")).set_name("joy/frozen.lua").call::<Table>((
        get_metatable,
        set_metatable,
        lua.array_metatable(),
    ))?;
    lua.set_named_registry_value(REGISTRY_KEY, operations)
}

pub(crate) fn freeze(lua: &Lua, table: &Table) -> mlua::Result<()> {
    let operations: Table = lua.named_registry_value(REGISTRY_KEY)?;
    operations.raw_get::<Function>("freeze")?.call::<()>(table.clone())
}

/// Host serde reads the backing directly without invoking authored metamethods. Keep this handle private.
pub(crate) fn backing(lua: &Lua, table: &Table) -> mlua::Result<Table> {
    let operations: Table = lua.named_registry_value(REGISTRY_KEY)?;
    operations.raw_get::<Function>("backing")?.call(table.clone())
}
