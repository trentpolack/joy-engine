// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Camera-facing particle geometry for the renderer's transparent queue.
//!
//! Geometry is generated in `f64` with fixed polygon shapes, vertex order, and arithmetic order, so it reproduces on
//! every target and matches the golden geometry recordings within their tolerances; it is sorted back to front in
//! `f64` and only then converted to the renderer's single-precision vertices. Building geometry never advances the
//! simulation or consumes randomness.
//!
//! Coordinates and sizes are world units; colors are linear HDR with straight alpha (negative channels clamp to zero and
//! alpha to at most one). Textured and flipbook sprites carry top-left-origin UVs inset by half a texel so linear
//! filtering cannot bleed between atlas frames.

use std::f64::consts::PI;
use std::sync::{
    Arc,
    OnceLock,
};

use crate::{
    CameraBasis,
    ColorVertex,
    TexturedVertex,
};

use joy_core::math::DVec3;
use joy_core::math::deterministic;
use joy_fx::{
    CompiledParticleEffect,
    EffectParticle,
    ParticleEffect,
    ParticleRenderer,
    ParticleTexture,
    ParticleTextureDimensions,
};

/// Floats per colored vertex in the reference stream (XYZ, RGBA).
const COLORED_STRIDE: usize = 7;

/// Floats per textured vertex in the reference stream (XYZ, RGBA, UV).
const TEXTURED_STRIDE: usize = 9;

/// Samples of the round particle outline.
const ROUND_SAMPLES: usize = 16;

/// Samples of the billow outline.
const BILLOW_SAMPLES: usize = 24;

/// Seconds of velocity a streak ribbon trails behind its particle.
const STREAK_SECONDS: f64 = 0.035;

/// An orthonormal world-space camera basis; particle sizes are world units along `right` and `up`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParticleCameraBasis {
    /// Screen-right direction.
    pub right: [f64; 3],
    /// Screen-up direction.
    pub up: [f64; 3],
    /// Direction from the scene toward the viewer.
    pub backward: [f64; 3],
}

/// Fixed-plane basis for orthographic XY games: right +X, up +Y, viewer on +Z.
pub const PARTICLE_XY_BASIS: ParticleCameraBasis = ParticleCameraBasis {
    right: [1.0, 0.0, 0.0],
    up: [0.0, 1.0, 0.0],
    backward: [0.0, 0.0, 1.0],
};

impl From<CameraBasis> for ParticleCameraBasis {
    fn from(basis: CameraBasis) -> Self {
        let widen = |vector: joy_core::math::Vec3| [f64::from(vector.x), f64::from(vector.y), f64::from(vector.z)];
        Self {
            right: widen(basis.right),
            up: widen(basis.up),
            backward: widen(basis.backward),
        }
    }
}

/// One transparent material run: consecutive back-to-front triangles sharing a material.
#[derive(Clone, Debug, PartialEq)]
pub enum ParticleBatch {
    /// Vertex-color triangles.
    Colored(Vec<ColorVertex>),
    /// Triangles sampling an embedded PNG.
    Textured {
        /// The `data:image/png;base64,…` source, the key into a `ParticleTextureSet`.
        source: Arc<str>,
        /// Three vertices per triangle.
        vertices: Vec<TexturedVertex>,
    },
}

impl ParticleBatch {
    /// The texture source of a textured run.
    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Colored(_) => None,
            Self::Textured {
                source,
                ..
            } => Some(source),
        }
    }

    /// Vertices in the run.
    pub fn vertex_count(&self) -> usize {
        match self {
            Self::Colored(vertices) => vertices.len(),
            Self::Textured {
                vertices,
                ..
            } => vertices.len(),
        }
    }
}

/// Per-emitter geometry counters for one batch build. Times are milliseconds; no GPU timing is implied.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleRenderMetricsRow {
    /// Emitter id.
    pub id: String,
    /// Triangles generated.
    pub triangles: u64,
    /// Vertices generated.
    pub vertices: u64,
    /// CPU time spent generating geometry.
    pub geometry_ms: f64,
}

