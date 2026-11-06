// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rust types that cross the Lua boundary; custom userdata supplies its description through `lua_userdata!`.

use super::LuaDefinitions;

/// The Lua-visible type of a Rust callback argument or result. Collection also discovers referenced userdata.
pub trait LuaType {
    /// Describe the type, adding any referenced classes to this definition set.
    fn lua_type(definitions: &mut LuaDefinitions) -> String;
}

/// The ordered values consumed or returned by a callback, including tuples and variadic arguments.
pub trait LuaTypes {
    /// Lua types in call order. A variadic tail is marked with `...`.
    fn lua_types(definitions: &mut LuaDefinitions) -> Vec<String>;
}

impl<T: LuaType> LuaTypes for T {
    fn lua_types(definitions: &mut LuaDefinitions) -> Vec<String> {
        vec![T::lua_type(definitions)]
    }
}

impl LuaTypes for () {
    fn lua_types(_: &mut LuaDefinitions) -> Vec<String> {
        Vec::new()
    }
}

impl<T: LuaType> LuaTypes for mlua::Variadic<T> {
    fn lua_types(definitions: &mut LuaDefinitions) -> Vec<String> {
        vec![format!("{}...", T::lua_type(definitions))]
    }
}

macro_rules! scalar_types {
    ($kind:literal: $($ty:ty),+ $(,)?) => { $(
        impl LuaType for $ty {
            fn lua_type(_: &mut LuaDefinitions) -> String { $kind.to_owned() }
        }
    )+ };
}
scalar_types!("number": f32, f64);
scalar_types!("integer": i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);
scalar_types!("boolean": bool);
scalar_types!("string": String, &str, mlua::LuaString);
scalar_types!("table": mlua::Table);
scalar_types!("any": mlua::Value);

impl<T: LuaType> LuaType for Option<T> {
    fn lua_type(definitions: &mut LuaDefinitions) -> String {
        format!("{}|nil", T::lua_type(definitions))
    }
}
impl<T: LuaType> LuaType for Vec<T> {
    fn lua_type(definitions: &mut LuaDefinitions) -> String {
        format!("({})[]", T::lua_type(definitions))
    }
}
impl<T: LuaType, const N: usize> LuaType for [T; N] {
    fn lua_type(definitions: &mut LuaDefinitions) -> String {
        Vec::<T>::lua_type(definitions)
    }
}

macro_rules! tuple_types {
    ($($ty:ident),+) => {
        impl<$($ty: LuaType),+> LuaTypes for ($($ty,)+) {
            fn lua_types(definitions: &mut LuaDefinitions) -> Vec<String> {
                vec![$($ty::lua_type(definitions)),+]
            }
        }
    };
}
tuple_types!(A);
tuple_types!(A, B);
tuple_types!(A, B, C);
tuple_types!(A, B, C, D);
tuple_types!(A, B, C, D, E);
tuple_types!(A, B, C, D, E, F);
