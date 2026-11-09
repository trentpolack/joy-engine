// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;
use crate::compile;
use serde_json::json;

/// A parameter, project metadata, two materials, and text and face attributes: every metadata path a game reads.
const ATTRIBUTED_SOURCE: &str = r#"
param width = 2 meta(min=1, max=8, step=0.5)
project meta(name="Crate")
let painted = color(box(width, 1, 1), rgba(0, 1, 0, 1))
let plain = translate(box(1, 1, 1), vector(3, 0, 0))
addAttribute(plain, "faces", "chunk", 2)
addAttribute(painted, "points", "label", "painted")
material(plain, materialPbr(rgba(0.5, 0.5, 0.5, 1), 0, 0.4))
output("Crate", merge(painted, plain))
"#;

#[test]
fn records_restore_the_evaluation_without_its_outputs_and_reject_damaged_bytes() {
    let geometry = compile(ATTRIBUTED_SOURCE, &json!({"width": 3.3}), None).unwrap();
    let record = encode_geometry(&geometry);
    let decoded = decode_geometry(&record).unwrap();

    // Everything but the named outputs survives exactly, and the mesh a renderer draws is identical.
    assert!(geometry.outputs.is_some(), "the fixture declares a named output");
    assert_eq!(
        decoded,
        GeometryData {
            outputs: None,
            ..geometry.clone()
        }
    );
    assert_eq!(decoded.to_mesh(), geometry.to_mesh());

    // Damaged or foreign bytes fail with a typed error instead of panicking or allocating their claimed length.
    assert_eq!(decode_geometry(b"nope"), Err(CookError::NotCooked));
    assert!(matches!(decode_geometry(&record[..record.len() - 1]), Err(CookError::Malformed(_))));
    let mut huge = record.clone();
    huge[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(decode_geometry(&huge), Err(CookError::Malformed(_))));
}