/// Caller-owned geometry metrics, reset and filled by each [`create_particle_effect_batches`] call.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParticleRenderMetrics {
    /// One row per emitter; additional triangles are not counted.
    pub emitters: Vec<ParticleRenderMetricsRow>,
    /// CPU time spent sorting and batching.
    pub sort_ms: f64,
}

/// Create zeroed metrics for `definition`, one row per emitter.
pub fn create_particle_render_metrics(definition: &CompiledParticleEffect) -> ParticleRenderMetrics {
    ParticleRenderMetrics {
        emitters: definition
            .emitters()
            .iter()
            .map(|emitter| ParticleRenderMetricsRow {
                id: emitter.asset.id.clone(),
                ..Default::default()
            })
            .collect(),
        sort_ms: 0.0,
    }
}

/// Particle geometry could not be built.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParticleRenderError {
    /// [`append_particle_effect`] received an effect with textured or flipbook emitters.
    #[error("Textured particles require create_particle_effect_batches and a texture-capable renderer.")]
    TexturedEffect,
    /// A textured emitter had no texture (impossible for compiled effects).
    #[error("Textured particle emitter requires a texture.")]
    MissingTexture,
    /// Additional triangles were not a whole number of triangles.
    #[error("Additional particle geometry requires complete XYZ/RGBA triangles.")]
    IncompleteTriangles,
    /// Additional triangles contained non-finite values.
    #[error("Particle geometry must contain finite vertex values.")]
    NonFiniteGeometry,
}

/// Append camera-facing colored particles to an unsorted transparent triangle list.
///
/// The renderer sorts [`FrameInputs::transparent_triangles`](crate::FrameInputs) back to front, so the output
/// can be passed there directly. Fails for effects with textured or flipbook emitters; use
/// [`create_particle_effect_batches`] for those.
pub fn append_particle_effect(
    vertices: &mut Vec<ColorVertex>,
    effect: &ParticleEffect,
    camera: &ParticleCameraBasis,
) -> Result<(), ParticleRenderError> {
    let emitters = effect.definition().emitters();
    if emitters.iter().any(|emitter| emitter.asset.renderer.is_textured()) {
        return Err(ParticleRenderError::TexturedEffect);
    }
    let mut scratch = Vec::new();
    for particle in effect.particles() {
        if !is_visible(particle) {
            continue;
        }
        let Some(emitter) = emitters.get(particle.emitter_index) else {
            continue;
        };
        scratch.clear();
        append_colored_particle(&mut scratch, particle, emitter.asset.renderer, camera);
        vertices.extend(scratch.as_chunks::<COLORED_STRIDE>().0.iter().map(|vertex| colored_vertex(vertex)));
    }
    Ok(())
}

/// Build owned transparent material runs for one effect, sorted globally back to front by triangle depth.
///
/// Consecutive triangles sharing a material merge without changing blend order. `additional_triangles` (complete,
/// finite, untextured triangles such as editor reference lines) are interleaved at their camera depth. `metrics`, when
/// supplied, is reset and filled. Equal depths keep generation order.
pub fn create_particle_effect_batches(
    effect: &ParticleEffect,
    camera: &ParticleCameraBasis,
    metrics: Option<&mut ParticleRenderMetrics>,
    additional_triangles: &[ColorVertex],
) -> Result<Vec<ParticleBatch>, ParticleRenderError> {
    let mut metrics = metrics;
    let mut triangles = collect_particle_triangles(effect, camera, metrics.as_deref_mut(), true)?;
    if !additional_triangles.len().is_multiple_of(3) {
        return Err(ParticleRenderError::IncompleteTriangles);
    }
    for triangle in additional_triangles.as_chunks::<3>().0 {
        let corner = |vertex: &ColorVertex| ParticleCorner {
            position: vertex.position.map(f64::from),
            color: vertex.color.map(f64::from),
            uv: [0.0; 2],
        };
        let corners = [corner(&triangle[0]), corner(&triangle[1]), corner(&triangle[2])];
        let finite =
            corners.iter().all(|corner| corner.position.iter().chain(&corner.color).all(|value| value.is_finite()));
        if !finite {
            return Err(ParticleRenderError::NonFiniteGeometry);
        }
        triangles.push(ParticleTriangle {
            corners,
            source: None,
        });
    }

    let sort_started = metrics.as_ref().map(|_| web_time::Instant::now());
    let depths = triangles.iter().map(|triangle| triangle.depth(&camera.backward)).collect();
    let batches = batch_particle_triangles(triangles, depths);
    if let (Some(metrics), Some(started)) = (metrics, sort_started) {
        metrics.sort_ms = started.elapsed().as_secs_f64() * 1000.0;
    }
    Ok(batches)
}

