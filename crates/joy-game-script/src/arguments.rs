// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Checked host arguments. mlua's generic container conversions push nested values before returning, and assert
//! on foreign VM handles. Validate each child before insertion so native and panic-abort browser builds return errors.

use std::collections::BTreeMap;
use std::collections::HashMap;

use mlua::AnyUserData;
use mlua::Function;
use mlua::IntoLua;
use mlua::Lua;
use mlua::LuaSerdeExt;
use mlua::MultiValue;
use mlua::Table;
use mlua::Thread;
use mlua::UserData;
use mlua::Value;

use crate::ScriptRandom;
use crate::ScriptVec3;
use crate::engine::ensure_owner;

/// A host value convertible without pushing foreign VM handles. Standard scalars, Lua handles, vectors, maps,
/// options, and arrays are supported. Custom implementations are trusted host code and must validate child handles
/// before passing them to mlua. For arbitrary userdata, use [`ScriptUserData`].
pub trait IntoScriptValue {
    /// Convert into `lua`, returning an error for foreign VM values or failed allocation.
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value>;
}

/// A checked tuple of host arguments, excluding the state argument inserted by the engine. Tuples through sixteen
/// elements and [`MultiValue`] are supported. Nested containers validate ownership before inserting any child.
pub trait IntoScriptArguments {
    /// Convert the host argument tuple into values belonging to `lua`.
    fn into_script_arguments(self, lua: &Lua) -> mlua::Result<MultiValue>;
}

/// Mark an arbitrary host userdata value for allocation in the destination VM. This explicit wrapper avoids an
/// unchecked blanket `IntoLua` conversion, which could hide foreign handles inside a generic Rust collection.
#[derive(Clone, Debug)]
pub struct ScriptUserData<T>(pub T);

impl<T: UserData + 'static> IntoScriptValue for ScriptUserData<T> {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        lua.create_userdata(self.0).map(Value::UserData)
    }
}

pub(crate) fn value(lua: &Lua, source: impl IntoScriptValue) -> mlua::Result<Value> {
    let value = source.into_script_value(lua)?;
    ensure_owner(lua, &value)?;
    Ok(value)
}

macro_rules! scalar_values {
    ($($kind:ty),* $(,)?) => {$ (
        impl IntoScriptValue for $kind {
            fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> { self.into_lua(lua) }
        }
    )*};
}
scalar_values!(bool, i8, i16, i32, i64, u8, u16, u32, u64, isize, usize, f32, f64, String, &str);

impl IntoScriptValue for () {
    fn into_script_value(self, _: &Lua) -> mlua::Result<Value> {
        Ok(Value::Nil)
    }
}
impl IntoScriptValue for &String {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        self.as_str().into_lua(lua)
    }
}

macro_rules! handle_values {
    ($($kind:ty),* $(,)?) => {$ (
        impl IntoScriptValue for $kind {
            fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
                let value = self.into_lua(lua)?;
                ensure_owner(lua, &value)?;
                Ok(value)
            }
        }
    )*};
}
handle_values!(
    Value,
    &Value,
    Table,
    &Table,
    Function,
    &Function,
    Thread,
    &Thread,
    AnyUserData,
    &AnyUserData,
    mlua::LuaString,
    &mlua::LuaString
);

impl IntoScriptValue for ScriptRandom {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        ScriptUserData(self).into_script_value(lua)
    }
}
impl IntoScriptValue for ScriptVec3 {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        ScriptUserData(self).into_script_value(lua)
    }
}

impl<T: IntoScriptValue> IntoScriptValue for Option<T> {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        match self {
            Some(source) => value(lua, source),
            None => Ok(Value::Nil),
        }
    }
}
impl<T: IntoScriptValue> IntoScriptValue for Vec<T> {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table_with_capacity(self.len(), 0)?;
        table.set_metatable(Some(lua.array_metatable()))?;
        for (index, source) in self.into_iter().enumerate() {
            let value = value(lua, source)?;
            // Nil removes Lua array entries; keep nullable host sequence slots explicitly present.
            table.raw_set(index + 1, if value.is_nil() { Value::NULL } else { value })?;
        }
        Ok(Value::Table(table))
    }
}
impl<T: IntoScriptValue, const N: usize> IntoScriptValue for [T; N] {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        Vec::from(self).into_script_value(lua)
    }
}
impl<T: IntoScriptValue + Clone> IntoScriptValue for &[T] {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        self.to_vec().into_script_value(lua)
    }
}
impl<K: IntoScriptValue, V: IntoScriptValue> IntoScriptValue for BTreeMap<K, V> {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        map(lua, self)
    }
}
impl<K: IntoScriptValue, V: IntoScriptValue, S> IntoScriptValue for HashMap<K, V, S> {
    fn into_script_value(self, lua: &Lua) -> mlua::Result<Value> {
        map(lua, self)
    }
}
fn map<K: IntoScriptValue, V: IntoScriptValue>(
    lua: &Lua,
    entries: impl IntoIterator<Item = (K, V)>,
) -> mlua::Result<Value> {
    let table = lua.create_table()?;
    for (key, source) in entries {
        table.raw_set(value(lua, key)?, value(lua, source)?)?;
    }
    Ok(Value::Table(table))
}

impl IntoScriptArguments for () {
    fn into_script_arguments(self, _: &Lua) -> mlua::Result<MultiValue> {
        Ok(MultiValue::new())
    }
}
impl IntoScriptArguments for MultiValue {
    fn into_script_arguments(self, lua: &Lua) -> mlua::Result<MultiValue> {
        for argument in &self {
            ensure_owner(lua, argument)?;
        }
        Ok(self)
    }
}
macro_rules! tuple_arguments {
    ($($kind:ident : $binding:ident),+ $(,)?) => {
        impl<$($kind: IntoScriptValue),+> IntoScriptArguments for ($($kind,)+) {
            fn into_script_arguments(self, lua: &Lua) -> mlua::Result<MultiValue> {
                let ($($binding,)+) = self;
                let mut arguments = MultiValue::new();
                $(arguments.push_back(value(lua, $binding)?);)+
                Ok(arguments)
            }
        }
    };
}
tuple_arguments!(A: a);
tuple_arguments!(A: a, B: b);
tuple_arguments!(A: a, B: b, C: c);
tuple_arguments!(A: a, B: b, C: c, D: d);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h, I: i);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h, I: i, J: j);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h, I: i, J: j, K: k);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h, I: i, J: j, K: k, L: l);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h, I: i, J: j, K: k, L: l, M: m);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h, I: i, J: j, K: k, L: l, M: m, N: n);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h, I: i, J: j, K: k, L: l, M: m, N: n, O: o);
tuple_arguments!(A: a, B: b, C: c, D: d, E: e, F: f, G: g, H: h, I: i, J: j, K: k, L: l, M: m, N: n, O: o, P: p);
