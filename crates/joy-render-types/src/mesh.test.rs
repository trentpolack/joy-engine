// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/mesh.rs`.

use super::*;

/// A two-triangle quad in two material slots, with every optional attribute.
fn quad() -> Mesh {
    Mesh {
        name: "quad".into(),
        positions: vec![DVec3::ZERO, DVec3::X, DVec3::new(1.0, 1.0, 0.0), DVec3::Y],
        normals: Some(vec![Vec3::Z; 4]),
        uvs: Some(vec![Vec2::ZERO, Vec2::X, Vec2::ONE, Vec2::Y]),
        colors: Some(vec![LinearColor::WHITE; 4]),
        indices: vec![0, 1, 2, 0, 2, 3],
        triangle_slots: Some(vec![0, 1]),
        material_slots: vec![MaterialSlot::named("front"), MaterialSlot::named("trim")],
        metadata: JsonRecord::new(),
    }
}

#[test]
fn meshes_round_trip_as_typed_arrays_and_omit_absent_attributes() {
    let mesh = quad();
    let json = serde_json::to_value(&mesh).unwrap();
    assert_eq!(json["positions"][2], serde_json::json!([1.0, 1.0, 0.0]));
    assert_eq!(json["colors"][0], serde_json::json!([1.0, 1.0, 1.0, 1.0]));
    assert_eq!(json["triangleSlots"], serde_json::json!([0, 1]));
    let parsed: Mesh = serde_json::from_value(json).unwrap();
    assert_eq!(parsed, mesh);

    // A bare mesh writes only its geometry and reads back with defaults.
    let bare = Mesh::new(mesh.positions.clone(), mesh.indices.clone());
    let json = serde_json::to_value(&bare).unwrap();
    assert_eq!(json.as_object().unwrap().keys().collect::<Vec<_>>(), ["positions", "indices"]);
    assert_eq!(serde_json::from_value::<Mesh>(json).unwrap(), bare);
}

#[test]
fn validation_reports_the_first_unusable_value() {
    assert_eq!(quad().validate(), Ok(()));
    assert_eq!(Mesh::default().validate(), Err(MeshError::Empty));
    assert_eq!(
        Mesh {
            indices: vec![0, 1],
            ..quad()
        }
        .validate(),
        Err(MeshError::IncompleteTriangle)
    );
    assert_eq!(
        Mesh {
            indices: vec![0, 1, 9],
            triangle_slots: None,
            ..quad()
        }
        .validate(),
        Err(MeshError::IndexOutOfRange {
            index: 9,
            vertex_count: 4
        })
    );
    assert_eq!(
        Mesh {
            uvs: Some(vec![Vec2::ZERO]),
            ..quad()
        }
        .validate(),
        Err(MeshError::Misaligned {
            attribute: MeshAttribute::Uvs,
            expected: 4,
            actual: 1
        })
    );
    let mut infinite = quad();
    infinite.positions[1].x = f64::INFINITY;
    assert_eq!(infinite.validate(), Err(MeshError::NonFinite(MeshAttribute::Positions)));
    assert_eq!(
        Mesh {
            triangle_slots: Some(vec![0, 2]),
            ..quad()
        }
        .validate(),
        Err(MeshError::SlotOutOfRange {
            slot: 2,
            slot_count: 2
        })
    );

    // Without named slots every triangle must use the one implicit slot.
    assert_eq!(
        Mesh {
            material_slots: Vec::new(),
            triangle_slots: Some(vec![0, 0]),
            ..quad()
        }
        .validate(),
        Ok(())
    );
}