// ---------------------------------------------------------------------------------------------------------------------
// Triangles shared with AssetScene
// ---------------------------------------------------------------------------------------------------------------------

/// One generated vertex in reference precision.
#[derive(Clone, Copy, Debug, PartialEq)]
#[doc(hidden)]
pub struct ParticleCorner {
    pub position: [f64; 3],
    pub color: [f64; 4],
    pub uv: [f64; 2],
}

/// One generated triangle and its material (`None` for vertex color).
#[derive(Clone, Debug, PartialEq)]
#[doc(hidden)]
pub struct ParticleTriangle {
    pub corners: [ParticleCorner; 3],
    pub source: Option<Arc<str>>,
}

impl ParticleTriangle {
    /// Sum of the corners' projections on `backward` (three times the centroid depth), accumulated corner by corner in
    /// a fixed order so recorded back-to-front sorts reproduce exactly.
    pub fn depth(&self, backward: &[f64; 3]) -> f64 {
        let mut depth = 0.0;
        for corner in &self.corners {
            for (value, direction) in corner.position.iter().zip(backward) {
                depth += value * direction;
            }
        }
        depth
    }
}

/// Generate every visible particle's triangles in generation order. Textured emitters are skipped unless
/// `include_textured` is set. `metrics`, when supplied, is reset and its emitter rows filled.
#[doc(hidden)]
pub fn collect_particle_triangles(
    effect: &ParticleEffect,
    camera: &ParticleCameraBasis,
    metrics: Option<&mut ParticleRenderMetrics>,
    include_textured: bool,
) -> Result<Vec<ParticleTriangle>, ParticleRenderError> {
    let mut metrics = metrics;
    if let Some(metrics) = metrics.as_deref_mut() {
        metrics.sort_ms = 0.0;
        for row in &mut metrics.emitters {
            row.triangles = 0;
            row.vertices = 0;
            row.geometry_ms = 0.0;
        }
    }

    let emitters = effect.definition().emitters();
    let mut triangles = Vec::new();
    let mut scratch = Vec::new();
    for particle in effect.particles() {
        if !is_visible(particle) {
            continue;
        }
        let Some(emitter) = emitters.get(particle.emitter_index) else {
            continue;
        };
        let renderer = emitter.asset.renderer;
        if renderer.is_textured() && !include_textured {
            continue;
        }
        let started = metrics.as_ref().map(|_| web_time::Instant::now());

        scratch.clear();
        let (stride, source) = if renderer.is_textured() {
            let (Some(texture), Some(dimensions)) = (&emitter.asset.texture, emitter.texture_dimensions) else {
                return Err(ParticleRenderError::MissingTexture);
            };
            let animated = renderer == ParticleRenderer::Flipbook;
            append_textured_particle(&mut scratch, particle, texture, animated, camera, dimensions);
            (TEXTURED_STRIDE, Some(texture.source.clone()))
        } else {
            append_colored_particle(&mut scratch, particle, renderer, camera);
            (COLORED_STRIDE, None)
        };
        for triangle in scratch.chunks_exact(stride * 3) {
            let corner = |index: usize| {
                let values = &triangle[index * stride..(index + 1) * stride];
                let uv = if stride == TEXTURED_STRIDE { [values[7], values[8]] } else { [0.0; 2] };
                ParticleCorner {
                    position: [values[0], values[1], values[2]],
                    color: [values[3], values[4], values[5], values[6]],
                    uv,
                }
            };
            triangles.push(ParticleTriangle {
                corners: [corner(0), corner(1), corner(2)],
                source: source.clone(),
            });
        }

        if let (Some(metrics), Some(started)) = (metrics.as_deref_mut(), started)
            && let Some(row) = metrics.emitters.get_mut(particle.emitter_index)
        {
            row.vertices += (scratch.len() / stride) as u64;
            row.triangles += (scratch.len() / (stride * 3)) as u64;
            row.geometry_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
    }
    Ok(triangles)
}

/// Stable-sort triangles by `depths` (ascending: farthest first) and merge consecutive runs of the same material.
#[doc(hidden)]
pub fn batch_particle_triangles(triangles: Vec<ParticleTriangle>, depths: Vec<f64>) -> Vec<ParticleBatch> {
    let mut order: Vec<usize> = (0..triangles.len()).collect();
    order.sort_by(|left, right| depths[*left].partial_cmp(&depths[*right]).unwrap_or(std::cmp::Ordering::Equal));

    let mut batches: Vec<ParticleBatch> = Vec::new();
    for index in order {
        let triangle = &triangles[index];
        let same_material = match (batches.last(), &triangle.source) {
            (Some(ParticleBatch::Colored(_)), None) => true,
            (
                Some(ParticleBatch::Textured {
                    source,
                    ..
                }),
                Some(next),
            ) => source.as_ref() == next.as_ref(),
            _ => false,
        };
        if !same_material {
            batches.push(match &triangle.source {
                None => ParticleBatch::Colored(Vec::new()),
                Some(source) => ParticleBatch::Textured {
                    source: source.clone(),
                    vertices: Vec::new(),
                },
            });
        }
        match batches.last_mut() {
            Some(ParticleBatch::Colored(vertices)) => {
                vertices.extend(triangle.corners.iter().map(|corner| ColorVertex {
                    position: corner.position.map(|value| value as f32),
                    color: corner.color.map(|value| value as f32),
                }));
            }
            Some(ParticleBatch::Textured {
                vertices,
                ..
            }) => {
                vertices.extend(triangle.corners.iter().map(|corner| TexturedVertex {
                    position: corner.position.map(|value| value as f32),
                    color: corner.color.map(|value| value as f32),
                    uv: corner.uv.map(|value| value as f32),
                }));
            }
            None => {}
        }
    }
    batches
}

// ---------------------------------------------------------------------------------------------------------------------
// Geometry
// ---------------------------------------------------------------------------------------------------------------------

fn is_visible(particle: &EffectParticle) -> bool {
    particle.alpha > 0.0 && particle.size > 0.0 && particle.age < particle.lifetime
}

fn colored_vertex(values: &[f64]) -> ColorVertex {
    ColorVertex {
        position: [values[0] as f32, values[1] as f32, values[2] as f32],
        color: [values[3] as f32, values[4] as f32, values[5] as f32, values[6] as f32],
    }
}

/// The unit round outline.
const ROUND: [f64; ROUND_SAMPLES] = [1.0; ROUND_SAMPLES];

/// The lobed billow outline, computed once.
fn billow_shape() -> &'static [f64; BILLOW_SAMPLES] {
    static SHAPE: OnceLock<[f64; BILLOW_SAMPLES]> = OnceLock::new();
    SHAPE.get_or_init(|| {
        std::array::from_fn(|index| {
            let angle = ((index as f64 / BILLOW_SAMPLES as f64) * PI) * 2.0;
            (1.0 + (deterministic::sin(angle * 3.0) * 0.09)) + (deterministic::cos(angle * 5.0) * 0.05)
        })
    })
}

