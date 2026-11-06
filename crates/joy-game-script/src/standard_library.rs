// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Portable scalar math, immutable f64 vectors, and explicit seeded random streams.

use std::f64::consts::PI;
use std::f64::consts::TAU;

use joy_core::math::DVec3;
use joy_core::math::clamp;
use joy_core::math::deterministic;
use mlua::FromLua;
use mlua::Lua;
use mlua::LuaSerdeExt;
use mlua::MetaMethod;
use mlua::Table;
use mlua::UserData;
use mlua::UserDataFields;
use mlua::UserDataMethods;
use mlua::Value;

use crate::ScriptRandom;

type UnaryMath = fn(f64) -> f64;
type BinaryMath = fn(f64, f64) -> f64;

/// An immutable Y-up world vector computed in `f64`, with immutable components. Arithmetic returns new
/// userdata; assigning one vector to another field never creates a mutable alias. Rebuild a vector to edit components.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScriptVec3(pub DVec3);

impl FromLua for ScriptVec3 {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        crate::engine::ensure_owner(lua, &value)?;
        if let Value::UserData(userdata) = &value {
            return Ok(*userdata.borrow::<Self>()?);
        }
        Err(mlua::Error::FromLuaConversionError {
            from: value.type_name(),
            to: "vec3".into(),
            message: Some("expected an f64 vec3 userdata".into()),
        })
    }
}

impl UserData for ScriptVec3 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("x", |_, vector| Ok(vector.0.x));
        fields.add_field_method_get("y", |_, vector| Ok(vector.0.y));
        fields.add_field_method_get("z", |_, vector| Ok(vector.0.z));
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Add, |_, left, right: Self| Ok(Self(left.0 + right.0)));
        methods.add_meta_method(MetaMethod::Sub, |_, left, right: Self| Ok(Self(left.0 - right.0)));
        methods.add_meta_method(MetaMethod::Unm, |_, vector, ()| Ok(Self(-vector.0)));
        methods.add_meta_function(MetaMethod::Mul, |lua, (left, right): (Value, Value)| match (&left, &right) {
            (Value::UserData(_), Value::UserData(_)) => {
                Ok(Self(Self::from_lua(left, lua)?.0 * Self::from_lua(right, lua)?.0))
            }
            (Value::UserData(_), _) => Ok(Self(Self::from_lua(left, lua)?.0 * f64::from_lua(right, lua)?)),
            _ => Ok(Self(f64::from_lua(left, lua)? * Self::from_lua(right, lua)?.0)),
        });
        methods.add_meta_method(MetaMethod::Div, |lua, left, right: Value| {
            if matches!(right, Value::UserData(_)) {
                Ok(Self(left.0 / Self::from_lua(right, lua)?.0))
            } else {
                Ok(Self(left.0 / f64::from_lua(right, lua)?))
            }
        });
        methods.add_meta_method(MetaMethod::Eq, |_, left, right: Self| Ok(left == &right));
        methods.add_meta_method(MetaMethod::ToString, |_, vector, ()| {
            Ok(format!("vec3({}, {}, {})", vector.0.x, vector.0.y, vector.0.z))
        });
        methods.add_method("length", |_, vector, ()| Ok(vector.0.length()));
        methods.add_method("length_squared", |_, vector, ()| Ok(vector.0.length_squared()));
        methods.add_method("distance", |_, vector, other: Self| Ok(vector.0.distance(other.0)));
        methods.add_method("dot", |_, vector, other: Self| Ok(vector.0.dot(other.0)));
        methods.add_method("cross", |_, vector, other: Self| Ok(Self(vector.0.cross(other.0))));
        methods.add_method("lerp", |_, vector, (other, amount): (Self, f64)| Ok(Self(vector.0.lerp(other.0, amount))));
        methods.add_method("normalize", |_, vector, ()| Ok(Self(vector.0.normalize())));
        methods.add_method("normalize_or_zero", |_, vector, ()| Ok(Self(vector.0.normalize_or_zero())));
    }
}

