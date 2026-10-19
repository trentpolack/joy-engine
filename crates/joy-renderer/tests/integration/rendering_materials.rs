// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Textured PBR materials and image-based lighting on a real device: base color maps sample by UV, normal maps tilt
//! shading, environments light surfaces by orientation, and masked materials cast shadows with holes.
//!
//! Needs a GPU adapter (llvmpipe or SwiftShader is enough); without one each test prints a note and returns. Set
//! `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

use std::collections::HashMap;

use joy_core::math::{
    Vec2,
    Vec3,
    deterministic,
};
use joy_core::{
    LinearColor,
    MaterialAlphaMode,
    PbrMaterial,
    PbrMaterialTextures,
    PbrMesh,
    Transform,
};
use joy_renderer::{
    BackendPreference,
    CAPTURE_FORMAT,
    Camera,
    CelestialLight,
    EnvironmentData,
    EnvironmentLighting,
    EnvironmentMapOptions,
    EnvironmentPixels,
    FrameInputs,
    GpuContext,
    ImageLighting,
    MeshAsset,
    MeshInstance,
    OffscreenTarget,
    OrbitCamera,
    PointLight,
    RenderScene,
    Renderer,
    RendererOptions,
    Shading,
    TextureAsset,
    project_to_viewport,
};

const WIDTH: u32 = 256;
const HEIGHT: u32 = 192;

fn gpu() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
}

fn material(textures: PbrMaterialTextures) -> PbrMaterial {
    PbrMaterial {
        base_color: vec![1.0, 1.0, 1.0, 1.0],
        emissive: vec![0.0, 0.0, 0.0],
        metallic: 0.0,
        roughness: 1.0,
        alpha_mode: MaterialAlphaMode::Opaque,
        alpha_cutoff: 0.5,
        textures,
    }
}

