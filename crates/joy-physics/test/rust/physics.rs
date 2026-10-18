// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Physics behavior the TypeScript suite protects: resting contact, bounded fixed stepping, ownership, raycast
//! vehicles, transform conventions, and determinism, plus signed distance field colliders.

use glam::Vec3;
use joy_core::{
    PrimitiveShape,
    quat_from_euler,
};
use joy_physics::{
    BodyHandle,
    BodyOptions,
    PhysicsError,
    PhysicsWorld,
    PhysicsWorldOptions,
    SdfBakeOptions,
    SdfPrimitive,
    SdfShape,
    VehicleHandle,
    VehicleInput,
    VehicleOptions,
    VehicleTuning,
    WheelOptions,
};

/// Advance in 60 Hz frames for a number of seconds.
fn simulate(world: &mut PhysicsWorld, seconds: f32) {
    let frames = (seconds * 60.0).round() as usize;
    for _ in 0..frames {
        world.step(1.0 / 60.0).unwrap();
    }
}

/// A large static ground box whose top face is at y = 0.
fn ground(world: &mut PhysicsWorld) -> BodyHandle {
    world
        .create_body(BodyOptions {
            half_extents: Vec3::new(50.0, 0.5, 50.0),
            position: Vec3::new(0.0, -0.5, 0.0),
            friction: 0.6,
            ..BodyOptions::new(PrimitiveShape::Box)
        })
        .unwrap()
}

/// The TypeScript suite's four-wheel test vehicle: front wheels steer, every wheel drives and brakes.
fn make_vehicle(world: &mut PhysicsWorld) -> (BodyHandle, VehicleHandle) {
    let chassis = world
        .create_body(BodyOptions {
            mass: 180.0,
            half_extents: Vec3::new(0.8, 0.25, 1.25),
            position: Vec3::new(0.0, 1.0, 0.0),
            ..BodyOptions::new(PrimitiveShape::Box)
        })
        .unwrap();
    let mut wheels = Vec::new();
    for z in [1.0, -1.0] {
        for x in [-0.8, 0.8] {
            wheels.push(WheelOptions {
                radius: 0.35,
                steering: z > 0.0,
                driven: true,
                brake: true,
                ..WheelOptions::new(Vec3::new(x, 0.0, z))
            });
        }
    }
    let options =
        VehicleOptions { wheels, engine_force: 400.0, max_steer: 0.4, brake_force: 20.0, ..Default::default() };
    let vehicle = world.create_vehicle(chassis, options).unwrap();
    (chassis, vehicle)
}

/// An axis-aligned box mesh of `half` extents centered on the origin, optionally without its top face (an open crate).
fn box_mesh(half: Vec3, open_top: bool) -> (Vec<Vec3>, Vec<u32>) {
    let corner = |index: u32| {
        let sign = |bit: u32| if index & bit == 0 { -1.0 } else { 1.0 };
        Vec3::new(sign(1), sign(2), sign(4)) * half
    };
    let positions = (0..8).map(corner).collect();
    // Faces as quads (outward winding): -X, +X, -Y, +Y, -Z, +Z.
    let faces: [[u32; 4]; 6] = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
    let mut indices = Vec::new();
    for (face, [a, b, c, d]) in faces.into_iter().enumerate() {
        if open_top && face == 3 {
            continue;
        }
        indices.extend([a, b, c, a, c, d]);
    }
    (positions, indices)
}

