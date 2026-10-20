// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Deferred and clustered lighting on a real device: deferred shading draws the same image as forward shading, layers
//! the G-buffer cannot hold fall back to forward per material, game shading models opt into deferred lighting (and
//! registration rejects deferred functions that break the contract), and hundreds of point lights each light their own
//! patch through the light clusters.
//!
//! Needs a GPU adapter (llvmpipe or SwiftShader is enough); without one each test prints a note and returns. Set
//! `JOY_RENDER_CAPTURE_DIR` to keep PNG captures. Native only.
#![cfg(not(target_arch = "wasm32"))]

use joy_core::color::srgb_to_linear;
use joy_core::math::{
    DVec3,
    Vec2,
    Vec3,
    deterministic,
};
use joy_core::{
    LinearColor,
    Transform,
};
use joy_render_types::{
    Coat,
    CustomShading,
    Fuzz,
    Haze,
    Material,
    Mesh,
    Shading,
    SurfaceLayer,
    ThinFilm,
};
use joy_renderer::{
    BackendPreference,
    CAPTURE_FORMAT,
    Camera,
    DirectionalLight,
    FrameInputs,
    GpuContext,
    MaterialAsset,
    MeshAsset,
    MeshInstance,
    OffscreenTarget,
    OrbitCamera,
    ParameterDefinition,
    PointLight,
    RenderScene,
    Renderer,
    RendererOptions,
    ShadingModelDefinition,
    project_to_viewport,
};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 240;

