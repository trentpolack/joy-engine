// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The lab's deterministic Y-up room (TypeScript `room.ts`): saturated red and green walls for screen-space color
//! bleed, blocks and corners for contact occlusion, thin slats for temporal antialiasing, a smooth floor (scene
//! roughness 0.18) and blue block (0.05) for reflections, linear colors above one for HDR bloom, and 27 shadowed point
//! lights for direct and clustered volumetric lighting.

use joy_engine::prelude::*;

/// One generated room: its scene (owned by the caller), the block that moves, and its lights.
pub struct Room {
    /// Every room mesh; dropping the scene releases its GPU uploads once no other scene draws the shared cube.
    pub scene: RenderScene,
    /// The orange block that sine motion moves.
    pub moving: MeshInstanceId,
    /// Point lights in world units; every light casts shadows, the TypeScript default.
    pub lights: Vec<PointLight>,
}

/// The moving block's rest position.
pub const MOVING_POSITION: Vec3 = Vec3::new(0.0, 1.1, 1.2);

/// Build the room. `alternate` swaps the side wall colors, so replacing the scene is visible.
pub fn create_room(alternate: bool) -> Room {
    let mut scene = RenderScene::new();
    let cube = primitive_box();
    let mut add = |position: [f32; 3], scale: [f32; 3], tint: [f32; 3], roughness: f32, yaw: f32| {
        let transform = Transform::new(Vec3::from(position), Vec3::new(0.0, yaw, 0.0), Vec3::from(scale));
        let mut instance = MeshInstance::new(cube.clone()).with_transform(transform).with_tint(tint);
        instance.scene_roughness = roughness;
        scene.insert(instance)
    };

    // An open front and ceiling keep every diagnostic surface visible from the default camera.
    add([0.0, -0.2, 0.0], [12.0, 0.4, 12.0], [0.62, 0.65, 0.7], 0.18, 0.0);
    add([0.0, 3.0, -5.8], [12.0, 6.0, 0.4], [0.7, 0.72, 0.76], 1.0, 0.0);
    let left = if alternate { [0.12, 0.3, 0.9] } else { [0.85, 0.08, 0.035] };
    let right = if alternate { [0.8, 0.4, 0.05] } else { [0.04, 0.7, 0.18] };
    add([-5.8, 3.0, 0.0], [0.4, 6.0, 12.0], left, 1.0, 0.0);
    add([5.8, 3.0, 0.0], [0.4, 6.0, 12.0], right, 1.0, 0.0);
    add([-2.2, 1.15, -1.8], [2.3, 2.3, 2.3], [0.76, 0.78, 0.81], 1.0, 0.3);
    add([2.1, 1.85, -2.4], [2.0, 3.7, 2.0], [0.62, 0.7, 0.87], 0.05, -0.24);
    add([0.0, 0.22, 2.4], [2.1, 0.44, 1.6], [0.06, 0.08, 0.12], 0.05, 0.0);
    let moving = add(MOVING_POSITION.to_array(), [1.1, 1.1, 1.1], [0.95, 0.55, 0.06], 0.32, 0.0);

    // Linear color exceeds one on purpose: bloom sees actual HDR scene radiance.
    add([0.0, 4.9, -4.9], [2.8, 0.16, 0.15], [12.0, 7.0, 2.0], 1.0, 0.0);
    add([-4.9, 1.6, -3.4], [0.16, 1.5, 0.15], [3.0, 0.12, 0.08], 1.0, 0.0);
    add([4.9, 1.6, -3.4], [0.16, 1.5, 0.15], [0.12, 3.0, 0.4], 1.0, 0.0);
    for index in 0..14 {
        add([-4.6 + (index as f32 * 0.34), 1.8, -5.5], [0.035, 2.7, 0.15], [0.1, 0.12, 0.16], 1.0, 0.0);
    }

    let light = |position: [f32; 3], color: [f32; 3], range: f32| PointLight {
        position: Vec3::from(position),
        color,
        range,
        casts_shadow: true,
    };
    let mut lights = vec![
        light([0.0, 4.5, -1.8], [3.4, 3.0, 2.5], 12.0),
        light([-4.8, 2.0, -1.0], [1.3, 0.08, 0.035], 6.0),
        light([4.8, 2.0, -1.0], [0.04, 1.2, 0.18], 6.0),
    ];
    for index in 0..24 {
        let angle = index as f32 * std::f32::consts::PI / 12.0;
        let tier = (index % 3) as f32;
        lights.push(light(
            [angle.sin() * 4.8, 0.8 + (tier * 0.5), angle.cos() * 4.8],
            [0.18 + (tier * 0.06), 0.16, 0.25],
            3.5,
        ));
    }
    Room { scene, moving, lights }
}