/// A horizontal two-unit quad at height `y`, facing up, with UVs running +X and +Z from the top-left corner.
fn quad(y: f64, material: PbrMaterial) -> PbrMesh {
    PbrMesh {
        positions: vec![-1.0, y, -1.0, 1.0, y, -1.0, 1.0, y, 1.0, -1.0, y, 1.0],
        normals: [0.0, 1.0, 0.0].repeat(4),
        uvs: vec![0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        colors: vec![1.0; 12],
        alphas: vec![1.0; 4],
        triangles: vec![0, 2, 1, 0, 3, 2],
        triangle_materials: vec![0, 0],
        materials: vec![material],
    }
}

/// A UV sphere of radius one with outward normals.
fn sphere(material: PbrMaterial) -> PbrMesh {
    let (rings, segments) = (24_u32, 48_u32);
    let mut mesh = PbrMesh {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        colors: Vec::new(),
        alphas: Vec::new(),
        triangles: Vec::new(),
        triangle_materials: Vec::new(),
        materials: vec![material],
    };
    for ring in 0..=rings {
        let polar = std::f64::consts::PI * f64::from(ring) / f64::from(rings);
        for segment in 0..=segments {
            let azimuth = std::f64::consts::TAU * f64::from(segment) / f64::from(segments);
            let normal = [
                deterministic::sin(polar) * deterministic::cos(azimuth),
                deterministic::cos(polar),
                deterministic::sin(polar) * deterministic::sin(azimuth),
            ];
            mesh.positions.extend(normal);
            mesh.normals.extend(normal);
            mesh.uvs.extend([f64::from(segment) / f64::from(segments), f64::from(ring) / f64::from(rings)]);
            mesh.colors.extend([1.0; 3]);
            mesh.alphas.push(1.0);
        }
    }
    for ring in 0..rings {
        for segment in 0..segments {
            let a = ring * (segments + 1) + segment;
            let b = a + segments + 1;
            mesh.triangles.extend([a, a + 1, b, a + 1, b + 1, b]);
            mesh.triangle_materials.extend([0, 0]);
        }
    }
    mesh
}

fn asset(mesh: &PbrMesh, textures: &HashMap<&str, TextureAsset>) -> MeshAsset {
    MeshAsset::from_pbr_mesh(mesh, |source| textures.get(source).cloned()).expect("valid PBR mesh")
}

fn render(
    gpu: &GpuContext,
    renderer: &mut Renderer,
    scene: &RenderScene,
    camera: &OrbitCamera,
    inputs: &FrameInputs<'_>,
    name: &str,
) -> Vec<u8> {
    let target = OffscreenTarget::new(gpu, WIDTH, HEIGHT).expect("capture target");
    renderer.resize(WIDTH, HEIGHT, 1.0);
    renderer.render(scene, camera, inputs, target.view()).expect("frame renders");
    let pixels = target.read_rgba(gpu).expect("readback");
    if let Ok(directory) = std::env::var("JOY_RENDER_CAPTURE_DIR") {
        std::fs::create_dir_all(&directory).expect("capture directory");
        let path = std::path::Path::new(&directory).join(format!("{name}.png"));
        image::save_buffer(&path, &pixels, WIDTH, HEIGHT, image::ColorType::Rgba8).expect("png written");
    }
    pixels
}

fn pixel_at(pixels: &[u8], camera: &OrbitCamera, world: Vec3) -> [u8; 3] {
    let frame = camera.camera_frame(WIDTH as f32 / HEIGHT as f32);
    let point = project_to_viewport(world, frame.view_projection(), Vec2::new(WIDTH as f32, HEIGHT as f32))
        .expect("point is on screen");
    let offset = ((point.y as u32 * WIDTH + point.x as u32) * 4) as usize;
    [pixels[offset], pixels[offset + 1], pixels[offset + 2]]
}

fn luminance(color: [u8; 3]) -> f32 {
    (0.2126 * f32::from(color[0])) + (0.7152 * f32::from(color[1])) + (0.0722 * f32::from(color[2]))
}

fn top_down() -> OrbitCamera {
    OrbitCamera { distance: 5.0, pitch: 1.45, yaw: 0.0, ..OrbitCamera::default() }
}

#[test]
fn base_color_maps_sample_by_uv() {
    let Some(gpu) = gpu() else {
        return;
    };
    // Left texel red, right texel blue: U runs along +X.
    let texture = TextureAsset::from_rgba8(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]).unwrap();
    let textures = HashMap::from([("checker.png", texture)]);
    let maps = PbrMaterialTextures { base_color: Some("checker.png".into()), ..Default::default() };
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(asset(&quad(0.0, material(maps)), &textures)).with_shading(Shading::Pbr));
    let camera = top_down();
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    let inputs = FrameInputs { ambient: Some(1.0), ..FrameInputs::default() };
    let pixels = render(&gpu, &mut renderer, &scene, &camera, &inputs, "material-base-color");
    let left = pixel_at(&pixels, &camera, Vec3::new(-0.5, 0.0, 0.0));
    let right = pixel_at(&pixels, &camera, Vec3::new(0.5, 0.0, 0.0));
    assert!(left[0] > 120 && left[2] < 40, "left texel should be red: {left:?}");
    assert!(right[2] > 120 && right[0] < 40, "right texel should be blue: {right:?}");

    // Dropping the scene releases the material; a later frame without it still renders.
    drop(scene);
    render(&gpu, &mut renderer, &RenderScene::new(), &camera, &inputs, "material-released");
}

#[test]
fn normal_maps_tilt_shading_toward_their_direction() {
    let Some(gpu) = gpu() else {
        return;
    };
    // A uniform map tilting the surface toward +U (+X) at about 45 degrees.
    let tilted = TextureAsset::from_rgba8(1, 1, vec![218, 128, 218, 255]).unwrap();
    let textures = HashMap::from([("tilted.png", tilted)]);
    let flat_material = material(PbrMaterialTextures::default());
    let mapped_material = material(PbrMaterialTextures { normal: Some("tilted.png".into()), ..Default::default() });
    let camera = top_down();
    let lights = [PointLight {
        position: Vec3::new(6.0, 1.0, 0.0),
        color: LinearColor::rgb(30.0, 30.0, 30.0),
        range: 20.0,
        casts_shadow: false,
    }];
    let inputs = FrameInputs { ambient: Some(0.0), lights: &lights, ..FrameInputs::default() };
    let mut brightness = Vec::new();
    for (name, material) in [("material-normal-flat", flat_material), ("material-normal-tilted", mapped_material)] {
        let mut scene = RenderScene::new();
        scene.insert(MeshInstance::new(asset(&quad(0.0, material), &textures)).with_shading(Shading::Pbr));
        let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
        let pixels = render(&gpu, &mut renderer, &scene, &camera, &inputs, name);
        brightness.push(luminance(pixel_at(&pixels, &camera, Vec3::ZERO)));
    }
    assert!(brightness[1] > brightness[0] * 1.5, "tilting toward the light should brighten: {brightness:?}");
}

