// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The per-particle GPU instance record and how one simulated particle becomes one.
//!
//! Every particle draws as one instance of a shared six-vertex quad: the vertex shader expands it to face the camera
//! (or to follow a streak's velocity), and the fragment shader computes the shape analytically
//! (`shaders/particle-shapes.wgsl`). Building an instance only reads the simulation; it never advances it or draws
//! randomness. Positions are world units, colors linear HDR, and rotations radians.

use bytemuck::{
    Pod,
    Zeroable,
};
use joy_core::math::{
    Mat4,
    Vec3,
};
use joy_fx::{
    EffectParticle,
    ParticleBlend,
    ParticleRenderer,
    ParticleTexture,
    ParticleTextureDimensions,
};

/// Seconds of velocity a streak ribbon trails behind its particle, so its world length is independent of frame rate.
pub(crate) const STREAK_SECONDS: f64 = 0.035;

/// A streak's head glow radius, in multiples of its size (`STREAK_GLOW` in particle-shapes.wgsl).
pub(crate) const STREAK_GLOW: f32 = 1.7;

/// The billow outline's largest radius, in multiples of its size: `1 + 0.09 + 0.05` for the lobed outline
/// (`BILLOW_EXTENT` in particle-shapes.wgsl).
pub(crate) const BILLOW_EXTENT: f32 = 1.14;

/// A ring's smallest half thickness in world units; larger rings use [`RING_WIDTH_FRACTION`] of their radius.
pub(crate) const RING_MINIMUM_HALF_WIDTH: f32 = 0.15;

/// A ring's half thickness as a fraction of its radius, once that exceeds [`RING_MINIMUM_HALF_WIDTH`].
pub(crate) const RING_WIDTH_FRACTION: f32 = 0.045;

/// Flag bit (above the shape byte) marking an instance whose color is premultiplied (`PARTICLE_PREMULTIPLIED`).
const PREMULTIPLIED_FLAG: u32 = 1 << 8;

/// Largest value of a normalized 16-bit texture coordinate.
const UNORM16_MAXIMUM: f64 = 65535.0;

/// Shape ids the particle shaders branch on (`PARTICLE_SHAPE_*` in particle-shapes.wgsl).
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParticleShape {
    /// Center color fading linearly to zero alpha at the edge.
    Soft = 0,
    /// A velocity ribbon with a soft glow at its head.
    Streak = 1,
    /// An antialiased annulus.
    Ring = 2,
    /// Two overlapping feathered lobes.
    Billow = 3,
    /// A solid antialiased disc.
    Circle = 4,
    /// A rotated solid quad.
    Square = 5,
    /// A rotated quad sampling one atlas cell of a texture.
    Sprite = 6,
}

impl From<ParticleRenderer> for ParticleShape {
    fn from(renderer: ParticleRenderer) -> Self {
        match renderer {
            ParticleRenderer::Soft => Self::Soft,
            ParticleRenderer::Streak => Self::Streak,
            ParticleRenderer::Ring => Self::Ring,
            ParticleRenderer::Billow => Self::Billow,
            ParticleRenderer::Circle => Self::Circle,
            ParticleRenderer::Square => Self::Square,
            ParticleRenderer::Textured | ParticleRenderer::Flipbook => Self::Sprite,
        }
    }
}

/// One particle as the GPU draws it: 60 bytes, five vertex attributes (`ParticleInstance` in particle-shapes.wgsl).
///
/// The layout is a renderer contract, not a stable format: it changes whenever particle drawing does.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ParticleInstance {
    /// World position (a streak's head) and size in world units: the radius, or a square's half extent.
    pub position_size: [f32; 4],
    /// Linear HDR RGB (nonnegative) and alpha (at most one); premultiplied for premultiplied emitters.
    pub color: [f32; 4],
    /// A streak's world offset from head to tail (zero for other shapes), then the camera-facing roll in radians.
    pub trail_rotation: [f32; 4],
    /// A sprite's atlas cell as normalized 16-bit left, top, right, and bottom texture coordinates (top-left origin),
    /// inset by half a texel; zero for untextured shapes.
    pub uv_rect: [u16; 4],
    /// Shape id in the low byte; bit 8 marks premultiplied color.
    pub shape: u32,
}

impl ParticleInstance {
    /// World position (a streak's head).
    pub fn position(&self) -> Vec3 {
        Vec3::new(self.position_size[0], self.position_size[1], self.position_size[2])
    }

    /// Radius of a sphere around the position that contains everything the instance draws, in world units.
    pub(crate) fn extent(&self) -> f32 {
        let size = self.position_size[3];
        let trail = Vec3::new(self.trail_rotation[0], self.trail_rotation[1], self.trail_rotation[2]).length();
        match self.shape & 0xff {
            shape if shape == ParticleShape::Streak as u32 => (size * STREAK_GLOW).max(trail + (size * 0.5)),
            shape if shape == ParticleShape::Ring as u32 => size + ring_half_width(size),
            shape if shape == ParticleShape::Billow as u32 => size * BILLOW_EXTENT,
            // A rotated square reaches its corners.
            shape if shape >= ParticleShape::Square as u32 => size * std::f32::consts::SQRT_2,
            _ => size,
        }
    }
}

/// A ring's half thickness for radius `size`, in world units.
pub(crate) fn ring_half_width(size: f32) -> f32 {
    RING_MINIMUM_HALF_WIDTH.max(size * RING_WIDTH_FRACTION)
}