/// Append one particle's colored triangles (stride 7) in world space.
fn append_colored_particle(
    vertices: &mut Vec<f64>,
    particle: &EffectParticle,
    renderer: ParticleRenderer,
    camera: &ParticleCameraBasis,
) {
    let size = particle.size;
    let rotation = particle.rotation;
    let color = [
        f64::max(0.0, particle.r),
        f64::max(0.0, particle.g),
        f64::max(0.0, particle.b),
        f64::min(1.0, particle.alpha),
    ];
    let edge = [color[0], color[1], color[2], 0.0];

    // Local billboard geometry around the origin; streak ribbons are written directly in world space first.
    let mut local = Vec::new();
    match renderer {
        ParticleRenderer::Circle => add_radial_polygon(&mut local, 0.0, 0.0, size, color, &ROUND, rotation, 0.0),
        ParticleRenderer::Square => append_quad(&mut local, size, rotation, color),
        ParticleRenderer::Ring => add_ring(&mut local, 0.0, 0.0, size, f64::max(0.15, size * 0.045), color, 32, 0.0),
        ParticleRenderer::Streak => {
            append_streak(vertices, particle, color, camera);
            add_gradient_radial_polygon(&mut local, 0.0, 0.0, size * 1.7, color, edge, &ROUND, rotation, 0.0);
        }
        ParticleRenderer::Billow => {
            // Overlapping feathered lobes give volume without project-specific code.
            let shape = billow_shape();
            add_gradient_radial_polygon(&mut local, 0.0, 0.0, size, color, edge, shape, rotation, 0.0);
            let inner = [color[0] * 1.5, color[1] * 1.5, color[2] * 1.5, color[3] * 0.65];
            add_gradient_radial_polygon(
                &mut local,
                0.0 - (size * 0.16),
                0.0 - (size * 0.18),
                size * 0.65,
                inner,
                edge,
                shape,
                rotation + 0.7,
                0.0 + 0.01,
            );
        }
        _ => add_gradient_radial_polygon(&mut local, 0.0, 0.0, size, color, edge, &ROUND, rotation, 0.0),
    }

    let origin = [particle.x, particle.y, particle.z];
    for vertex in local.as_chunks::<COLORED_STRIDE>().0 {
        let axes = origin.iter().zip(camera.right).zip(camera.up).zip(camera.backward);
        for (((origin, right), up), backward) in axes {
            vertices.push(((origin + (vertex[0] * right)) + (vertex[1] * up)) + (vertex[2] * backward));
        }
        vertices.extend_from_slice(&vertex[3..7]);
    }
}

