// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Headless rendering of GPU-instanced particles: every shape covers its particle in every blend mode and blends as
//! authored, source-over particles draw back to front, effects outside the view are culled, opaque geometry hides
//! particles behind it, soft particles fade where they meet opaque geometry, lit particles shade by the frame's lights,
//! and arcade frames place particles between their translucent layers in the games' display-color convention.
//!
//! Needs a GPU adapter (a software adapter such as llvmpipe is enough); without one each test prints a message and
//! returns. Set `JOY_RENDER_CAPTURE_DIR` to keep PNG captures of the shape, blend, and arcade scenes. Native only.
#![cfg(not(target_arch = "wasm32"))]

use joy_core::LinearColor;
use joy_core::Transform;
use joy_core::math::{
    Mat4,
    Vec2,
    Vec3,
};
use joy_fx::{
    CompiledParticleEffect,
    DEFAULT_PARTICLE_TEXTURE,
    ParticleBlend,
    ParticleEffect,
    ParticleEffectOptions,
    compile_particle_effect,
    parse_particle_effect,
};
use joy_renderer::particles::{
    ParticleDrawOptions,
    ParticleFrame,
    ParticleTextureSet,
};
use joy_renderer::{
    BackendPreference,
    CAPTURE_FORMAT,
    Camera,
    ColorVertex,
    DirectionalLight,
    FrameInputs,
    GpuContext,
    Material,
    MaterialAsset,
    MeshAsset,
    MeshInstance,
    OffscreenTarget,
    OrbitCamera,
    PointLight,
    RenderScene,
    Renderer,
    RendererOptions,
    TopDownCamera,
    TopDownView,
    decode_display_colors,
    project_to_viewport,
};
use serde_json::{
    Value,
    json,
};

/// Every particle renderer, in the column order of the shape scenes.
const RENDERERS: [&str; 8] = ["soft", "circle", "ring", "billow", "streak", "square", "textured", "flipbook"];

/// World distance between shape columns.
const COLUMN_SPACING: f64 = 2.6;

/// Every shape's roll, in radians.
const ROLL: f64 = 0.5;

/// The authored particle color: linear orange with mostly opaque alpha.
const ORANGE: [f64; 4] = [1.0, 0.45, 0.12, 0.85];

/// The shape scenes' linear background.
const BACKGROUND: LinearColor = LinearColor::new(0.02, 0.025, 0.04, 1.0);

fn gpu() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

/// One emitter per renderer for each `(blend, y)` row: one particle per column at size one, rolled by [`ROLL`]. The
/// streak's head sits right of its column and trails down to the left.
fn shapes_definition(rows: &[(ParticleBlend, f64)]) -> CompiledParticleEffect {
    let mut emitters = Vec::new();
    for (row, (blend, y)) in rows.iter().enumerate() {
        for (column, renderer) in RENDERERS.iter().enumerate() {
            let x = column_x(column);
            let [r, g, b, a] = ORANGE;
            let spawn = if *renderer == "streak" {
                format!("x = {x} + 0.8; y = {y}; size = 0.45; vx = 60; vy = 25; color = rgba({r}, {g}, {b}, {a});")
            } else {
                format!("x = {x}; y = {y}; size = 1; rotation = {ROLL}; color = rgba({r}, {g}, {b}, {a});")
            };
            let mut emitter = json!({"id": format!("{renderer}_{row}"), "enabled": true, "root": true, "duration": 0,
                "rate": 0, "bursts": [{"time": 0, "count": 1}], "lifetime": 5, "renderer": renderer,
                "blend": blend.as_str(), "spawn": spawn, "update": ""});
            if *renderer == "textured" {
                emitter["texture"] = json!({"source": DEFAULT_PARTICLE_TEXTURE});
            }
            if *renderer == "flipbook" {
                emitter["texture"] = json!({"source": DEFAULT_PARTICLE_TEXTURE, "columns": 2, "rows": 2});
            }
            emitters.push(emitter);
        }
    }
    compile_particle_effect(&json!({
        "version": 1, "name": "Shapes", "maxParticles": 64, "parameters": {}, "emitters": emitters
    }))
    .unwrap()
}