#[test]
fn dynamic_box_rests_on_static_ground_and_queries_copy_state() {
    let mut world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    let floor = ground(&mut world);
    let crate_box = world
        .create_body(BodyOptions {
            mass: 2.0,
            position: Vec3::new(0.0, 4.0, 0.0),
            ..BodyOptions::new(PrimitiveShape::Box)
        })
        .unwrap();
    let ball = world
        .create_body(BodyOptions {
            mass: 1.0,
            position: Vec3::new(2.0, 3.0, 0.0),
            ..BodyOptions::new(PrimitiveShape::Sphere)
        })
        .unwrap();
    simulate(&mut world, 3.0);

    // Both bodies come to rest on the ground's top face; the static ground never moves.
    assert!((world.body_transform(crate_box).unwrap().position.y - 0.5).abs() < 0.05);
    assert!((world.body_transform(ball).unwrap().position.y - 0.5).abs() < 0.05);
    assert_eq!(world.body_transform(floor).unwrap().position, Vec3::new(0.0, -0.5, 0.0));
    assert_eq!(world.body_mass(floor), Some(0.0));
    assert!((world.body_mass(crate_box).unwrap() - 2.0).abs() < 1e-5);

    // Ray queries return the closest body with an upward normal, and miss empty space.
    let hit = world.raycast(Vec3::new(0.0, 5.0, 0.0), Vec3::new(0.0, -1.0, 0.0)).unwrap();
    assert_eq!(hit.body, crate_box);
    assert!(hit.normal.y > 0.9);
    assert!((hit.distance - 4.0).abs() < 0.06);
    assert!(world.raycast(Vec3::new(80.0, 2.0, 0.0), Vec3::new(80.0, 1.0, 0.0)).is_none());

    // Impulses change velocity immediately; teleports keep velocity and are visible to queries at once.
    world.apply_impulse(crate_box, Vec3::new(2.0, 0.0, 0.0), None).unwrap();
    assert!(world.body_velocity(crate_box).unwrap().x > 0.9);
    world.set_body_transform(crate_box, Some(Vec3::new(3.0, 2.0, 1.0)), Some(Vec3::new(0.3, 0.2, 0.1))).unwrap();
    let restored = world.body_transform(crate_box).unwrap();
    assert!((restored.rotation - Vec3::new(0.3, 0.2, 0.1)).abs().max_element() < 1e-5);
    assert!(world.body_velocity(crate_box).unwrap().x > 0.9);
    let moved = world.raycast(Vec3::new(3.0, 5.0, 1.0), Vec3::new(3.0, 0.5, 1.0)).unwrap();
    assert_eq!(moved.body, crate_box);
}

#[test]
fn fixed_stepping_bounds_catch_up_and_rejects_invalid_configuration() {
    let options = PhysicsWorldOptions { gravity: Vec3::ZERO, fixed_step: 1.0 / 60.0, max_sub_steps: 3 };
    let mut world = PhysicsWorld::new(options).unwrap();
    let body =
        world.create_body(BodyOptions { radius: 1.0, mass: 1.0, ..BodyOptions::new(PrimitiveShape::Sphere) }).unwrap();
    world.set_body_velocity(body, Vec3::new(0.0, 0.0, 1.0)).unwrap();
    assert_eq!(world.step(1.0 / 120.0).unwrap(), 0);
    assert_eq!(world.step(1.0 / 120.0).unwrap(), 1);
    assert_eq!(world.step(100_000.0).unwrap(), 3);
    assert_eq!(world.step(0.0).unwrap(), 0, "excess catch-up time is discarded");
    assert!(world.body_transform(body).unwrap().position.z < 0.07);

    for value in [f32::NAN, f32::INFINITY, -1.0] {
        assert!(world.step(value).is_err());
        assert!(world.create_body(BodyOptions { mass: value, ..BodyOptions::new(PrimitiveShape::Sphere) }).is_err());
    }
    let flat = BodyOptions { half_extents: Vec3::new(0.0, 1.0, 1.0), ..BodyOptions::new(PrimitiveShape::Box) };
    assert_eq!(world.create_body(flat), Err(PhysicsError::NonPositiveHalfExtents));
    let heavy_plane = BodyOptions { mass: 1.0, ..BodyOptions::new(PrimitiveShape::Plane) };
    assert_eq!(world.create_body(heavy_plane), Err(PhysicsError::DynamicPlane));
    assert!(PhysicsWorld::new(PhysicsWorldOptions { fixed_step: 0.0, ..Default::default() }).is_err());
    assert!(PhysicsWorld::new(PhysicsWorldOptions { max_sub_steps: 1_000_000, ..Default::default() }).is_err());
    assert!(world.set_body_transform(body, Some(Vec3::new(0.0, f32::INFINITY, 0.0)), None).is_err());
    assert!(world.apply_force(body, Vec3::new(0.0, f32::NAN, 0.0), None).is_err());
    assert_eq!(world.body_count(), 1);
}

#[test]
fn forces_apply_for_exactly_one_fixed_step() {
    let options = PhysicsWorldOptions { gravity: Vec3::ZERO, ..Default::default() };
    let mut world = PhysicsWorld::new(options).unwrap();
    let body = world
        .create_body(BodyOptions { mass: 1.0, linear_damping: 0.0, ..BodyOptions::new(PrimitiveShape::Sphere) })
        .unwrap();
    world.apply_force(body, Vec3::new(60.0, 0.0, 0.0), None).unwrap();
    simulate(&mut world, 0.5);

    // 60 N on 1 kg for one 1/60 s step adds 1 m/s, and later steps add nothing.
    let velocity = world.body_velocity(body).unwrap();
    assert!((velocity.x - 1.0).abs() < 1e-3, "{velocity}");
}