/// A world-space velocity ribbon, including motion toward or away from the camera.
fn append_streak(out: &mut Vec<f64>, particle: &EffectParticle, color: [f64; 4], camera: &ParticleCameraBasis) {
    let velocity = [particle.vx, particle.vy, particle.vz];
    let view = camera.backward;
    let side = [
        (velocity[1] * view[2]) - (velocity[2] * view[1]),
        (velocity[2] * view[0]) - (velocity[0] * view[2]),
        (velocity[0] * view[1]) - (velocity[1] * view[0]),
    ];
    let length = DVec3::from_array(side).length();
    let side = if length > 1e-8 { side.map(|value| value / length) } else { camera.right };
    let head = [particle.x, particle.y, particle.z];
    // A fixed 35 ms velocity trail gives the ribbon a world-space length independent of frame rate.
    let tail = [
        head[0] - (velocity[0] * STREAK_SECONDS),
        head[1] - (velocity[1] * STREAK_SECONDS),
        head[2] - (velocity[2] * STREAK_SECONDS),
    ];
    for (point, direction) in [(head, -1.0), (tail, -1.0), (tail, 1.0), (head, -1.0), (tail, 1.0), (head, 1.0)] {
        for axis in 0..3 {
            out.push(point[axis] + (((side[axis] * particle.size) * direction) * 0.5));
        }
        out.extend_from_slice(&color);
    }
}

