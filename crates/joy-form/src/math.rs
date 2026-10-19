// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scalar and vector arithmetic, built-in math functions, and deterministic noise.

use crate::value::{
    Value,
    arity,
};
use joy_core::math::deterministic;

pub fn arithmetic(operator: &str, left: &Value, right: &Value) -> Result<Value, String> {
    if let (Value::Number(a), Value::Number(b)) = (left, right) {
        let result = match operator {
            "==" => f64::from(a == b),
            "!=" => f64::from(a != b),
            "<" => f64::from(a < b),
            ">" => f64::from(a > b),
            "<=" => f64::from(a <= b),
            ">=" => f64::from(a >= b),
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => a / b,
            "%" => a % b,
            _ => deterministic::powf(*a, *b),
        };
        return Ok(Value::Number(result));
    }
    if matches!(left, Value::Array(_)) || matches!(right, Value::Array(_)) {
        if !["+", "-", "*", "/"].contains(&operator) {
            return Err("Vector arithmetic supports +, -, *, and /; compare scalar components.".into());
        }
        let size = match (left, right) {
            (Value::Array(a), Value::Array(b)) => {
                if a.len() != b.len() {
                    return Err("Vector/color arithmetic requires matching component counts.".into());
                }
                a.len()
            }
            (Value::Array(a), _) => a.len(),
            (_, Value::Array(b)) => b.len(),
            _ => unreachable!(),
        };
        let mut result = Vec::with_capacity(size);
        for index in 0..size {
            let a = if let Value::Array(v) = left { v[index] } else { left.scalar()? };
            let b = if let Value::Array(v) = right { v[index] } else { right.scalar()? };
            result.push(match operator {
                "+" => a + b,
                "-" => a - b,
                "*" => a * b,
                _ => a / b,
            });
        }
        return Ok(Value::Array(result));
    }
    left.scalar()?;
    right.scalar()?;
    unreachable!()
}

pub fn vector_function(name: &str, values: &[Value]) -> Result<Option<Value>, String> {
    if name == "mix" && values.iter().take(2).any(|v| matches!(v, Value::Array(_))) {
        if values.len() != 3 {
            return Err("mix expects two values and a scalar weight.".into());
        }
        let delta = arithmetic("-", &values[1], &values[0])?;
        let scaled = arithmetic("*", &delta, &Value::Number(values[2].scalar()?))?;
        return Ok(Some(arithmetic("+", &values[0], &scaled)?));
    }
    if !["length", "normalize", "dot", "cross"].contains(&name) {
        return Ok(None);
    }
    let count = if ["dot", "cross"].contains(&name) { 2 } else { 1 };
    if values.len() != count || values.iter().any(|v| !matches!(v,Value::Array(a) if a.len()==3)) {
        return Err(format!("{name} expects {count} XYZ vector{}.", if count > 1 { "s" } else { "" }));
    }
    let a = values[0].vector()?;
    if name == "length" {
        return Ok(Some(Value::Number(hypot(&a))));
    }
    if name == "normalize" {
        let scale = a.iter().map(|n| n.abs()).fold(0.0, f64::max);
        if scale == 0.0 {
            return Err("normalize expects a nonzero vector.".into());
        }
        let scaled: Vec<f64> = a.iter().map(|n| n / scale).collect();
        let length = hypot(&scaled);
        return Ok(Some(Value::Array(scaled.iter().map(|n| n / length).collect())));
    }
    let b = values[1].vector()?;
    if name == "dot" {
        return Ok(Some(Value::Number(a.iter().zip(b).fold(0.0, |sum, (a, b)| sum + a * b))));
    }
    Ok(Some(Value::Array(vec![a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]])))
}

pub fn function(name: &str, args: &[f64]) -> Result<Option<f64>, String> {
    let count = match name {
        "sin" | "cos" | "tan" | "abs" | "sqrt" | "floor" | "ceil" | "round" | "exp" | "log" => 1,
        "min" | "max" | "pow" | "atan2" => 2,
        "noise" | "smoothstep" | "clamp" | "mix" => 3,
        _ => return Ok(None),
    };
    arity(name, args.len(), count)?;
    let a = args[0];
    let result = match name {
        "sin" => deterministic::sin(a),
        "cos" => deterministic::cos(a),
        "tan" => deterministic::tan(a),
        "abs" => a.abs(),
        "sqrt" => a.sqrt(),
        "floor" => a.floor(),
        "ceil" => a.ceil(),
        "round" => {
            if (-0.5..0.0).contains(&a) {
                -0.0
            } else {
                let lower = a.floor();
                if a - lower < 0.5 { lower } else { lower + 1.0 }
            }
        }
        "exp" => deterministic::exp(a),
        "log" => deterministic::ln(a),
        "min" => a.min(args[1]),
        "max" => a.max(args[1]),
        "pow" => deterministic::powf(a, args[1]),
        "atan2" => deterministic::atan2(a, args[1]),
        "noise" => noise(a, args[1], args[2]),
        "smoothstep" => {
            if args[1] <= a {
                return Err("smoothstep requires low < high.".into());
            }
            let t = ((args[2] - a) / (args[1] - a)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
        "clamp" => a.min(args[2]).max(args[1]),
        "mix" => a + (args[1] - a) * args[2],
        _ => unreachable!(),
    };
    Ok(Some(result))
}

pub fn to_u32(value: f64) -> u32 {
    let modulo = value.trunc() % 4_294_967_296.0;
    (if modulo < 0.0 { modulo + 4_294_967_296.0 } else { modulo }) as u32
}

pub fn noise(x: f64, y: f64, z: f64) -> f64 {
    let coordinates = [x, y, z];
    let base = coordinates.map(f64::floor);
    let blend: [f64; 3] = std::array::from_fn(|axis| {
        let t = coordinates[axis] - base[axis];
        t * t * (3.0 - 2.0 * t)
    });
    let mut result = 0.0;
    for corner in 0..8 {
        let dx = corner & 1;
        let dy = (corner >> 1) & 1;
        let dz = (corner >> 2) & 1;
        let mut hash = to_u32(base[0] + dx as f64).wrapping_mul(374761393)
            ^ to_u32(base[1] + dy as f64).wrapping_mul(668265263)
            ^ to_u32(base[2] + dz as f64).wrapping_mul(1442695041);
        hash = (hash ^ (hash >> 13)).wrapping_mul(1274126177);
        let sample = (hash ^ (hash >> 16)) as f64 / 4294967295.0 * 2.0 - 1.0;
        result += sample
            * (if dx != 0 { blend[0] } else { 1.0 - blend[0] })
            * (if dy != 0 { blend[1] } else { 1.0 - blend[1] })
            * (if dz != 0 { blend[2] } else { 1.0 - blend[2] });
    }
    result.clamp(-1.0, 1.0)
}

pub fn hypot(values: &[f64]) -> f64 {
    let scale = values.iter().map(|v| v.abs()).fold(0.0, f64::max);
    if scale == 0.0 {
        return 0.0;
    }
    values.iter().map(|v| deterministic::powi(v / scale, 2)).sum::<f64>().sqrt() * scale
}