fn column_x(column: usize) -> f64 {
    (column as f64 - 3.5) * COLUMN_SPACING
}

/// An opaque white band behind the lower half of each row centered at `rows`.
fn backdrop_bands(rows: &[f64]) -> Vec<ColorVertex> {
    let mut vertices = Vec::new();
    for y in rows {
        let corner = |x: f32, corner_y: f32| ColorVertex {
            position: [x, corner_y, -1.0],
            color: [1.0, 1.0, 1.0, 1.0],
        };
        let (left, right, bottom, top) = (-14.0, 14.0, (*y as f32) - 1.6, *y as f32);
        vertices.extend([
            corner(left, bottom),
            corner(right, bottom),
            corner(right, top),
            corner(left, bottom),
            corner(right, top),
            corner(left, top),
        ]);
    }
    vertices
}

/// A camera on +Z looking at the origin from `distance`.
fn front_camera(distance: f32) -> OrbitCamera {
    OrbitCamera {
        distance,
        yaw: 0.0,
        pitch: 0.0,
        ..OrbitCamera::default()
    }
}

/// Render `scene` and `inputs` from `camera` into a `size` capture; returns the pixels and the renderer.
fn render(
    gpu: &GpuContext,
    size: (u32, u32),
    scene: &RenderScene,
    camera: &OrbitCamera,
    inputs: &FrameInputs<'_>,
) -> (Vec<u8>, Renderer) {
    let target = OffscreenTarget::new(gpu, size.0, size.1).expect("capture target");
    let mut renderer = Renderer::new(gpu, CAPTURE_FORMAT, RendererOptions::default());
    renderer.resize(size.0, size.1, 1.0);
    renderer.render(scene, camera, inputs, target.view()).expect("frame renders");
    (target.read_rgba(gpu).expect("readback"), renderer)
}

/// Keep `pixels` as `name` when `JOY_RENDER_CAPTURE_DIR` is set.
fn save_capture(name: &str, pixels: &[u8], size: (u32, u32)) {
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join(name);
        image::save_buffer(&path, pixels, size.0, size.1, image::ColorType::Rgba8).expect("png written");
    }
}

fn pixel(pixels: &[u8], width: u32, point: Vec2) -> [i32; 3] {
    let offset = (((point.y as u32) * width + (point.x as u32)) * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]].map(i32::from)
}

fn largest_difference(left: [i32; 3], right: [i32; 3]) -> i32 {
    left.iter().zip(right).map(|(a, b)| (a - b).abs()).max().unwrap()
}

/// A point `local` (in particle sizes, before roll) from the center of the particle in `column` at height `y`, rolled
/// for the shapes that roll.
fn shape_point(column: usize, y: f64, local: Vec2) -> Vec3 {
    let rolled = if RENDERERS[column] == "square" || RENDERERS[column] == "textured" || RENDERERS[column] == "flipbook"
    {
        Vec2::from_angle(ROLL as f32).rotate(local)
    } else {
        local
    };
    Vec3::new(column_x(column) as f32 + rolled.x, y as f32 + rolled.y, 0.0)
}

