// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use joy_core::PrimitiveShape;

use super::*;
use crate::BodyOptions;

fn piece(id: &str, center: DVec3) -> DestructibleChunkDefinition {
    DestructibleChunkDefinition {
        id: id.to_owned(),
        center,
        size: DVec3::splat(0.5),
        material: "concrete".to_owned(),
        hit_points: 1.0,
        shape: None,
    }
}

#[test]
fn falling_debris_strikes_each_target_once_and_exemptions_hold() {
    let mut debris = DebrisWorld::new(DebrisWorldOptions::default()).expect("debris world");
    debris
        .simulation_mut()
        .create_body(BodyOptions {
            position: Vec3::new(0.0, -1.0, 0.0),
            half_extents: Vec3::new(50.0, 1.0, 50.0),
            ..BodyOptions::new(PrimitiveShape::Box)
        })
        .expect("ground");

    // Two pieces fall onto the same target; the second is exempt (as if a crush rule already charged it).
    let release = DebrisRelease {
        velocity: Vec3::new(0.0, -6.0, 0.0),
        scale: Some(1.0),
        ..DebrisRelease::default()
    };
    let striker = |exempt: Vec<u64>| DebrisStriker {
        kilograms: 100.0,
        exempt,
    };
    debris.release(&piece("struck", DVec3::new(0.0, 3.0, 0.0)), release, Some(striker(Vec::new()))).unwrap();
    debris.release(&piece("exempt", DVec3::new(0.2, 3.0, 0.0)), release, Some(striker(vec![7]))).unwrap();

    let targets = [DebrisTarget {
        id: 7,
        strikeable: true,
        base: DVec3::ZERO,
        radius: 0.5,
        height: 2.0,
    }];
    let mut strikes = Vec::new();
    for _ in 0..60 {
        let step = debris.step(1.0 / 60.0);
        assert!(step.failure.is_none());
        strikes.extend(debris.strikes(&targets, 1.0));
    }
    assert_eq!(strikes.len(), 1, "one strike per piece and target, and none from the exempt piece");
    assert!(strikes[0].joules > 0.0);
    assert_eq!(debris.counts().moving + debris.counts().rubble, 2);
}