fn gpu() -> Option<GpuContext> {
    match pollster::block_on(GpuContext::headless(BackendPreference::Auto)) {
        Ok(gpu) => Some(gpu),
        Err(error) => {
            eprintln!("skipping GPU test: {error}");
            None
        }
    }
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

/// Linear luminance (0-255 scale) of the display pixel `world` projects to.
fn luminance_at(pixels: &[u8], camera: &OrbitCamera, world: Vec3) -> f32 {
    let frame = camera.camera_frame(WIDTH as f32 / HEIGHT as f32);
    let point = project_to_viewport(world, frame.view_projection(), Vec2::new(WIDTH as f32, HEIGHT as f32))
        .expect("point is on screen");
    let offset = ((point.y as u32 * WIDTH + point.x as u32) * 4) as usize;
    let linear = |channel: u8| (srgb_to_linear(f64::from(pixels[offset + channel as usize]) / 255.0) * 255.0) as f32;
    (0.2126 * linear(0)) + (0.7152 * linear(1)) + (0.0722 * linear(2))
}

/// A horizontal square of half-size `extent` at height zero, facing up.
fn ground(extent: f64) -> Mesh {
    Mesh {
        normals: Some(vec![Vec3::Y; 4]),
        ..Mesh::new(
            vec![
                DVec3::new(-extent, 0.0, -extent),
                DVec3::new(extent, 0.0, -extent),
                DVec3::new(extent, 0.0, extent),
                DVec3::new(-extent, 0.0, extent),
            ],
            vec![0, 2, 1, 0, 3, 2],
        )
    }
}

/// A UV sphere of radius one with outward normals.
fn sphere() -> Mesh {
    let (rings, segments) = (20_u32, 40_u32);
    let (mut positions, mut normals, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    for ring in 0..=rings {
        let polar = std::f64::consts::PI * f64::from(ring) / f64::from(rings);
        for segment in 0..=segments {
            let azimuth = std::f64::consts::TAU * f64::from(segment) / f64::from(segments);
            let normal = DVec3::new(
                deterministic::sin(polar) * deterministic::cos(azimuth),
                deterministic::cos(polar),
                deterministic::sin(polar) * deterministic::sin(azimuth),
            );
            positions.push(normal);
            normals.push(normal.as_vec3());
        }
    }
    for ring in 0..rings {
        for segment in 0..segments {
            let a = ring * (segments + 1) + segment;
            let b = a + segments + 1;
            indices.extend([a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    Mesh {
        normals: Some(normals),
        ..Mesh::new(positions, indices)
    }
}

fn asset(mesh: &Mesh, material: Material) -> MeshAsset {
    let material = MaterialAsset::new(material).expect("valid material");
    MeshAsset::with_materials(mesh, vec![material]).expect("valid mesh")
}

fn light(position: Vec3, color: f32, range: f32, casts_shadow: bool) -> PointLight {
    PointLight {
        position,
        color: LinearColor::rgb(color, color * 0.9, color * 0.8),
        range,
        casts_shadow,
    }
}

/// A lit ground under five layered spheres, one per layer feature (plain, haze, coat, fuzz, and a thin film the
/// G-buffer cannot hold), with a shadowing key light and point lights, two of them casting.
fn studio() -> RenderScene {
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(asset(&ground(8.0), Material::lit(LinearColor::rgb(0.55, 0.5, 0.45)))));
    let base = SurfaceLayer::dielectric(LinearColor::rgb(0.7, 0.2, 0.15), 0.35);
    let layers = [
        base.clone(),
        SurfaceLayer {
            haze: Some(Haze {
                roughness: 0.8,
                weight: 0.5,
            }),
            ..base.clone()
        },
        SurfaceLayer {
            coat: Some(Coat {
                weight: 1.0,
                roughness: 0.1,
                ..Coat::default()
            }),
            ..base.clone()
        },
        SurfaceLayer {
            fuzz: Some(Fuzz {
                color: LinearColor::rgb(0.6, 0.6, 0.9),
                roughness: 0.5,
                amount: 1.0,
            }),
            ..base.clone()
        },
        SurfaceLayer {
            thin_film: Some(ThinFilm {
                thickness: 400.0,
                ior: 1.4,
                weight: 1.0,
            }),
            metallic: 1.0,
            base_color: LinearColor::rgb(0.9, 0.85, 0.8),
            ..base
        },
    ];
    let sphere = sphere();
    for (index, layer) in layers.into_iter().enumerate() {
        let x = (index as f32 - 2.0) * 2.4;
        let transform = Transform::new(Vec3::new(x, 1.0, 0.0), Vec3::ZERO, Vec3::ONE);
        scene.insert(MeshInstance::new(asset(&sphere, Material::layered(layer))).with_transform(transform));
    }
    scene
}

fn studio_camera() -> OrbitCamera {
    OrbitCamera {
        distance: 14.0,
        pitch: 0.45,
        yaw: 0.2,
        target: Vec3::new(0.0, 0.8, 0.0),
        ..OrbitCamera::default()
    }
}

#[test]
fn deferred_lighting_matches_forward_shading() {
    let Some(gpu) = gpu() else {
        return;
    };
    let scene = studio();
    let camera = studio_camera();
    let lights = [
        light(Vec3::new(-3.5, 2.5, 2.0), 6.0, 7.0, true),
        light(Vec3::new(3.5, 2.0, 2.5), 5.0, 6.0, true),
        light(Vec3::new(0.0, 0.6, 2.2), 3.0, 3.0, false),
        light(Vec3::new(-6.0, 0.8, -1.5), 4.0, 4.0, false),
    ];
    let inputs = FrameInputs {
        directional_light: Some(DirectionalLight {
            direction: Vec3::new(0.4, 0.8, 0.3),
            color: LinearColor::WHITE,
            intensity: 1.5,
            angular_radius: 0.02,
            casts_shadow: true,
        }),
        lights: &lights,
        ambient: Some(0.15),
        clear_color: LinearColor::rgb(0.02, 0.025, 0.03),
        ..FrameInputs::default()
    };

    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    let deferred = render(&gpu, &mut renderer, &scene, &camera, &inputs, "lighting-deferred");
    let diagnostics = renderer.diagnostics().clone();
    if !renderer.deferred_supported() {
        assert!(!diagnostics.deferred, "devices without deferred support shade forward");
        return;
    }
    assert!(diagnostics.deferred);
    assert_eq!(diagnostics.lighting_passes, 2, "the lit ground and the layered spheres each light in one pass");
    assert_eq!(diagnostics.forward_opaque_batches, 1, "the thin-film sphere shades forward");
    assert_eq!(diagnostics.shadow_point_lights, 2);

    renderer.set_deferred(false);
    let forward = render(&gpu, &mut renderer, &scene, &camera, &inputs, "lighting-forward");
    assert!(!renderer.diagnostics().deferred);

    // The G-buffer quantizes colors, F0, roughness, and normals, and positions come back from depth, so pixels differ
    // by a few levels at most, and on average by far less than one.
    let differences: Vec<u8> = deferred
        .iter()
        .zip(&forward)
        .enumerate()
        .filter(|(index, _)| index % 4 != 3)
        .map(|(_, (a, b))| a.abs_diff(*b))
        .collect();
    let mean = differences.iter().map(|value| f64::from(*value)).sum::<f64>() / differences.len() as f64;
    let mut sorted = differences.clone();
    sorted.sort_unstable();
    let percentile = sorted[(sorted.len() * 999) / 1000];
    assert!(mean < 0.35, "mean channel difference {mean}");
    assert!(percentile <= 8, "99.9th percentile channel difference {percentile}");

    // Both paths light the scene: the key light's shadow and the point lights show on the ground.
    let lit_ground = luminance_at(&deferred, &camera, Vec3::new(-3.5, 0.0, 2.6));
    let far_ground = luminance_at(&deferred, &camera, Vec3::new(6.5, 0.0, -6.5));
    assert!(
        lit_ground > far_ground + 10.0,
        "the point light brightens the ground beneath it: {lit_ground} vs {far_ground}"
    );
}

#[test]
fn hundreds_of_point_lights_each_light_their_own_patch() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut scene = RenderScene::new();
    scene.insert(MeshInstance::new(asset(&ground(24.0), Material::lit(LinearColor::WHITE))));

    // A 20 by 20 grid two units apart; each light reaches 0.9 units, so the ground between lights stays dark.
    let mut lights = Vec::new();
    for row in 0..20 {
        for column in 0..20 {
            let position = Vec3::new((column as f32 * 2.0) - 19.0, 0.5, (row as f32 * 2.0) - 19.0);
            lights.push(light(position, 6.0, 0.9, false));
        }
    }
    let inputs = FrameInputs {
        lights: &lights,
        ambient: Some(0.02),
        ..FrameInputs::default()
    };
    let camera = OrbitCamera {
        distance: 56.0,
        radius: 30.0,
        pitch: 1.5,
        yaw: 0.0,
        ..OrbitCamera::default()
    };

    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    for deferred in [true, false] {
        renderer.set_deferred(deferred);
        let name = if deferred { "lighting-grid-deferred" } else { "lighting-grid-forward" };
        let pixels = render(&gpu, &mut renderer, &scene, &camera, &inputs, name);
        let diagnostics = renderer.diagnostics();
        assert_eq!(diagnostics.point_lights, 400, "every light is drawn");
        assert_eq!(diagnostics.cluster_references_dropped, 0);
        assert!(diagnostics.cluster_light_references >= 400);

        // The first, a middle, and the last light each light the ground under them; between lights stays dark.
        for index in [0, 210, 399] {
            let under = lights[index].position * Vec3::new(1.0, 0.0, 1.0);
            let between = under + Vec3::new(1.0, 0.0, 1.0);
            let lit = luminance_at(&pixels, &camera, under);
            let dark = luminance_at(&pixels, &camera, between);
            assert!(lit > dark + 20.0, "light {index} ({name}): {lit} under it vs {dark} between lights");
        }
    }
}

/// A game's two-band shading model; `deferred` appends the functions that opt it into deferred lighting.
fn banded_model(name: &str, deferred: &str) -> ShadingModelDefinition {
    let source = format!(
        "{}\n{deferred}",
        r#"
const MATERIAL_RECEIVES_LIGHT: bool = true;

struct MaterialParameters {
  coverage: vec4f,
  color: vec4f,
};

@group(3) @binding(0) var<uniform> material: MaterialParameters;

struct MaterialSurface {
  normal: vec3f,
  color: vec3f,
};

fn materialSurface(input: SurfaceInput) -> MaterialSurface {
  var surface: MaterialSurface;
  surface.normal = surfaceNormal(input);
  surface.color = input.color.rgb*material.color.rgb;
  return surface;
}

fn materialOpacity(input: SurfaceInput) -> f32 {
  return 1.0;
}

fn materialAlpha(surface: MaterialSurface) -> f32 {
  return 1.0;
}

fn materialDirect(surface: MaterialSurface, light: LightSample) -> vec3f {
  return surface.color*light.radiance*step(0.3, dot(surface.normal, light.direction));
}

fn materialIndirect(surface: MaterialSurface, ambient: AmbientLight) -> vec3f {
  return surface.color*ambientIrradiance(ambient, surface.normal);
}

fn materialEmission(surface: MaterialSurface) -> vec3f {
  return vec3f(0.0);
}

fn materialAttributes(surface: MaterialSurface) -> SurfaceAttributes {
  var attributes: SurfaceAttributes;
  attributes.normal = surface.normal;
  attributes.roughness = 1.0;
  attributes.diffuseAlbedo = surface.color;
  attributes.specularColor = vec3f(0.0);
  attributes.metallic = 0.0;
  attributes.emissive = vec3f(0.0);
  attributes.occlusion = 1.0;
  return attributes;
}
"#
    );
    ShadingModelDefinition {
        name: name.to_owned(),
        source: source.into(),
        parameters: vec![ParameterDefinition {
            name: "color".to_owned(),
            default: [1.0; 4],
        }],
        textures: Vec::new(),
    }
}

const BANDED_ENCODE: &str = r#"
fn materialEncode(surface: MaterialSurface) -> vec3u {
  return vec3u(packRgb9e5(surface.color), 0u, 0u);
}
"#;

const BANDED_DECODE: &str = r#"
fn materialDecode(payload: vec3u, normal: vec3f, view: vec3f) -> MaterialSurface {
  var surface: MaterialSurface;
  surface.normal = normal;
  surface.color = unpackRgb9e5(payload.x);
  return surface;
}
"#;

/// Decoding from the material group, which the lighting pass does not bind.
const BANDED_DECODE_FROM_MATERIAL: &str = r#"
fn materialDecode(payload: vec3u, normal: vec3f, view: vec3f) -> MaterialSurface {
  var surface: MaterialSurface;
  surface.normal = normal;
  surface.color = material.color.rgb;
  return surface;
}
"#;

#[test]
fn game_shading_models_opt_into_deferred_lighting() {
    let Some(gpu) = gpu() else {
        return;
    };
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, RendererOptions::default());
    renderer.register_shading_model(banded_model("banded", "")).expect("a forward model registers");
    let deferred_functions = format!("{BANDED_ENCODE}\n{BANDED_DECODE}");
    renderer
        .register_shading_model(banded_model("banded-deferred", &deferred_functions))
        .expect("a deferred model registers");

    // Broken deferred functions report at registration instead of failing inside the device.
    let half = renderer.register_shading_model(banded_model("banded-half", BANDED_ENCODE)).unwrap_err();
    assert!(half.to_string().contains("materialDecode"), "{half}");
    let reads_material = format!("{BANDED_ENCODE}\n{BANDED_DECODE_FROM_MATERIAL}");
    let unbound = renderer.register_shading_model(banded_model("banded-unbound", &reads_material)).unwrap_err();
    assert!(unbound.to_string().contains("must not read"), "{unbound}");
    assert!(renderer.register_shading_model(banded_model("banded", "")).is_err(), "names stay unique");

    let banded = |model: &str| {
        Material::new(Shading::Custom(CustomShading {
            model: model.to_owned(),
            ..CustomShading::default()
        }))
    };
    let scene_with = |model: &str| {
        let mut scene = RenderScene::new();
        scene.insert(MeshInstance::new(asset(&ground(6.0), Material::lit(LinearColor::rgb(0.5, 0.5, 0.5)))));
        let transform = Transform::new(Vec3::new(0.0, 1.0, 0.0), Vec3::ZERO, Vec3::ONE);
        scene.insert(MeshInstance::new(asset(&sphere(), banded(model))).with_transform(transform));
        scene
    };
    let lights = [light(Vec3::new(2.0, 3.0, 2.0), 6.0, 8.0, true)];
    let inputs = FrameInputs {
        lights: &lights,
        ambient: Some(0.1),
        ..FrameInputs::default()
    };
    let camera = studio_camera();

    let deferred_scene = scene_with("banded-deferred");
    let deferred = render(&gpu, &mut renderer, &deferred_scene, &camera, &inputs, "lighting-banded-deferred");
    if !renderer.deferred_supported() {
        return;
    }
    assert_eq!(renderer.diagnostics().lighting_passes, 2, "the lit ground and the game's model each light in a pass");
    assert_eq!(renderer.diagnostics().forward_opaque_batches, 0);
    renderer.set_deferred(false);
    let forward = render(&gpu, &mut renderer, &deferred_scene, &camera, &inputs, "lighting-banded-forward");
    let mean =
        deferred.iter().zip(&forward).map(|(a, b)| f64::from(a.abs_diff(*b))).sum::<f64>() / deferred.len() as f64;
    assert!(mean < 0.35, "the game's model shades alike on both paths: {mean}");

    renderer.set_deferred(true);
    render(&gpu, &mut renderer, &scene_with("banded"), &camera, &inputs, "lighting-banded-forward-model");
    assert_eq!(
        renderer.diagnostics().forward_opaque_batches,
        1,
        "a model without the deferred functions shades forward"
    );
    assert_eq!(renderer.diagnostics().lighting_passes, 1);
}