#[test]
fn pbr_surfaces_flip_normals_only_on_back_faces() {
    let Some(gpu) = gpu() else {
        return;
    };
    // Seen from above and lit only from above. The TypeScript pass flips the authored normal on back faces alone, so a
    // front face whose smooth-shaded normal points away from the eye keeps it (and stays dark here), while a back face
    // flips its upward normal down.
    let upward = quad(0.0, material(PbrMaterialTextures::default()));
    let away = PbrMesh { normals: [0.0, -1.0, 0.0].repeat(4), ..upward.clone() };
    let back_facing = PbrMesh { triangles: vec![0, 1, 2, 0, 2, 3], ..upward.clone() };
    let camera = top_down();
    let lights = [PointLight {
        position: Vec3::new(0.0, 3.0, 0.0),
        color: LinearColor::rgb(30.0, 30.0, 30.0),
        range: 20.0,
        casts_shadow: false,
    }];
    let inputs = FrameInputs { ambient: Some(0.0), lights: &lights, ..FrameInputs::default() };
    let mut brightness = Vec::new();
    for (name, mesh) in [("facing-front", upward), ("facing-away", away), ("facing-back", back_facing)] {
        let mut scene = RenderScene::new();
        scene.insert(MeshInstance::new(asset(&mesh, &HashMap::new())).with_shading(Shading::Pbr));
        let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
        let pixels = render(&gpu, &mut renderer, &scene, &camera, &inputs, &format!("material-{name}"));
        brightness.push(luminance(pixel_at(&pixels, &camera, Vec3::ZERO)));
    }
    assert!(brightness[1] * 3.0 < brightness[0], "front faces keep normals that point away: {brightness:?}");
    assert!(brightness[2] * 3.0 < brightness[0], "back faces flip their normals: {brightness:?}");
}

#[test]
fn image_lighting_lights_surfaces_by_orientation() {
    let Some(gpu) = gpu() else {
        return;
    };
    // Bright sky above the horizon, black ground below, so lighting must follow orientation.
    let (width, height) = (64_u32, 32_u32);
    let mut data = vec![0.0_f32; (width * height * 4) as usize];
    for row in 0..height {
        for column in 0..width {
            let offset = ((row * width + column) * 4) as usize;
            let value = if row < height / 2 { 2.0 } else { 0.0 };
            data[offset..offset + 4].copy_from_slice(&[value, value, value, 1.0]);
        }
    }
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    let pixels = EnvironmentPixels { width, height, data: EnvironmentData::Float(&data) };
    let map =
        renderer.prepare_environment_map(&pixels, &EnvironmentMapOptions::default()).expect("environment prepares");
    let bad = EnvironmentPixels { width, height, data: EnvironmentData::Float(&data[4..]) };
    assert!(renderer.prepare_environment_map(&bad, &EnvironmentMapOptions::default()).is_err());

    let mut scene = RenderScene::new();
    scene.insert(
        MeshInstance::new(asset(&sphere(material(PbrMaterialTextures::default())), &HashMap::new()))
            .with_shading(Shading::Pbr),
    );
    let camera = OrbitCamera { distance: 4.0, pitch: 0.0, yaw: 0.0, ..OrbitCamera::default() };
    let dark = FrameInputs { ambient: Some(0.0), ..FrameInputs::default() };
    let lit = FrameInputs {
        ambient: Some(0.0),
        image_lighting: Some(ImageLighting { map: &map, intensity: 1.0, rotation: 0.0 }),
        ..FrameInputs::default()
    };
    let unlit_pixels = render(&gpu, &mut renderer, &scene, &camera, &dark, "material-ibl-off");
    let lit_pixels = render(&gpu, &mut renderer, &scene, &camera, &lit, "material-ibl-on");

    // Sample sphere points whose normals point up, sideways at 45 degrees up and down, and down, facing the camera.
    let frame = camera.camera_frame(WIDTH as f32 / HEIGHT as f32);
    let toward_camera = (frame.eye - Vec3::ZERO).normalize();
    let side = toward_camera.cross(Vec3::Y).normalize();
    let point = |normal: Vec3| (normal + toward_camera * 0.6).normalize();
    let up = luminance(pixel_at(&lit_pixels, &camera, point(Vec3::Y)));
    let upper = luminance(pixel_at(&lit_pixels, &camera, point((side + Vec3::Y).normalize())));
    let lower = luminance(pixel_at(&lit_pixels, &camera, point((side - Vec3::Y).normalize())));
    let down = luminance(pixel_at(&lit_pixels, &camera, point(-Vec3::Y)));
    let without = luminance(pixel_at(&unlit_pixels, &camera, point(Vec3::Y)));
    assert!(without < 3.0, "no image lighting should leave the sphere black: {without}");
    assert!(up > 60.0 && up > down * 3.0, "sky above should light upward normals: up {up}, down {down}");
    assert!(upper > lower * 2.0, "oblique normals must sample the correct cube rows: upper {upper}, lower {lower}");
}

