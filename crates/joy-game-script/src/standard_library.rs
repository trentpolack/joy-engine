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
use mlua::Value;

use crate::ScriptRandom;
use crate::{
    LuaBindings,
    LuaDefinitions,
    LuaSignature,
};

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

crate::lua_userdata! {
    ScriptVec3, "Vec3",
    fields |fields| {
        fields.add_field_method_get("x", |_, vector| Ok(vector.0.x));
        fields.add_field_method_get("y", |_, vector| Ok(vector.0.y));
        fields.add_field_method_get("z", |_, vector| Ok(vector.0.z));
    }
    methods |methods| {
        methods.add_meta_method(MetaMethod::Add, LuaSignature::new(&["right"]), |_, left, right: Self| {
            Ok(Self(left.0 + right.0))
        });
        methods.add_meta_method(MetaMethod::Sub, LuaSignature::new(&["right"]), |_, left, right: Self| {
            Ok(Self(left.0 - right.0))
        });
        methods.add_meta_method(MetaMethod::Unm, LuaSignature::new(&[]), |_, vector, ()| Ok(Self(-vector.0)));
        methods.add_meta_function(
            MetaMethod::Mul,
            LuaSignature::new(&["left", "right"]).argument(0, "Vec3|number").argument(1, "Vec3|number"),
            |lua, (left, right): (Value, Value)| match (&left, &right) {
                (Value::UserData(_), Value::UserData(_)) => {
                    Ok(Self(Self::from_lua(left, lua)?.0 * Self::from_lua(right, lua)?.0))
                }
                (Value::UserData(_), _) => Ok(Self(Self::from_lua(left, lua)?.0 * f64::from_lua(right, lua)?)),
                _ => Ok(Self(f64::from_lua(left, lua)? * Self::from_lua(right, lua)?.0)),
            },
        );
        methods.add_meta_method(
            MetaMethod::Div,
            LuaSignature::new(&["right"]).argument(0, "Vec3|number"),
            |lua, left, right: Value| {
                if matches!(right, Value::UserData(_)) {
                    Ok(Self(left.0 / Self::from_lua(right, lua)?.0))
                } else {
                    Ok(Self(left.0 / f64::from_lua(right, lua)?))
                }
            },
        );
        methods.add_meta_method(MetaMethod::Eq, LuaSignature::new(&["right"]), |_, left, right: Self| Ok(left == &right));
        methods.add_meta_method(MetaMethod::ToString, LuaSignature::new(&[]), |_, vector, ()| {
            Ok(format!("vec3({}, {}, {})", vector.0.x, vector.0.y, vector.0.z))
        });
        methods.add_method("length", LuaSignature::new(&[]), |_, vector, ()| Ok(vector.0.length()));
        methods.add_method("length_squared", LuaSignature::new(&[]), |_, vector, ()| Ok(vector.0.length_squared()));
        methods
            .add_method("distance", LuaSignature::new(&["other"]), |_, vector, other: Self| Ok(vector.0.distance(other.0)));
        methods.add_method("dot", LuaSignature::new(&["other"]), |_, vector, other: Self| Ok(vector.0.dot(other.0)));
        methods
            .add_method("cross", LuaSignature::new(&["other"]), |_, vector, other: Self| Ok(Self(vector.0.cross(other.0))));
        methods.add_method("lerp", LuaSignature::new(&["other", "amount"]), |_, vector, (other, amount): (Self, f64)| {
            Ok(Self(vector.0.lerp(other.0, amount)))
        });
        methods.add_method("normalize", LuaSignature::new(&[]), |_, vector, ()| Ok(Self(vector.0.normalize())));
        methods.add_method("normalize_or_zero", LuaSignature::new(&[]), |_, vector, ()| {
            Ok(Self(vector.0.normalize_or_zero()))
        });
    }
}