#[test]
fn ownership_rejects_foreign_static_and_attached_chassis_and_removal_cascades() {
    let mut first = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    let mut second = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    let (chassis, vehicle) = make_vehicle(&mut first);
    let wheels = vec![WheelOptions::new(Vec3::ZERO)];
    let one_wheel = VehicleOptions { wheels: wheels.clone(), ..Default::default() };

    // Foreign, already attached, and static chassis are all rejected; foreign handles never alias.
    assert_eq!(second.create_vehicle(chassis, one_wheel.clone()), Err(PhysicsError::InvalidChassis));
    assert_eq!(first.create_vehicle(chassis, one_wheel.clone()), Err(PhysicsError::InvalidChassis));
    let floor = ground(&mut first);
    assert_eq!(first.create_vehicle(floor, one_wheel.clone()), Err(PhysicsError::InvalidChassis));
    assert!(second.body_transform(chassis).is_none());
    assert_eq!(first.wheel_transforms(vehicle).len(), 4);

    // Invalid wheel configuration leaves the chassis unattached.
    assert!(first.remove_vehicle(vehicle));
    assert!(!first.remove_vehicle(vehicle));
    let invalid = [
        VehicleOptions {
            wheels: vec![WheelOptions { radius: f32::NAN, ..WheelOptions::new(Vec3::ZERO) }],
            ..Default::default()
        },
        VehicleOptions {
            wheels: vec![WheelOptions { suspension_stiffness: f32::INFINITY, ..WheelOptions::new(Vec3::ZERO) }],
            ..Default::default()
        },
        VehicleOptions { wheels: wheels.clone(), engine_force: f32::INFINITY, ..Default::default() },
        VehicleOptions { wheels: Vec::new(), ..Default::default() },
    ];
    for options in invalid {
        assert!(first.create_vehicle(chassis, options).is_err());
        assert_eq!(first.vehicle_count(), 0);
        assert_eq!(first.body_vehicle(chassis), None);
    }

    // Drive limits retune live within the creation bounds; invalid values change nothing.
    let replacement = first.create_vehicle(chassis, one_wheel).unwrap();
    let tuning = first.vehicle_tuning(replacement).unwrap();
    let retuned = VehicleTuning { engine_force: 900.0, ..tuning };
    first.set_vehicle_tuning(replacement, retuned).unwrap();
    assert_eq!(first.vehicle_tuning(replacement), Some(retuned));
    for invalid in [
        VehicleTuning { max_steer: 2.0, ..tuning },
        VehicleTuning { brake_force: -1.0, ..tuning },
        VehicleTuning { engine_force: f32::NAN, ..tuning },
    ] {
        assert!(matches!(first.set_vehicle_tuning(replacement, invalid), Err(PhysicsError::OutOfRange { .. })));
    }
    assert_eq!(first.vehicle_tuning(replacement), Some(retuned));
    assert_eq!(second.set_vehicle_tuning(replacement, tuning), Err(PhysicsError::UnknownVehicle));

    // Removing a chassis removes the vehicle that borrows it.
    assert!(first.remove_body(chassis));
    assert!(!first.contains_vehicle(replacement));
    assert_eq!(
        first.set_vehicle_input(replacement, VehicleInput { throttle: 1.0, ..Default::default() }),
        Err(PhysicsError::UnknownVehicle)
    );
    assert_eq!(first.vehicle_count(), 0);
    assert!(first.wheel_transforms(replacement).is_empty());
}