#[test]
fn every_shape_and_blend_mode_covers_its_particle_and_blends_as_authored() {
    let Some(gpu) = gpu() else {
        return;
    };
    let rows = [(ParticleBlend::Alpha, 2.6), (ParticleBlend::Additive, 0.0), (ParticleBlend::Premultiplied, -2.6)];
    let definition = shapes_definition(&rows);
    let effect = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let mut textures = ParticleTextureSet::new();
    textures.prepare(&definition).unwrap();
    textures.activate(Some(&definition));
    let mut particles = ParticleFrame::new();
    particles.add_effect(&effect, Some(&textures)).unwrap();
    assert_eq!(particles.batches().len(), 4, "source-over and additive batches, each colored and textured");

    let size = (1600, 700);
    let camera = front_camera(11.0);
    let backdrop = backdrop_bands(&rows.map(|(_, y)| y));
    let inputs = FrameInputs {
        clear_color: BACKGROUND,
        opaque_triangles: &backdrop,
        particles: Some(&particles),
        ..FrameInputs::default()
    };
    let (pixels, renderer) = render(&gpu, size, &RenderScene::new(), &camera, &inputs);
    save_capture("particles-blend-modes.png", &pixels, size);
    assert_eq!(renderer.diagnostics().particles, RENDERERS.len() * rows.len());
    assert_eq!(renderer.diagnostics().particle_batches_culled, 0);

    let viewport = Vec2::new(size.0 as f32, size.1 as f32);
    let view_projection = camera.camera_frame(viewport.x / viewport.y).view_projection();
    let sample = |point: Vec3| pixel(&pixels, size.0, project_to_viewport(point, view_projection, viewport).unwrap());

    // Each shape covers a point it always draws (a ring's band, a streak's head, a flipbook cell's arms) over the dark
    // upper half, and leaves the gap to the next column untouched.
    let background = sample(Vec3::new(-10.0, 4.3, 0.0));
    let covered_points = [
        Vec2::ZERO,
        Vec2::ZERO,
        Vec2::new(1.0, 0.0),
        Vec2::ZERO,
        Vec2::new(0.8, 0.0),
        Vec2::ZERO,
        Vec2::ZERO,
        Vec2::new(0.8, -0.8),
    ];
    for (blend, y) in rows {
        for (column, renderer) in RENDERERS.iter().enumerate() {
            let covered = sample(shape_point(column, y, covered_points[column]));
            assert!(
                covered[0] > background[0] + 60 && covered[0] > covered[2] + 40,
                "{renderer} {} draws orange: {covered:?} over {background:?}",
                blend.as_str()
            );
            if column + 1 < RENDERERS.len() {
                let gap = sample(Vec3::new((column_x(column) + 1.3) as f32, y as f32 + 0.9, 0.0));
                let outside = largest_difference(gap, background);
                assert!(outside <= 3, "{renderer} {} stays inside its bounds: {gap:?}", blend.as_str());
            }
        }
    }

    // Over the white band, a soft particle's falloff blends as authored: straight and premultiplied alpha hide some of
    // the band's blue, while additive light only adds.
    let band = |y: f64| sample(Vec3::new((column_x(0) + 1.3) as f32, y as f32 - 0.5, 0.0));
    let soft = |y: f64| sample(Vec3::new(column_x(0) as f32, y as f32 - 0.5, 0.0));
    let [(_, alpha_y), (_, additive_y), (_, premultiplied_y)] = rows;
    assert!(soft(alpha_y)[2] < band(alpha_y)[2] - 10, "alpha occludes: {:?} {:?}", soft(alpha_y), band(alpha_y));
    let added = soft(additive_y);
    assert!(added.iter().zip(band(additive_y)).all(|(particle, band)| *particle >= band - 1), "additive {added:?}");
    assert!(added[0] > band(additive_y)[0] + 10, "additive brightens {added:?}");
    let premultiplied = soft(premultiplied_y);
    assert!(premultiplied[2] < band(premultiplied_y)[2] - 10, "premultiplied occludes by alpha {premultiplied:?}");
    assert!(premultiplied[0] > soft(alpha_y)[0], "premultiplied RGB is not weighted by alpha again");

    // The single-row scene the CPU triangle path was captured with, for review against it.
    if std::env::var("JOY_RENDER_CAPTURE_DIR").is_ok() {
        let definition = shapes_definition(&[(ParticleBlend::Alpha, 0.0)]);
        let effect = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
        let mut textures = ParticleTextureSet::new();
        textures.prepare(&definition).unwrap();
        textures.activate(Some(&definition));
        particles.clear();
        particles.add_effect(&effect, Some(&textures)).unwrap();
        let backdrop = backdrop_bands(&[0.0]);
        let inputs = FrameInputs {
            clear_color: BACKGROUND,
            opaque_triangles: &backdrop,
            particles: Some(&particles),
            ..FrameInputs::default()
        };
        let size = (1600, 400);
        let (pixels, _) = render(&gpu, size, &RenderScene::new(), &front_camera(6.64), &inputs);
        save_capture("particles-shapes.png", &pixels, size);
    }
}