/// A rotated billboard with `size` as its half extent.
fn append_quad(out: &mut Vec<f64>, size: f64, rotation: f64, color: [f64; 4]) {
    let cosine = deterministic::cos(rotation);
    let sine = deterministic::sin(rotation);
    for (x, y) in [(-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
        out.push(((x * cosine) - (y * sine)) * size);
        out.push(((x * sine) + (y * cosine)) * size);
        out.push(0.0);
        out.extend_from_slice(&color);
    }
}

/// Append one textured or flipbook sprite (stride 9) in world space.
fn append_textured_particle(
    out: &mut Vec<f64>,
    particle: &EffectParticle,
    texture: &ParticleTexture,
    animated: bool,
    camera: &ParticleCameraBasis,
    dimensions: ParticleTextureDimensions,
) {
    let color = [
        f64::max(0.0, particle.r),
        f64::max(0.0, particle.g),
        f64::max(0.0, particle.b),
        f64::min(1.0, particle.alpha),
    ];
    let mut local = Vec::with_capacity(6 * COLORED_STRIDE);
    append_quad(&mut local, particle.size, particle.rotation, color);

    let frames = f64::from(texture.frames);
    let columns = f64::from(texture.columns);
    let rows = f64::from(texture.rows);
    let mut frame = 0.0;
    if animated {
        let elapsed_frames =
            if texture.fps > 0.0 { particle.age * texture.fps } else { (particle.age / particle.lifetime) * frames };
        let index = f64::max(0.0, elapsed_frames.floor());
        frame = if texture.looping { index % frames } else { f64::min(frames - 1.0, index) };
    }
    let column = frame % columns;
    let row = (frame / columns).floor();
    // Inset atlas UVs by half a source texel so linear filtering cannot sample adjacent frames.
    let width = f64::from(dimensions.width);
    let height = f64::from(dimensions.height);
    let left = (column / columns) + (0.5 / width);
    let right = ((column + 1.0) / columns) - (0.5 / width);
    let top = (row / rows) + (0.5 / height);
    let bottom = ((row + 1.0) / rows) - (0.5 / height);
    let uvs = [[left, top], [left, bottom], [right, bottom], [left, top], [right, bottom], [right, top]];

    let origin = [particle.x, particle.y, particle.z];
    for (vertex, uv) in local.as_chunks::<COLORED_STRIDE>().0.iter().zip(uvs) {
        for ((origin, right), up) in origin.iter().zip(camera.right).zip(camera.up) {
            out.push((origin + (vertex[0] * right)) + (vertex[1] * up));
        }
        out.extend_from_slice(&vertex[3..7]);
        out.extend_from_slice(&uv);
    }
}

/// An irregular solid triangle fan.
#[allow(clippy::too_many_arguments)]
fn add_radial_polygon(
    vertices: &mut Vec<f64>,
    x: f64,
    y: f64,
    radius: f64,
    color: [f64; 4],
    shape: &[f64],
    rotation: f64,
    z: f64,
) {
    add_gradient_radial_polygon(vertices, x, y, radius, color, color, shape, rotation, z);
}

/// An irregular triangle fan with independent center and edge colors.
#[allow(clippy::too_many_arguments)]
fn add_gradient_radial_polygon(
    vertices: &mut Vec<f64>,
    x: f64,
    y: f64,
    radius: f64,
    center: [f64; 4],
    edge: [f64; 4],
    shape: &[f64],
    rotation: f64,
    z: f64,
) {
    let count = shape.len() as f64;
    for index in 0..shape.len() {
        let first_angle = rotation + (((index as f64 / count) * PI) * 2.0);
        let second_angle = rotation + ((((index + 1) as f64 / count) * PI) * 2.0);
        let first = shape[index];
        let second = shape[(index + 1) % shape.len()];
        vertices.extend_from_slice(&[x, y, z]);
        vertices.extend_from_slice(&center);
        vertices.push(x + ((deterministic::cos(first_angle) * radius) * first));
        vertices.push(y + ((deterministic::sin(first_angle) * radius) * first));
        vertices.push(z);
        vertices.extend_from_slice(&edge);
        vertices.push(x + ((deterministic::cos(second_angle) * radius) * second));
        vertices.push(y + ((deterministic::sin(second_angle) * radius) * second));
        vertices.push(z);
        vertices.extend_from_slice(&edge);
    }
}

/// An annulus of two triangles per segment; `width` is the half thickness.
#[allow(clippy::too_many_arguments)]
fn add_ring(
    vertices: &mut Vec<f64>,
    x: f64,
    y: f64,
    radius: f64,
    width: f64,
    color: [f64; 4],
    segments: usize,
    z: f64,
) {
    let count = segments as f64;
    for index in 0..segments {
        let first_angle = ((index as f64 / count) * PI) * 2.0;
        let second_angle = (((index + 1) as f64 / count) * PI) * 2.0;
        let inner = f64::max(0.0, radius - width);
        let outer = radius + width;
        let corners = [(inner, first_angle), (outer, first_angle), (outer, second_angle), (inner, second_angle)];
        for corner in [0, 1, 2, 0, 2, 3] {
            let (corner_radius, angle) = corners[corner];
            vertices.push(x + (deterministic::cos(angle) * corner_radius));
            vertices.push(y + (deterministic::sin(angle) * corner_radius));
            vertices.push(z);
            vertices.extend_from_slice(&color);
        }
    }
}