pub(crate) fn register(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    // Only library-named, in-memory imports are installed later by ScriptLibrary. Environment access and
    // dynamic loaders would bypass both private globals and the source-level portable-power restriction.
    for name in ["os", "io", "package", "require", "load", "dofile", "loadfile", "collectgarbage", "coroutine", "debug"]
    {
        globals.raw_set(name, Value::Nil)?;
    }
    globals.raw_set("_G", Value::Nil)?;
    globals.raw_set("null", lua.null())?;
    globals.raw_set(
        "print",
        lua.create_function(|_, values: mlua::Variadic<Value>| {
            log::info!("[script] {}", values.iter().map(|value| format!("{value:?}")).collect::<Vec<_>>().join(" "));
            Ok(())
        })?,
    )?;
    register_math(lua)?;
    globals
        .raw_set("vec3", lua.create_function(|_, (x, y, z): (f64, f64, f64)| Ok(ScriptVec3(DVec3::new(x, y, z))))?)?;
    globals.raw_set("seeded_random", lua.create_function(|_, seed: i64| Ok(ScriptRandom::seeded(seed)))?)?;
    for name in ["math", "string", "table", "utf8"] {
        if let Value::Table(table) = globals.raw_get::<Value>(name)? {
            crate::frozen::freeze(lua, &table)?;
        }
    }
    Ok(())
}

fn register_math(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    let math = globals.raw_get::<Table>("math")?;
    for name in ["random", "randomseed", "noise"] {
        math.raw_set(name, Value::Nil)?;
    }
    let unary: &[(&str, UnaryMath)] = &[
        ("sinh", deterministic::sinh),
        ("cosh", deterministic::cosh),
        ("tanh", deterministic::tanh),
        ("sin", deterministic::sin),
        ("cos", deterministic::cos),
        ("tan", deterministic::tan),
        ("asin", deterministic::asin),
        ("acos", deterministic::acos),
        ("atan", deterministic::atan),
        ("exp", deterministic::exp),
        ("ln", deterministic::ln),
        ("log2", deterministic::log2),
        ("log10", deterministic::log10),
        ("sqrt", f64::sqrt),
        ("floor", f64::floor),
        ("ceil", f64::ceil),
        ("round", f64::round),
        ("trunc", f64::trunc),
        ("fract", f64::fract),
        ("abs", f64::abs),
    ];
    for &(name, function) in unary {
        let binding = lua.create_function(move |_, value: f64| Ok(function(value)))?;
        globals.raw_set(name, binding.clone())?;
        // Lua's non-transcendental functions retain integer and variadic semantics. Global engine helpers
        // deliberately use f64, while the standard math namespace keeps the ordinary Lua contracts.
        if !matches!(name, "sqrt" | "floor" | "ceil" | "abs") {
            math.raw_set(name, binding)?;
        }
    }
    let binary: &[(&str, BinaryMath)] = &[
        ("atan2", deterministic::atan2),
        ("powf", deterministic::powf),
        ("pow", deterministic::powf),
        ("hypot", deterministic::hypot),
        ("min", f64::min),
        ("max", f64::max),
        ("fmod", |left, right| left % right),
    ];
    for &(name, function) in binary {
        let binding = lua.create_function(move |_, (left, right): (f64, f64)| Ok(function(left, right)))?;
        globals.raw_set(name, binding.clone())?;
        if !matches!(name, "min" | "max" | "fmod") {
            math.raw_set(name, binding)?;
        }
    }
    math.raw_set(
        "atan",
        lua.create_function(|_, (y, x): (f64, Option<f64>)| {
            Ok(match x {
                Some(x) => deterministic::atan2(y, x),
                None => deterministic::atan(y),
            })
        })?,
    )?;
    math.raw_set(
        "log",
        lua.create_function(|_, (value, base): (f64, Option<f64>)| {
            Ok(match base {
                Some(base) => deterministic::ln(value) / deterministic::ln(base),
                None => deterministic::ln(value),
            })
        })?,
    )?;
    globals.raw_set(
        "powi",
        lua.create_function(|_, (base, exponent): (f64, i64)| {
            Ok(deterministic::powi(base, exponent.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32))
        })?,
    )?;
    globals.raw_set(
        "clamp",
        lua.create_function(|_, (value, minimum, maximum): (f64, f64, f64)| Ok(clamp(value, minimum, maximum)))?,
    )?;
    globals.raw_set(
        "lerp",
        lua.create_function(|_, (from, to, amount): (f64, f64, f64)| Ok(from + ((to - from) * amount)))?,
    )?;
    globals.raw_set("PI", lua.create_function(|_, ()| Ok(PI))?)?;
    globals.raw_set("TAU", lua.create_function(|_, ()| Ok(TAU))?)?;
    globals.raw_set("to_float", lua.create_function(|_, value: f64| Ok(value))?)?;
    globals.raw_set("to_int", lua.create_function(|_, value: f64| Ok(value as i64))?)?;
    Ok(())
}