/// A box of full extents `size` with flat white faces (two triangles per face).
fn block(size: f32) -> MeshAsset {
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    for (normal, u, v) in faces {
        let base = (positions.len() / 3) as u32;
        for (du, dv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            for axis in 0..3 {
                positions.push((normal[axis] + (u[axis] * du) + (v[axis] * dv)) * size * 0.5);
            }
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    MeshAsset::new(&crate::flat_mesh(&positions, &indices, &[])).expect("block geometry is valid")
}

/// One root emitter of `count` particles drawn with `renderer` and `blend`.
fn emitter(id: &str, renderer: &str, blend: &str, count: u32, spawn: &str) -> Value {
    json!({"id": id, "enabled": true, "root": true, "duration": 0, "rate": 0, "bursts": [{"time": 0, "count": count}],
        "lifetime": 5, "renderer": renderer, "blend": blend, "spawn": spawn, "update": ""})
}

#[test]
fn particles_sort_back_to_front_cull_outside_the_view_and_hide_behind_opaque_geometry() {
    let Some(gpu) = gpu() else {
        return;
    };

    // Two green sprites at x = -2.5 and x = 2.5 and a premultiplied soft particle with zero alpha; in a second effect,
    // an overlapping pair of squares whose nearer (blue) particle is generated first.
    let mut sprites =
        emitter("sprites", "textured", "alpha", 2, "x = (index - 0.5) * 5; size = 1.5; color = rgba(0, 1, 0, 1);");
    sprites["texture"] = json!({"source": DEFAULT_PARTICLE_TEXTURE});
    let squares = emitter(
        "pair",
        "square",
        "alpha",
        2,
        "y = 2.5; z = 1 - index; size = 0.6; color = rgba(index, 0, 1 - index, 0.9);",
    );
    let glow =
        emitter("glow", "soft", "premultiplied", 1, "x = 2.5; y = -2; z = 3.5; size = 0.8; color = rgba(1, 0, 0, 0);");
    let definition = compile_particle_effect(&json!({
        "version": 1, "name": "Headless", "maxParticles": 8, "parameters": {}, "emitters": [sprites, glow]
    }))
    .unwrap();
    let effect = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let pair_definition = compile_particle_effect(&json!({
        "version": 1, "name": "Pair", "maxParticles": 2, "parameters": {}, "emitters": [squares]
    }))
    .unwrap();
    let pair = ParticleEffect::new(&pair_definition, ParticleEffectOptions::default()).unwrap();
    let mut textures = ParticleTextureSet::new();
    textures.prepare(&definition).unwrap();
    textures.activate(Some(&definition));

    // A second copy placed far behind the camera is culled before upload.
    let mut particles = ParticleFrame::new();
    particles.add_effect(&effect, Some(&textures)).unwrap();
    particles.add_effect(&pair, None).unwrap();
    let behind = Mat4::from_translation(Vec3::new(0.0, 0.0, 50.0));
    particles.add_transformed_effect(&effect, Some(&textures), &behind).unwrap();

    // Camera on +Z looking at the origin; an unlit white block sits between it and the right-hand sprite.
    let camera = front_camera(10.0);
    let mut scene = RenderScene::new();
    let unlit = MaterialAsset::new(Material::unlit(LinearColor::WHITE)).unwrap();
    let occluder = MeshInstance::new(block(2.0))
        .with_transform(Transform::new(Vec3::new(2.5, 0.0, 2.0), Vec3::ZERO, Vec3::ONE))
        .with_material(unlit);
    scene.insert(occluder);
    let inputs = FrameInputs {
        clear_color: LinearColor::new(0.0, 0.0, 0.25, 1.0),
        particles: Some(&particles),
        ..FrameInputs::default()
    };
    let size = (320, 240);
    let (pixels, renderer) = render(&gpu, size, &scene, &camera, &inputs);
    save_capture("particles-occlusion.png", &pixels, size);
    assert_eq!(renderer.diagnostics().particle_batches_culled, 2, "the hidden copy's two batches");
    assert_eq!(renderer.diagnostics().particles, 5);

    let viewport = Vec2::new(size.0 as f32, size.1 as f32);
    let view_projection = camera.camera_frame(viewport.x / viewport.y).view_projection();
    let sample = |point: Vec3| pixel(&pixels, size.0, project_to_viewport(point, view_projection, viewport).unwrap());

    // The left sprite's opaque center shows the green tint; its transparent corner shows the background.
    let background = sample(Vec3::new(-4.5, 3.2, 0.0));
    let center = sample(Vec3::new(-2.5, 0.0, 0.0));
    assert!(center[1] > 120 && center[1] > center[0] + 60 && center[1] > center[2] + 60, "sprite center {center:?}");
    let corner = sample(Vec3::new(-2.5 - 1.35, 1.35, 0.0));
    assert!(largest_difference(corner, background) <= 3, "transparent texels keep the background: {corner:?}");

    // The nearer blue square draws over the farther red one although it was generated first.
    let pair = sample(Vec3::new(0.0, 2.5, 0.0));
    assert!(pair[2] > pair[0] + 60, "back to front: {pair:?}");

    // The block in front hides the right sprite's center (depth test against opaque geometry); the sprite's arm left of
    // the block stays visible.
    let hidden = sample(Vec3::new(2.5, 0.0, 0.0));
    let neutral = hidden.iter().max().unwrap() - hidden.iter().min().unwrap() < 12;
    assert!(neutral && hidden[0] > 60, "the neutral block, not the green sprite, shows: {hidden:?}");
    let edge = sample(Vec3::new(2.5 - 1.35, 0.0, 0.0));
    assert!(edge[1] > edge[0] + 60, "the sprite's arm stays visible: {edge:?}");

    // Premultiplied particles with zero alpha still add their light.
    let glow = sample(Vec3::new(2.5, -2.0, 3.5));
    assert!(glow[0] > background[0] + 60, "zero-alpha premultiplied light {glow:?} over {background:?}");
}

/// Particles in the arcade games' plane, stepped `ticks` fixed steps from `x`, `y`.
fn arcade_effect(file: &str, x: f64, y: f64, ticks: u64) -> ParticleEffect {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/joyfx").join(file);
    let definition = parse_particle_effect(&std::fs::read_to_string(path).unwrap()).unwrap();
    let options = ParticleEffectOptions {
        seed: 4242,
        position: [x, y, 0.0],
        ..Default::default()
    };
    let mut effect = ParticleEffect::new(&definition, options).unwrap();
    while effect.ticks() < ticks {
        effect.step().unwrap();
    }
    effect
}

/// A top-down orthographic view over a `width` × `height` logical viewport centered on it.
fn arcade_view(width: u32, height: u32) -> TopDownView {
    let camera = TopDownCamera {
        height: 200.0,
        near: 1.0,
        far: 2000.0,
    };
    let size = Vec2::new(width as f32, height as f32);
    TopDownView::new(size, size * 0.5, camera).unwrap()
}

/// Render an arcade frame of `inputs` from `view` with linear display processing (no tone curve).
fn render_arcade(gpu: &GpuContext, view: &TopDownView, inputs: &FrameInputs<'_>) -> Vec<u8> {
    let size = view.size();
    let (width, height) = (size.x as u32, size.y as u32);
    let options = RendererOptions {
        post_process: joy_renderer::PostProcessSettings::NEUTRAL,
        shadows: None,
        ..RendererOptions::default()
    };
    let mut renderer = Renderer::new(gpu, CAPTURE_FORMAT, options);
    renderer.resize(width, height, 1.0);
    let target = OffscreenTarget::new(gpu, width, height).expect("capture target");
    renderer.render(&RenderScene::new(), view, inputs, target.view()).expect("frame renders");
    target.read_rgba(gpu).expect("readback")
}

/// An axis-aligned quad of transparent triangles at height `z`.
fn quad(left: f32, top: f32, right: f32, bottom: f32, z: f32, color: [f32; 4]) -> [ColorVertex; 6] {
    let corner = |x: f32, y: f32| ColorVertex {
        position: [x, y, z],
        color,
    };
    [
        corner(left, top),
        corner(right, top),
        corner(right, bottom),
        corner(left, top),
        corner(right, bottom),
        corner(left, bottom),
    ]
}

#[test]
fn arcade_frames_place_particles_between_their_layers_in_the_games_color_convention() {
    let Some(gpu) = gpu() else {
        return;
    };
    let (width, height) = (200_u32, 100_u32);

    // Two squares at height zero in the display color the triangles use, placed in the transparency order at height
    // 1.5: after the swatch at height 1 and before the cover at height 2, which hides the right one although the
    // particles' own height is below both.
    let color = [0.5, 0.25, 0.1];
    let spawn =
        format!("x = 50 + index * 100; y = 50; size = 12; color = rgba({}, {}, {}, 1);", color[0], color[1], color[2]);
    let definition = compile_particle_effect(&json!({"version": 1, "name": "Layer", "maxParticles": 4,
        "parameters": {}, "emitters": [emitter("squares", "square", "alpha", 2, &spawn)]}))
    .unwrap();
    let effect = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let mut particles = ParticleFrame::new();
    let layered = ParticleDrawOptions {
        sort_point: Some(Vec3::new(100.0, 50.0, 1.5)),
        display_colors: true,
        ..ParticleDrawOptions::default()
    };
    particles.add_effect_with(&effect, None, &layered).unwrap();

    // The games build triangles in display colors and decode the finished lists.
    let swatch = quad(90.0, 0.0, 110.0, 20.0, 1.0, [color[0], color[1], color[2], 1.0]);
    let cover = quad(120.0, 20.0, 180.0, 80.0, 2.0, [0.0, 0.0, 0.6, 1.0]);
    let mut transparent = [swatch, cover].concat();
    decode_display_colors(&mut transparent);
    let inputs = FrameInputs {
        transparent_triangles: &transparent,
        particles: Some(&particles),
        ..FrameInputs::default()
    };
    let pixels = render_arcade(&gpu, &arcade_view(width, height), &inputs);

    // The left square shows the same color as the decoded swatch; the cover hides the right square.
    let square = pixel(&pixels, width, Vec2::new(50.0, 50.0));
    let reference = pixel(&pixels, width, Vec2::new(100.0, 10.0));
    assert!(reference[0] > 100, "the swatch draws: {reference:?}");
    assert!(largest_difference(square, reference) <= 3, "particle {square:?} matches triangle {reference:?}");
    let covered = pixel(&pixels, width, Vec2::new(150.0, 50.0));
    assert!(covered[2] > covered[0] + 60, "triangles above the sort point draw over particles: {covered:?}");

    // The arcade effects in the games' color convention, for review against the games' captures.
    if std::env::var("JOY_RENDER_CAPTURE_DIR").is_ok() {
        let (width, height) = (960_u32, 480_u32);
        particles.clear();
        let blast = arcade_effect("blast-garden-blast.joyfx", 240.0, 240.0, 9);
        let explosion = arcade_effect("space-scuffles-explosion.joyfx", 720.0, 240.0, 14);
        let display = ParticleDrawOptions {
            display_colors: true,
            ..ParticleDrawOptions::default()
        };
        particles.add_effect_with(&blast, None, &display).unwrap();
        particles.add_effect_with(&explosion, None, &display).unwrap();
        let inputs = FrameInputs {
            clear_color: LinearColor::new(0.01, 0.012, 0.02, 1.0),
            particles: Some(&particles),
            ..FrameInputs::default()
        };
        let pixels = render_arcade(&gpu, &arcade_view(width, height), &inputs);
        save_capture("particles-arcade.png", &pixels, (width, height));
    }
}

/// One opaque, unit-ambient white wall at height zero below `top` (world units, in front of a front camera).
fn wall_below(top: f32) -> Vec<ColorVertex> {
    let corner = |x: f32, y: f32| ColorVertex {
        position: [x, y, 0.0],
        color: [1.0, 1.0, 1.0, 1.0],
    };
    let (left, right, bottom) = (-8.0, 8.0, -6.0);
    vec![
        corner(left, bottom),
        corner(right, bottom),
        corner(right, top),
        corner(left, bottom),
        corner(right, top),
        corner(left, top),
    ]
}

#[test]
fn soft_particles_fade_where_they_meet_opaque_geometry() {
    let Some(gpu) = gpu() else {
        return;
    };

    // Green squares just in front of a wall that covers their lower half: the left one hard, the right one fading
    // over two world units. Their upper halves have only the background behind them.
    let hard = emitter("hard", "square", "alpha", 1, "x = -2; y = 0; z = 0.3; size = 1; color = rgba(0, 1, 0, 1);");
    let mut soft = emitter("soft", "square", "alpha", 1, "x = 2; y = 0; z = 0.3; size = 1; color = rgba(0, 1, 0, 1);");
    soft["softness"] = json!(2);
    let definition = compile_particle_effect(&json!({
        "version": 1, "name": "Soft", "maxParticles": 4, "parameters": {}, "emitters": [hard, soft]
    }))
    .unwrap();
    let effect = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let mut particles = ParticleFrame::new();
    particles.add_effect(&effect, None).unwrap();
    assert!(particles.batches()[0].is_soft(), "the shared batch draws with the soft pipelines");

    let camera = front_camera(10.0);
    let wall = wall_below(0.0);
    let inputs = FrameInputs {
        clear_color: LinearColor::BLACK,
        opaque_triangles: &wall,
        ambient: Some(1.0),
        particles: Some(&particles),
        ..FrameInputs::default()
    };
    let size = (320, 240);
    let (pixels, _) = render(&gpu, size, &RenderScene::new(), &camera, &inputs);
    save_capture("particles-soft.png", &pixels, size);
    let viewport = Vec2::new(size.0 as f32, size.1 as f32);
    let view_projection = camera.camera_frame(viewport.x / viewport.y).view_projection();
    let sample = |point: Vec3| pixel(&pixels, size.0, project_to_viewport(point, view_projection, viewport).unwrap());

    // Over the wall, the hard square stays solid green while the soft one, 0.3 units in front of it, keeps only about
    // a sixth of its coverage and lets the white wall through.
    let hard_on_wall = sample(Vec3::new(-2.0, -0.5, 0.3));
    let soft_on_wall = sample(Vec3::new(2.0, -0.5, 0.3));
    assert!(hard_on_wall[1] > 200 && hard_on_wall[0] < 20, "the hard square covers the wall: {hard_on_wall:?}");
    assert!(soft_on_wall[0] > 150 && soft_on_wall[1] > soft_on_wall[0], "the wall shows through: {soft_on_wall:?}");

    // With nothing opaque behind them, both squares draw the same.
    let hard_in_air = sample(Vec3::new(-2.0, 0.5, 0.3));
    let soft_in_air = sample(Vec3::new(2.0, 0.5, 0.3));
    assert!(hard_in_air[1] > 200, "green over the background: {hard_in_air:?}");
    assert!(largest_difference(hard_in_air, soft_in_air) <= 2, "{soft_in_air:?} fades only near geometry");
}

#[test]
fn lit_particles_shade_by_the_frames_lights_while_unlit_ones_keep_their_color() {
    let Some(gpu) = gpu() else {
        return;
    };

    // Two white squares facing the camera: the left one unlit, the right one lit. A red key light shines from the
    // camera's side and a blue point light sits just in front of the lit square.
    let unlit = emitter("unlit", "square", "alpha", 1, "x = -2; size = 1; color = rgba(1, 1, 1, 1);");
    let mut lit = emitter("lit", "square", "alpha", 1, "x = 2; size = 1; color = rgba(1, 1, 1, 1);");
    lit["lighting"] = json!("lit");
    let definition = compile_particle_effect(&json!({
        "version": 1, "name": "Lit", "maxParticles": 4, "parameters": {}, "emitters": [unlit, lit]
    }))
    .unwrap();
    let effect = ParticleEffect::new(&definition, ParticleEffectOptions::default()).unwrap();
    let mut particles = ParticleFrame::new();
    particles.add_effect(&effect, None).unwrap();

    let key = DirectionalLight {
        direction: Vec3::Z,
        color: LinearColor::rgb(1.0, 0.0, 0.0),
        intensity: 0.6,
        casts_shadow: false,
        ..DirectionalLight::default()
    };
    let lights = [PointLight {
        position: Vec3::new(2.0, 0.0, 1.0),
        color: LinearColor::rgb(0.0, 0.0, 2.0),
        range: 4.0,
        casts_shadow: false,
    }];
    let camera = front_camera(10.0);
    let inputs = FrameInputs {
        clear_color: LinearColor::BLACK,
        directional_light: Some(key),
        lights: &lights,
        ambient: Some(0.1),
        particles: Some(&particles),
        ..FrameInputs::default()
    };
    let size = (320, 240);
    let options = RendererOptions {
        post_process: joy_renderer::PostProcessSettings::NEUTRAL,
        shadows: None,
        ..RendererOptions::default()
    };
    let target = OffscreenTarget::new(&gpu, size.0, size.1).expect("capture target");
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, options);
    renderer.resize(size.0, size.1, 1.0);
    renderer.render(&RenderScene::new(), &camera, &inputs, target.view()).expect("frame renders");
    let pixels = target.read_rgba(&gpu).expect("readback");
    save_capture("particles-lit.png", &pixels, size);
    let viewport = Vec2::new(size.0 as f32, size.1 as f32);
    let view_projection = camera.camera_frame(viewport.x / viewport.y).view_projection();
    let sample = |point: Vec3| pixel(&pixels, size.0, project_to_viewport(point, view_projection, viewport).unwrap());

    // The unlit square keeps its authored white whatever lights the frame.
    let plain = sample(Vec3::new(-2.0, 0.0, 0.0));
    assert!(plain.iter().all(|channel| *channel > 250), "unlit particles keep their color: {plain:?}");

    // The lit square takes the key light's red and the point light's blue over a little ambient gray, and the
    // point light falls off away from it.
    let shaded = sample(Vec3::new(2.0, 0.0, 0.0));
    assert!(shaded[0] > shaded[1] + 60 && shaded[2] > shaded[1] + 60, "red key and blue point light: {shaded:?}");
    assert!(shaded[1] > 40 && shaded[1] < 160, "a little ambient light: {shaded:?}");
    let edge = sample(Vec3::new(2.9, 0.9, 0.0));
    assert!(edge[2] < shaded[2], "the point light reaches the center most: {edge:?} {shaded:?}");
}
