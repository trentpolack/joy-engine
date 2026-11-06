// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Typed host results. Read collections through private frozen storage while returning original Lua handles.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::hash::BuildHasher;
use std::hash::Hash;

use mlua::AnyUserData;
use mlua::FromLua;
use mlua::Function;
use mlua::Lua;
use mlua::Table;
use mlua::Thread;
use mlua::UserData;
use mlua::Value;

use crate::ScriptRandom;
use crate::ScriptUserData;
use crate::ScriptVec3;
use crate::engine::ensure_owner;

/// A typed host result that reads frozen collections without exposing their backing tables. Standard scalars,
/// Lua handles, vectors, options, sequences, and maps are supported. Handle results retain their original identity,
/// including inside collections. Collection traversal follows the finite Rust result type; it does not recursively
/// copy arbitrary Lua graphs. For serde records with cycle and depth validation, use [`crate::from_script_value`].
///
/// Custom implementations are trusted host code, like [`crate::IntoScriptValue`]. They must validate ownership and
/// bound their own recursion. A custom userdata result can delegate to its existing [`FromLua`] implementation.
pub trait FromScriptValue: Sized {
    /// Read `value` from `lua`, reporting ownership or type mismatches as errors.
    fn from_script_value(value: Value, lua: &Lua) -> mlua::Result<Self>;
}

pub(crate) fn value<T: FromScriptValue>(lua: &Lua, source: Value) -> mlua::Result<T> {
    ensure_owner(lua, &source)?;
    T::from_script_value(source, lua)
}

macro_rules! direct_values {
    ($($kind:ty),* $(,)?) => {$ (
        impl FromScriptValue for $kind {
            fn from_script_value(value: Value, lua: &Lua) -> mlua::Result<Self> {
                ensure_owner(lua, &value)?;
                Self::from_lua(value, lua)
            }
        }
    )*};
}
direct_values!(
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    u64,
    isize,
    usize,
    f32,
    f64,
    String,
    Box<str>,
    Value,
    Table,
    Function,
    Thread,
    AnyUserData,
    mlua::LuaString,
    ScriptVec3
);

impl FromScriptValue for bool {
    fn from_script_value(value: Value, _: &Lua) -> mlua::Result<Self> {
        // A typed gameplay decision must not silently coerce nil or an unrelated value through Lua truthiness.
        match value {
            Value::Boolean(value) => Ok(value),
            _ => Err(mlua::Error::runtime("expected a boolean script result")),
        }
    }
}

impl<T: UserData + Clone + 'static> FromScriptValue for ScriptUserData<T> {
    fn from_script_value(value: Value, lua: &Lua) -> mlua::Result<Self> {
        ensure_owner(lua, &value)?;
        let userdata = AnyUserData::from_lua(value, lua)?;
        Ok(Self(userdata.borrow::<T>()?.clone()))
    }
}

impl FromScriptValue for ScriptRandom {
    fn from_script_value(value: Value, lua: &Lua) -> mlua::Result<Self> {
        ScriptUserData::<Self>::from_script_value(value, lua).map(|value| value.0)
    }
}

impl<T: FromScriptValue> FromScriptValue for Option<T> {
    fn from_script_value(source: Value, lua: &Lua) -> mlua::Result<Self> {
        if source.is_nil() || source == Value::NULL {
            return Ok(None);
        }
        value(lua, source).map(Some)
    }
}

impl<T: FromScriptValue> FromScriptValue for Vec<T> {
    fn from_script_value(source: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = collection(lua, source)?;
        table.sequence_values::<Value>().map(|entry| value(lua, entry?)).collect()
    }
}

impl<T: FromScriptValue, const N: usize> FromScriptValue for [T; N] {
    fn from_script_value(source: Value, lua: &Lua) -> mlua::Result<Self> {
        let entries = Vec::<T>::from_script_value(source, lua)?;
        let length = entries.len();
        entries.try_into().map_err(|_| mlua::Error::runtime(format!("expected {N} array entries, received {length}")))
    }
}

impl<T: FromScriptValue> FromScriptValue for Box<[T]> {
    fn from_script_value(source: Value, lua: &Lua) -> mlua::Result<Self> {
        Vec::<T>::from_script_value(source, lua).map(Vec::into_boxed_slice)
    }
}

impl<K: FromScriptValue + Ord, V: FromScriptValue> FromScriptValue for BTreeMap<K, V> {
    fn from_script_value(source: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = collection(lua, source)?;
        table
            .pairs::<Value, Value>()
            .map(|entry| {
                let (key, entry) = entry?;
                Ok((value(lua, key)?, value(lua, entry)?))
            })
            .collect()
    }
}

impl<K: FromScriptValue + Eq + Hash, V: FromScriptValue, S: BuildHasher + Default> FromScriptValue
    for HashMap<K, V, S>
{
    fn from_script_value(source: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = collection(lua, source)?;
        table
            .pairs::<Value, Value>()
            .map(|entry| {
                let (key, entry) = entry?;
                Ok((value(lua, key)?, value(lua, entry)?))
            })
            .collect()
    }
}

fn collection(lua: &Lua, source: Value) -> mlua::Result<Table> {
    ensure_owner(lua, &source)?;
    let table = Table::from_lua(source, lua)?;
    crate::frozen::backing(lua, &table)
}