#[test]
fn masked_materials_cast_shadows_with_holes() {
    let Some(gpu) = gpu() else {
        return;
    };
    // Left half opaque, right half transparent: the masked right half must not shadow the floor.
    let alpha = TextureAsset::from_rgba8(2, 1, vec![255, 255, 255, 255, 255, 255, 255, 0]).unwrap();
    let textures = HashMap::from([("cutout.png", alpha)]);
    let cutout = PbrMaterial {
        alpha_mode: MaterialAlphaMode::Mask,
        ..material(PbrMaterialTextures { base_color: Some("cutout.png".into()), ..Default::default() })
    };
    let mut scene = RenderScene::new();
    let floor = quad(0.0, material(PbrMaterialTextures::default()));
    scene.insert(
        MeshInstance::new(asset(&floor, &textures)).with_shading(Shading::Pbr).with_transform(Transform::new(
            Vec3::ZERO,
            Vec3::ZERO,
            Vec3::splat(4.0),
        )),
    );
    scene.insert(MeshInstance::new(asset(&quad(1.0, cutout), &textures)).with_shading(Shading::Pbr));
    let sun = CelestialLight {
        // The light is high toward -Z, so the cutout's shadow falls 0.75 units toward +Z, beyond what it occludes.
        direction: Vec3::new(0.0, 0.8, -0.6),
        color: LinearColor::rgb(1.0, 1.0, 1.0),
        intensity: 4.0,
        angular_radius: 0.00465,
        casts_shadow: true,
    };
    let environment = EnvironmentLighting {
        elapsed_seconds: 0.0,
        time_of_day: 0.5,
        sun,
        moon: CelestialLight { intensity: 0.0, ..sun },
        primary: sun,
        skylight_color: LinearColor::rgb(0.0, 0.0, 0.0),
        skylight_intensity: 0.0,
        wind_velocity: Vec3::ZERO,
        scattering: [1.0, 0.6, 1.0, 1.0],
        fog_and_clouds: [0.0; 4],
        cloud_shape_and_shafts: [1800.0, 700.0, 0.0, 0.94],
        shaft_exposure_and_occlusion: [0.0, 1.0, 7.0],
    };
    let camera = OrbitCamera { distance: 7.0, pitch: 1.2, yaw: 0.0, ..OrbitCamera::default() };
    let options = RendererOptions { atmosphere: false, ..RendererOptions::default() };
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, options);
    let inputs = FrameInputs { environment: Some(&environment), ambient: Some(0.05), ..FrameInputs::default() };
    let pixels = render(&gpu, &mut renderer, &scene, &camera, &inputs, "material-masked-shadow");
    // In the shadow's visible strip, floor under the transparent half matches open floor; under the opaque half it is dark.
    let under_hole = luminance(pixel_at(&pixels, &camera, Vec3::new(0.5, 0.0, 1.6)));
    let open_floor = luminance(pixel_at(&pixels, &camera, Vec3::new(3.0, 0.0, 1.6)));
    let under_opaque = luminance(pixel_at(&pixels, &camera, Vec3::new(-0.5, 0.0, 1.6)));
    assert!(under_hole > open_floor * 0.8, "the hole should not cast a shadow: {under_hole} vs {open_floor}");
    assert!(under_opaque < open_floor * 0.5, "the opaque half should shadow the floor: {under_opaque} vs {open_floor}");
}