#[test]
fn raycast_wheels_provide_traction_steering_and_braking() {
    // Straight-line drive along +Z, then braking.
    let mut straight_world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    ground(&mut straight_world);
    let (chassis, vehicle) = make_vehicle(&mut straight_world);
    simulate(&mut straight_world, 1.0);
    assert_eq!(straight_world.grounded_wheel_count(vehicle), 4, "every wheel rests on the ground");
    let resting = straight_world.body_transform(chassis).unwrap().position;

    straight_world.set_vehicle_input(vehicle, VehicleInput { throttle: 1.0, ..Default::default() }).unwrap();
    simulate(&mut straight_world, 2.0);
    let driven = straight_world.body_transform(chassis).unwrap().position;
    assert!(driven.z - resting.z > 2.0, "positive throttle drives local +Z: {driven}");
    assert!(driven.x.abs() < 0.25, "straight drive does not drift: {driven}");
    let unbraked_speed = straight_world.vehicle_speed(vehicle);
    straight_world.set_vehicle_input(vehicle, VehicleInput { brake: 1.0, ..Default::default() }).unwrap();
    simulate(&mut straight_world, 1.0);
    assert!(straight_world.vehicle_speed(vehicle) < unbraked_speed * 0.5);

    // Positive steer turns toward local +X: the chassis yaws positive and drifts to +X.
    let mut turning_world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    ground(&mut turning_world);
    let (chassis, vehicle) = make_vehicle(&mut turning_world);
    simulate(&mut turning_world, 1.0);
    turning_world.set_vehicle_input(vehicle, VehicleInput { throttle: 1.0, steer: 1.0, brake: 0.0 }).unwrap();
    simulate(&mut turning_world, 2.0);
    let turned = turning_world.body_transform(chassis).unwrap();
    assert!(turned.position.x > 0.5, "{turned:?}");
    assert!(turned.rotation.y > 0.1, "heading turns toward +X: {turned:?}");
    assert!(
        turning_world.set_vehicle_input(vehicle, VehicleInput { throttle: f32::NAN, ..Default::default() }).is_err()
    );

    // Wheels without driven or steering flags ignore input.
    let mut passive_world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    ground(&mut passive_world);
    let chassis = passive_world
        .create_body(BodyOptions {
            mass: 180.0,
            half_extents: Vec3::new(0.8, 0.25, 1.25),
            position: Vec3::new(0.0, 1.0, 0.0),
            ..BodyOptions::new(PrimitiveShape::Box)
        })
        .unwrap();
    let wheels =
        [-1.0, 1.0].into_iter().flat_map(|z| [-0.8, 0.8].map(|x| WheelOptions::new(Vec3::new(x, 0.0, z)))).collect();
    let passive = passive_world.create_vehicle(chassis, VehicleOptions { wheels, ..Default::default() }).unwrap();
    simulate(&mut passive_world, 1.0);
    passive_world.set_vehicle_input(passive, VehicleInput { throttle: 1.0, steer: 1.0, brake: 0.0 }).unwrap();
    simulate(&mut passive_world, 1.0);
    assert!(passive_world.body_transform(chassis).unwrap().position.z.abs() < 0.01);
}

#[test]
fn colliders_and_wheels_follow_the_x_then_y_then_z_rotation_convention() {
    let mut world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    let rotations = [
        Vec3::new(std::f32::consts::FRAC_PI_4, std::f32::consts::FRAC_PI_4, 0.0),
        Vec3::new(0.8, -0.7, 2.4),
        Vec3::new(-1.2, 2.2, -2.8),
        Vec3::new(0.6, std::f32::consts::FRAC_PI_2, -0.3),
        Vec3::new(-0.9, -std::f32::consts::FRAC_PI_2, 1.1),
    ];
    let plane =
        world.create_body(BodyOptions { rotation: rotations[0], ..BodyOptions::new(PrimitiveShape::Plane) }).unwrap();
    let chassis = world
        .create_body(BodyOptions {
            mass: 180.0,
            position: Vec3::new(100.0, 0.0, 0.0),
            ..BodyOptions::new(PrimitiveShape::Box)
        })
        .unwrap();
    let vehicle = world
        .create_vehicle(chassis, VehicleOptions { wheels: vec![WheelOptions::new(Vec3::ZERO)], ..Default::default() })
        .unwrap();

    for rotation in rotations {
        world.set_body_transform(plane, None, Some(rotation)).unwrap();
        world.set_body_transform(chassis, None, Some(rotation)).unwrap();
        let visual = quat_from_euler(rotation);
        let normal = visual * Vec3::Y;

        // The plane's collider normal matches the rendered +Y normal for every rotation.
        let hit = world.raycast(normal * 3.0, normal * -3.0).unwrap();
        assert_eq!(hit.body, plane);
        assert!((hit.normal - normal).abs().max_element() < 1e-5, "normal for {rotation}");

        // Returned orientations reproduce the same rotation matrix as the renderer's convention.
        let wheel = world.wheel_transforms(vehicle)[0];
        for result in [world.body_transform(plane).unwrap().rotation, wheel.rotation] {
            let round_trip = quat_from_euler(result);
            let expected = glam::Mat3::from_quat(visual);
            let actual = glam::Mat3::from_quat(round_trip);
            assert!(
                (expected - actual).to_cols_array().iter().all(|value| value.abs() < 1e-5),
                "orientation for {rotation}"
            );
        }
    }
}