pub(crate) fn register(lua: &Lua, definitions: &mut LuaDefinitions) -> mlua::Result<()> {
    let globals = lua.globals();
    definitions.record("JoyNull", &[]);
    let mut bindings = LuaBindings::new(lua, definitions);
    // Only library-named, in-memory imports are installed later by ScriptLibrary. Environment access and
    // dynamic loaders would bypass both private globals and the source-level portable-power restriction.
    for name in ["os", "io", "package", "require", "load", "dofile", "loadfile", "collectgarbage", "coroutine", "debug"]
    {
        bindings.remove(name)?;
    }
    bindings.remove("_G")?;
    bindings.constant("null", "JoyNull", lua.null())?;
    bindings.function("print", LuaSignature::new(&["..."]), |_, values: mlua::Variadic<Value>| {
        log::info!("[script] {}", values.iter().map(|value| format!("{value:?}")).collect::<Vec<_>>().join(" "));
        Ok(())
    })?;
    bindings.function(
        "vec3",
        LuaSignature::new(&["x", "y", "z"]).doc("Immutable f64 world vector; arithmetic returns a new value."),
        |_, (x, y, z): (f64, f64, f64)| Ok(ScriptVec3(DVec3::new(x, y, z))),
    )?;
    bindings.function("seeded_random", LuaSignature::new(&["seed"]), |_, seed: i64| Ok(ScriptRandom::seeded(seed)))?;
    register_math(lua, definitions)?;
    for name in ["math", "string", "table", "utf8"] {
        if let Value::Table(table) = globals.raw_get::<Value>(name)? {
            crate::frozen::freeze(lua, &table)?;
        }
    }
    Ok(())
}

fn register_math(lua: &Lua, definitions: &mut LuaDefinitions) -> mlua::Result<()> {
    let mut bindings = LuaBindings::new(lua, definitions);
    for name in ["random", "randomseed", "noise"] {
        bindings.remove(&format!("math.{name}"))?;
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
        bindings.function(name, LuaSignature::new(&["value"]), move |_, value: f64| Ok(function(value)))?;
        // Lua's non-transcendental functions retain integer and variadic semantics. Global engine helpers
        // deliberately use f64, while the standard math namespace keeps the ordinary Lua contracts.
        if !matches!(name, "sqrt" | "floor" | "ceil" | "abs") {
            bindings.alias(&format!("math.{name}"), name)?;
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
        bindings.function(name, LuaSignature::new(&["left", "right"]), move |_, (left, right): (f64, f64)| {
            Ok(function(left, right))
        })?;
        if !matches!(name, "min" | "max" | "fmod") {
            bindings.alias(&format!("math.{name}"), name)?;
        }
    }
    bindings.function("math.atan", LuaSignature::new(&["y", "x"]), |_, (y, x): (f64, Option<f64>)| {
        Ok(match x {
            Some(x) => deterministic::atan2(y, x),
            None => deterministic::atan(y),
        })
    })?;
    bindings.function("math.log", LuaSignature::new(&["value", "base"]), |_, (value, base): (f64, Option<f64>)| {
        Ok(match base {
            Some(base) => deterministic::ln(value) / deterministic::ln(base),
            None => deterministic::ln(value),
        })
    })?;
    bindings.function("powi", LuaSignature::new(&["base", "exponent"]), |_, (base, exponent): (f64, i64)| {
        Ok(deterministic::powi(base, exponent.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32))
    })?;
    bindings.function(
        "clamp",
        LuaSignature::new(&["value", "minimum", "maximum"]),
        |_, (value, minimum, maximum): (f64, f64, f64)| Ok(clamp(value, minimum, maximum)),
    )?;
    bindings.function(
        "lerp",
        LuaSignature::new(&["from", "to", "amount"]),
        |_, (from, to, amount): (f64, f64, f64)| Ok(from + ((to - from) * amount)),
    )?;
    bindings.function("PI", LuaSignature::new(&[]), |_, ()| Ok(PI))?;
    bindings.function("TAU", LuaSignature::new(&[]), |_, ()| Ok(TAU))?;
    bindings.function("to_float", LuaSignature::new(&["value"]), |_, value: f64| Ok(value))?;
    bindings.function("to_int", LuaSignature::new(&["value"]), |_, value: f64| Ok(value as i64))?;
    Ok(())
}