/// A studio preview lights PBR surfaces with an environment's primary light over a plain background: the sky can be
/// turned off at runtime, and shadow settings (quality, distance, or off) change between frames on one renderer.
#[test]
fn sky_and_shadow_settings_change_between_frames() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut scene = RenderScene::new();
    scene.insert(
        MeshInstance::new(asset(&quad(0.0, material(PbrMaterialTextures::default())), &HashMap::new()))
            .with_shading(Shading::Pbr)
            .with_transform(Transform::new(Vec3::ZERO, Vec3::ZERO, Vec3::splat(4.0))),
    );
    scene.insert(MeshInstance::new(asset(&quad(1.0, material(PbrMaterialTextures::default())), &HashMap::new())));
    let sun = CelestialLight {
        direction: Vec3::new(0.0, 0.8, -0.6),
        color: LinearColor::rgb(1.0, 1.0, 1.0),
        intensity: 4.0,
        angular_radius: 0.00465,
        casts_shadow: true,
    };
    let environment = EnvironmentLighting {
        elapsed_seconds: 0.0,
        time_of_day: 0.5,
        sun,
        moon: CelestialLight { intensity: 0.0, ..sun },
        primary: sun,
        skylight_color: LinearColor::rgb(0.0, 0.0, 0.0),
        skylight_intensity: 0.0,
        wind_velocity: Vec3::ZERO,
        scattering: [1.0, 0.6, 1.0, 1.0],
        fog_and_clouds: [0.0; 4],
        cloud_shape_and_shafts: [1800.0, 700.0, 0.0, 0.94],
        shaft_exposure_and_occlusion: [0.0, 1.0, 7.0],
    };
    // Looking almost straight down keeps the sky out of the floor samples but in the corners when it draws.
    let camera = OrbitCamera { distance: 9.0, pitch: 0.35, yaw: 0.0, ..OrbitCamera::default() };
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    let background = LinearColor::new(0.08, 0.1, 0.13, 1.0);
    let inputs = FrameInputs {
        environment: Some(&environment),
        ambient: Some(0.05),
        clear_color: background,
        ..FrameInputs::default()
    };
    let with_sky = render(&gpu, &mut renderer, &scene, &camera, &inputs, "material-studio-sky");
    renderer.set_atmosphere(false);
    assert!(!renderer.atmosphere());
    let without_sky = render(&gpu, &mut renderer, &scene, &camera, &inputs, "material-studio-plain");
    let corner = |pixels: &[u8]| [pixels[0], pixels[1], pixels[2]];
    assert_ne!(corner(&with_sky), corner(&without_sky), "the sky fills the corner while it draws");
    let plain = corner(&without_sky);
    assert!(plain.iter().all(|channel| *channel < 90), "the plain background stays dark: {plain:?}");

    let shadowed = luminance(pixel_at(&without_sky, &camera, Vec3::new(0.0, 0.0, 1.6)));
    assert!(renderer.diagnostics().shadows_active);
    let options = renderer.shadow_options().expect("default shadows");
    renderer.set_shadow_options(Some(joy_renderer::ShadowOptions {
        quality: joy_renderer::ShadowQuality::Cinematic,
        shadow_distance: 30.0,
        ..options
    }));
    let cinematic = render(&gpu, &mut renderer, &scene, &camera, &inputs, "material-studio-cinematic");
    assert!(renderer.diagnostics().shadows_active, "reallocated maps keep casting");
    assert_eq!(renderer.diagnostics().shadow_views, 4, "cinematic quality renders four cascades");
    let cinematic_shadow = luminance(pixel_at(&cinematic, &camera, Vec3::new(0.0, 0.0, 1.6)));
    renderer.set_shadow_options(None);
    let unshadowed = render(&gpu, &mut renderer, &scene, &camera, &inputs, "material-studio-unshadowed");
    assert!(!renderer.diagnostics().shadows_active);
    let open = luminance(pixel_at(&unshadowed, &camera, Vec3::new(0.0, 0.0, 1.6)));
    assert!(shadowed < open * 0.5 && cinematic_shadow < open * 0.5, "{shadowed} / {cinematic_shadow} vs {open}");
}