#[test]
fn sdf_bodies_rest_stack_and_answer_raycasts() {
    let mut world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
    ground(&mut world);
    let crate_shape = SdfShape::primitive(SdfPrimitive::Box { half_extents: Vec3::splat(0.5) }).unwrap();
    let ring_shape = SdfShape::primitive(SdfPrimitive::Torus { major_radius: 0.5, minor_radius: 0.15 }).unwrap();
    let (positions, indices) = box_mesh(Vec3::new(0.6, 0.3, 0.4), true);
    let baked = SdfShape::from_triangle_mesh(&positions, &indices, SdfBakeOptions { resolution: 16 }).unwrap();

    // A three-high SDF stack, a non-convex ring, and a baked mesh, all dropped onto a primitive box ground.
    let stack: Vec<BodyHandle> = (0..3)
        .map(|level| {
            let position = Vec3::new(-3.0, 0.55 + (level as f32 * 1.02), 0.0);
            world.create_sdf_body(&crate_shape, BodyOptions { mass: 1.0, position, ..BodyOptions::default() }).unwrap()
        })
        .collect();
    let ring = world
        .create_sdf_body(
            &ring_shape,
            BodyOptions {
                mass: 1.0,
                position: Vec3::new(0.0, 1.0, 0.0),
                rotation: Vec3::new(0.3, 0.0, 0.2),
                ..BodyOptions::default()
            },
        )
        .unwrap();
    let tray = world
        .create_sdf_body(
            &baked,
            BodyOptions { mass: 1.0, position: Vec3::new(3.0, 1.0, 0.0), ..BodyOptions::default() },
        )
        .unwrap();
    simulate(&mut world, 4.0);

    // Everything settles at its resting height without tipping, sinking, or drifting.
    for (level, body) in stack.iter().enumerate() {
        let transform = world.body_transform(*body).unwrap();
        assert!((transform.position.y - (0.5 + level as f32)).abs() < 0.02, "level {level}: {transform:?}");
        assert!((transform.position.x + 3.0).abs() < 0.02 && transform.rotation.abs().max_element() < 0.02);
        assert!(world.body_velocity(*body).unwrap().length() < 0.02);
    }
    let ring_pose = world.body_transform(ring).unwrap();
    assert!((ring_pose.position.y - 0.15).abs() < 0.01, "{ring_pose:?}");
    assert!(ring_pose.rotation.x.abs() < 0.02 && ring_pose.rotation.z.abs() < 0.02);
    assert!((world.body_transform(tray).unwrap().position.y - 0.3).abs() < 0.02);
    let statistics = world.sdf_collision_statistics();
    assert!(statistics.pairs >= 5 && statistics.contacts >= 5, "{statistics:?}");

    // Rays stop at SDF surfaces, pass through the ring's hole to the ground, and report outward normals.
    let top = world.raycast(Vec3::new(-3.0, 5.0, 0.0), Vec3::new(-3.0, -1.0, 0.0)).unwrap();
    assert_eq!(top.body, stack[2]);
    assert!((top.point.y - 3.0).abs() < 0.03 && top.normal.y > 0.95);
    let tube = world.raycast(Vec3::new(0.5, 2.0, 0.0), Vec3::new(0.5, -1.0, 0.0)).unwrap();
    assert_eq!(tube.body, ring);
    assert!((tube.point.y - 0.3).abs() < 0.03);
    let hole = world.raycast(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, -1.0, 0.0)).unwrap();
    assert_ne!(hole.body, ring);
    assert!(hole.point.y.abs() < 0.01);
}

