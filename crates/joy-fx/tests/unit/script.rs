// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/script.rs`.

use super::*;
use joy_core::SeededRandom;

fn run(source: &str, values: &[(&str, f64)]) -> Result<IndexMap<String, f64>, ParticleScriptError> {
    let script = ParticleScript::compile(source, &[], &[])?;
    let mut record: IndexMap<String, f64> = values.iter().map(|(name, value)| ((*name).to_owned(), *value)).collect();
    let mut budget = ParticleScriptBudget { remaining: 1_000 };
    script.run(&mut record, &mut SeededRandom::new(1), &mut budget)?;
    Ok(record)
}

#[test]
fn numbers_tokenize_like_javascript() {
    assert_eq!(parse_number("1."), 1.0);
    assert_eq!(parse_number("1.e2"), 100.0);
    assert_eq!(parse_number(".5"), 0.5);
    assert_eq!(parse_number("2E-1"), 0.2);
    assert_eq!(match_token(b"1e"), Some(1));
    assert_eq!(match_token(b"1e+5x"), Some(4));
    assert_eq!(match_token(b"..5"), Some(1));
    assert_eq!(match_token(b"<=1"), Some(2));
    assert_eq!(match_token(b"&x"), None);
}

#[test]
fn precedence_laziness_and_containers_follow_the_reference() {
    let record =
        run("u0 = -2^2; u1 = 1/0 && 1; u2 = 0 || 3; let v = vector(1, 2, 3); position = v * 2; vx = position.y;", &[])
            .unwrap();
    assert_eq!(record["u0"], 4.0);
    assert_eq!(record["u1"], 1.0);
    assert_eq!(record["u2"], 1.0);
    assert_eq!(record["vx"], 4.0);
    assert_eq!(record["v.z"], 3.0);
}

#[test]
fn diagnostics_carry_locations() {
    let error = ParticleScript::compile("x = 1;\n  y = foo", &[], &[]).unwrap_err();
    assert_eq!(error.to_string(), "Unknown numeric value 'foo' or field is not writable (line 2, column 7)");
    let error = run("x = 1;\nsize /= 0;", &[("size", 1.0), ("x", 0.0)]).unwrap_err();
    assert_eq!((error.line, error.column), (2, 1));
    let end = ParticleScript::compile("x = 1 +", &[], &[]).unwrap_err();
    assert_eq!(end.message, "Unexpected end of script");
}