// ---------------------------------------------------------------------------------------------------------------------
// Building instances
// ---------------------------------------------------------------------------------------------------------------------

/// What every particle of one emitter shares when it becomes an instance.
#[derive(Clone, Debug)]
pub(crate) struct EmitterDrawing {
    /// Shape id and flags.
    pub shape: u32,
    /// Whether the emitter's color is premultiplied (alpha zero still glows).
    pub premultiplied: bool,
    /// The sprite atlas and whether it animates (flipbook), for textured emitters.
    pub sprite: Option<SpriteAtlas>,
}

/// A textured emitter's atlas grid and animation.
#[derive(Clone, Debug)]
pub(crate) struct SpriteAtlas {
    columns: f64,
    rows: f64,
    frames: f64,
    fps: f64,
    looping: bool,
    animated: bool,
    width: f64,
    height: f64,
}

impl EmitterDrawing {
    /// The drawing settings of an emitter with `renderer` and `blend`; `texture` and `dimensions` are required for
    /// textured renderers (callers check first).
    pub fn new(
        renderer: ParticleRenderer,
        blend: ParticleBlend,
        texture: Option<(&ParticleTexture, ParticleTextureDimensions)>,
    ) -> Self {
        let premultiplied = blend == ParticleBlend::Premultiplied;
        let flags = if premultiplied { PREMULTIPLIED_FLAG } else { 0 };
        let sprite = texture.filter(|_| renderer.is_textured()).map(|(texture, dimensions)| SpriteAtlas {
            columns: f64::from(texture.columns),
            rows: f64::from(texture.rows),
            frames: f64::from(texture.frames),
            fps: texture.fps,
            looping: texture.looping,
            animated: renderer == ParticleRenderer::Flipbook,
            width: f64::from(dimensions.width),
            height: f64::from(dimensions.height),
        });
        Self {
            shape: (ParticleShape::from(renderer) as u32) | flags,
            premultiplied,
            sprite,
        }
    }

    /// Whether `particle` draws anything: positive size within its lifetime, and some alpha (or, for premultiplied
    /// color, some light).
    pub fn is_visible(&self, particle: &EffectParticle) -> bool {
        let lit = self.premultiplied && (particle.r > 0.0 || particle.g > 0.0 || particle.b > 0.0);
        particle.size > 0.0 && particle.age < particle.lifetime && (particle.alpha > 0.0 || lit)
    }

    /// The instance for a visible `particle`, placed by `transform` (effect space to world) when given.
    ///
    /// `size_scale` multiplies sizes under the transform (billboards stay camera-facing, so non-uniform scale moves
    /// particles but never stretches them).
    pub fn instance(&self, particle: &EffectParticle, transform: Option<&Mat4>, size_scale: f32) -> ParticleInstance {
        let local = Vec3::new(particle.x as f32, particle.y as f32, particle.z as f32);
        let trail_local = Vec3::new(
            -(particle.vx * STREAK_SECONDS) as f32,
            -(particle.vy * STREAK_SECONDS) as f32,
            -(particle.vz * STREAK_SECONDS) as f32,
        );
        let (position, trail) = match transform {
            Some(transform) => (transform.transform_point3(local), transform.transform_vector3(trail_local)),
            None => (local, trail_local),
        };
        let is_streak = (self.shape & 0xff) == ParticleShape::Streak as u32;
        let trail = if is_streak { trail } else { Vec3::ZERO };
        let alpha = particle.alpha.clamp(0.0, 1.0);
        ParticleInstance {
            position_size: [position.x, position.y, position.z, (particle.size as f32) * size_scale],
            color: [particle.r.max(0.0) as f32, particle.g.max(0.0) as f32, particle.b.max(0.0) as f32, alpha as f32],
            trail_rotation: [trail.x, trail.y, trail.z, particle.rotation as f32],
            uv_rect: self.sprite.as_ref().map_or([0; 4], |atlas| atlas.cell(particle)),
            shape: self.shape,
        }
    }
}

impl SpriteAtlas {
    /// The atlas cell `particle` shows, as normalized 16-bit coordinates inset by half a texel so linear filtering
    /// cannot sample neighboring frames. Flipbooks advance by particle age in seconds at `fps`, or across the lifetime
    /// at zero fps; textured sprites show the first cell. Frames read left to right, top to bottom.
    fn cell(&self, particle: &EffectParticle) -> [u16; 4] {
        let mut frame = 0.0;
        if self.animated {
            let elapsed_frames =
                if self.fps > 0.0 { particle.age * self.fps } else { (particle.age / particle.lifetime) * self.frames };
            let index = f64::max(0.0, elapsed_frames.floor());
            frame = if self.looping { index % self.frames } else { f64::min(self.frames - 1.0, index) };
        }
        let column = frame % self.columns;
        let row = (frame / self.columns).floor();
        let left = (column / self.columns) + (0.5 / self.width);
        let right = ((column + 1.0) / self.columns) - (0.5 / self.width);
        let top = (row / self.rows) + (0.5 / self.height);
        let bottom = ((row + 1.0) / self.rows) - (0.5 / self.height);
        [left, top, right, bottom].map(|value| (value.clamp(0.0, 1.0) * UNORM16_MAXIMUM).round() as u16)
    }
}