#[test]
fn sdf_shapes_validate_input_and_bake_open_meshes() {
    let invalid = [
        SdfPrimitive::Box { half_extents: Vec3::new(0.5, 0.0, 0.5) },
        SdfPrimitive::Sphere { radius: f32::NAN },
        SdfPrimitive::Torus { major_radius: 0.2, minor_radius: 0.3 },
    ];
    for primitive in invalid {
        assert!(matches!(SdfShape::primitive(primitive), Err(PhysicsError::InvalidSdfShape(_))), "{primitive:?}");
    }
    let (positions, indices) = box_mesh(Vec3::splat(0.5), false);
    let coarse = SdfBakeOptions { resolution: 4 };
    assert!(matches!(SdfShape::from_triangle_mesh(&positions, &indices, coarse), Err(PhysicsError::OutOfRange { .. })));
    let options = SdfBakeOptions { resolution: 16 };
    assert!(SdfShape::from_triangle_mesh(&positions, &indices[..4], options).is_err());
    assert!(SdfShape::from_triangle_mesh(&positions, &[0, 1, 99], options).is_err());
    let flat = [Vec3::ZERO, Vec3::X, Vec3::Z];
    assert!(matches!(SdfShape::from_triangle_mesh(&flat, &[0, 1, 2], options), Err(PhysicsError::InvalidSdfShape(_))));

    // Procedural meshes are often open or inconsistently wound: an open-top crate with flipped winding still bakes a
    // solid whose inside, outside, and volume match the closed box.
    let (positions, mut indices) = box_mesh(Vec3::splat(0.5), true);
    let (triangles, _) = indices.as_chunks_mut::<3>();
    for triangle in triangles {
        triangle.swap(1, 2);
    }
    let crate_shape = SdfShape::from_triangle_mesh(&positions, &indices, options).unwrap();
    assert!((crate_shape.distance(Vec3::ZERO) + 0.5).abs() < 0.02);
    assert!((crate_shape.distance(Vec3::new(1.0, 0.0, 0.0)) - 0.5).abs() < 0.02);
    assert!((crate_shape.distance(Vec3::new(0.0, -1.0, 0.0)) - 0.5).abs() < 0.02);
    assert!((crate_shape.volume() - 1.0).abs() < 0.05, "{crate_shape:?}");
    assert_eq!(crate_shape.local_bounds(), (Vec3::splat(-0.5), Vec3::splat(0.5)));
    assert!(crate_shape.seed_count() > 0 && !crate_shape.is_convex());
}

#[test]
fn identical_worlds_produce_bit_identical_results() {
    let run = || {
        let mut world = PhysicsWorld::new(PhysicsWorldOptions::default()).unwrap();
        ground(&mut world);
        let (chassis, vehicle) = make_vehicle(&mut world);
        let crate_box = world
            .create_body(BodyOptions {
                mass: 5.0,
                position: Vec3::new(0.2, 3.0, 4.0),
                rotation: Vec3::new(0.3, 0.1, -0.2),
                ..BodyOptions::new(PrimitiveShape::Box)
            })
            .unwrap();

        // SDF contacts sample seeded random streams; tumbling non-convex bodies exercise them every step.
        let ring = SdfShape::primitive(SdfPrimitive::Torus { major_radius: 0.4, minor_radius: 0.12 }).unwrap();
        let (positions, indices) = box_mesh(Vec3::new(0.3, 0.2, 0.25), true);
        let tray = SdfShape::from_triangle_mesh(&positions, &indices, SdfBakeOptions { resolution: 12 }).unwrap();
        let tumbling = [
            world
                .create_sdf_body(
                    &ring,
                    BodyOptions {
                        mass: 1.0,
                        position: Vec3::new(-0.3, 4.5, 4.0),
                        rotation: Vec3::new(0.9, 0.2, 0.4),
                        ..BodyOptions::default()
                    },
                )
                .unwrap(),
            world
                .create_sdf_body(
                    &tray,
                    BodyOptions {
                        mass: 1.0,
                        position: Vec3::new(0.4, 5.5, 4.2),
                        rotation: Vec3::new(-0.5, 0.7, 0.1),
                        ..BodyOptions::default()
                    },
                )
                .unwrap(),
        ];
        let mut trace = Vec::new();
        for frame in 0..240 {
            let input = VehicleInput { throttle: 1.0, steer: if frame > 90 { 0.6 } else { 0.0 }, brake: 0.0 };
            world.set_vehicle_input(vehicle, input).unwrap();
            world.step(1.0 / 60.0).unwrap();
            for body in [chassis, crate_box, tumbling[0], tumbling[1]] {
                let transform = world.body_transform(body).unwrap();
                trace.extend(transform.position.to_array().map(f32::to_bits));
                trace.extend(transform.rotation.to_array().map(f32::to_bits));
            }
            for wheel in world.wheel_transforms(vehicle) {
                trace.extend(wheel.position.to_array().map(f32::to_bits));
            }
        }
        trace
    };
    assert_eq!(run(), run());
}
