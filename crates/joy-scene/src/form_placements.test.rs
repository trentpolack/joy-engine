// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;
use serde_json::json;

const SOURCE: &str = r#"
    with userData({kind: "tree", object: "assets/objects/tree.joyobject", scale: 2}) { point(1, 0, 0) }
    with userData({kind: "spawn"}) { point(0, 0, 3) }
    point(5, 5, 5)
"#;

#[test]
fn tagged_points_become_placements_and_objects_become_linked_world_entities() {
    let geometry = joy_form::compile(SOURCE, &json!({}), None).unwrap();
    let placements = form_placements(&geometry);
    assert_eq!(placements.len(), 2);
    assert_eq!((placements[0].kind(), placements[0].object()), (Some("tree"), Some("assets/objects/tree.joyobject")));
    assert_eq!((placements[1].kind(), placements[1].object()), (Some("spawn"), None));
    assert_eq!(placements[1].position, Vec3::new(0.0, 0.0, 3.0));

    let mut parent = EntityDefinition::new("grove");
    parent.transform.position = Vec3::new(10.0, 0.0, 0.0);
    parent.transform.rotation = Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0);
    parent.transform.scale = Vec3::splat(3.0);
    let entities = placement_entities(&parent, &placements).unwrap();
    assert_eq!(entities.len(), 1);
    let tree = &entities[0];
    assert_eq!(tree.id, "grove/0");
    assert_eq!(tree.template.as_deref(), Some("assets/objects/tree.joyobject"));
    assert_eq!(tree.overrides.as_ref().unwrap()["components"][PLACEMENT_COMPONENT]["kind"], "tree");
    // A quarter turn about Y carries local +X to world -Z; parent scale applies to the offset and the instance.
    assert!((tree.transform.position - Vec3::new(10.0, 0.0, -3.0)).length() < 1e-5);
    assert!((tree.transform.scale - Vec3::splat(6.0)).length() < 1e-6);
}
