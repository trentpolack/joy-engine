// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The frame plan: every pass a frame records, in order, with the targets it writes and reads, and how each pass loads
//! and stores each attachment.
//!
//! Passes declare what they do with each target; the plan derives the operations from the order instead of from
//! combinations of frame flags:
//! - A pass clears an attachment when no earlier pass this frame wrote it, and loads it otherwise. Targets that keep
//!   content across frames (cached shadow maps) say so and load whenever something they hold survives.
//! - A pass stores an attachment when a later pass reads or writes it, or when frames after this one read it (shadow
//!   maps, and the scene color and motion the effects and post chain consume); otherwise its contents are discarded.
//!
//! [`FrameTargets`] resolves each target to the view the frame binds, so pass encoders build their attachments from the
//! plan alone. Passes owned by other modules (volumetric fog, composite effects, and post-processing) appear with what
//! they read and write so the plan describes the whole frame, and record their own attachments.

use joy_core::LinearColor;

/// One target a frame's passes write or read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameTarget {
    /// The HDR scene color at the render size.
    SceneColor,
    /// The scene depth (with the stencil of deferred lighting slots).
    SceneDepth,
    /// The stencil aspect of the scene depth target.
    SceneStencil,
    /// The scene-effect motion attachment.
    Motion,
    /// The deferred G-buffer.
    GBuffer,
    /// One directional shadow cascade layer.
    Cascade(usize),
    /// The point-light shadow atlas.
    ShadowAtlas,
    /// Volumetric fog's integrated volume.
    FogVolume,
    /// The caller's presentation view.
    Output,
}

impl FrameTarget {
    /// Whether frames after this one read the target, so every write stores it.
    fn read_by_later_frames(self) -> bool {
        // Shadow maps are cached between frames; the composite effects keep their own history from the scene color and
        // motion, which the frame's last passes read.
        matches!(self, Self::Cascade(_) | Self::ShadowAtlas | Self::SceneColor | Self::Motion | Self::Output)
    }
}

/// One pass of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FramePass {
    /// One directional cascade that renders this frame.
    Cascade(usize),
    /// Every point-light face that renders this frame, each through its tile's viewport.
    PointAtlas,
    /// Volumetric fog injection and integration (compute).
    Volumetrics,
    /// Depth (and a cleared stencil) for every opaque surface.
    DepthPrepass,
    /// Deferred surfaces into the scene color, motion, G-buffer, and stencil.
    GBuffer,
    /// One full-screen pass per deferred shading model, selected by the stencil.
    DeferredLighting,
    /// Opaque surfaces shaded forward (all of them, or after deferred lighting the ones it cannot hold), then the sky,
    /// and transparency when it joins this pass.
    Scene,
    /// Fog blended over the opaque scene.
    Fog,
    /// Transparency in its own pass over the lit scene.
    Transparency,
    /// Temporal antialiasing, motion blur, and adaptive exposure.
    Composite,
    /// Post-processing into the presentation view.
    Post,
}

/// How a pass begins with an attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Load {
    /// Start from the target's clear value.
    Clear,
    /// Keep what the target holds.
    Keep,
}

/// One attachment of a planned pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Attachment {
    pub target: FrameTarget,
    pub load: Load,
    /// Whether the pass stores what it wrote (otherwise the contents are discarded when it ends).
    pub store: bool,
}

/// What one pass does with one target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    /// Written as a color attachment.
    Color,
    /// Written (or tested and written) as the depth or stencil attachment.
    Depth,
    /// Tested as the depth attachment without writing; recorded as a loaded, discarded depth attachment unless a
    /// later pass needs it.
    DepthTest,
    /// Bound read-only as the depth-stencil attachment (no load or store operations).
    DepthReadOnly,
    /// Sampled, or written by a pass outside the renderer's attachments.
    Read,
    Write,
}

/// One pass as declared: what it does to each target, before operations are derived.
struct DeclaredPass {
    pass: FramePass,
    label: &'static str,
    uses: Vec<(FrameTarget, Use)>,
}

impl DeclaredPass {
    fn new(pass: FramePass, label: &'static str, uses: Vec<(FrameTarget, Use)>) -> Self {
        Self {
            pass,
            label,
            uses,
        }
    }

    /// Whether the pass touches `target`. Every pass after a target's first writer loads it, so any later use needs
    /// what an earlier pass stores.
    fn touches(&self, target: FrameTarget) -> bool {
        self.uses.iter().any(|(used, _)| *used == target)
    }
}

/// One pass of the plan with its derived attachments.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlannedPass {
    pub pass: FramePass,
    pub label: &'static str,
    /// Color attachments in binding order.
    pub colors: Vec<Attachment>,
    /// The depth attachment, when the pass binds one with operations.
    pub depth: Option<Attachment>,
    /// The stencil aspect's operations, when the pass writes or clears it.
    pub stencil: Option<Attachment>,
    /// Whether the depth-stencil target is bound read-only (tested, never written).
    pub depth_read_only: bool,
    /// Targets the pass samples or writes outside its attachments.
    pub reads: Vec<FrameTarget>,
}

/// The shape of one frame: everything that decides which passes run and what they touch.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FrameShape {
    /// Directional cascades that render this frame (cached ones are skipped).
    pub cascades: Vec<usize>,
    /// Whether any point-light face renders, and whether the atlas keeps cached faces (loads instead of clearing).
    pub point_atlas: Option<Load>,
    /// Whether volumetric fog runs (its volume pass, then its blend over the opaque scene).
    pub fog: bool,
    pub depth_prepass: bool,
    pub deferred: bool,
    /// Whether opaque passes write the motion attachment.
    pub motion: bool,
    /// Whether transparency draws at the end of the scene pass (otherwise in its own pass after fog).
    pub transparency_in_scene: bool,
    /// Whether transparency samples the opaque depth (soft particles), so its own pass binds depth read-only.
    pub soft_particles: bool,
    /// Whether the composite effects run.
    pub composite: bool,
}

/// The frame's passes in recording order.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FramePlan {
    passes: Vec<PlannedPass>,
}

impl FramePlan {
    /// Lay out the passes of a frame of `shape` and derive every attachment's operations.
    pub(crate) fn new(shape: &FrameShape) -> Self {
        let declared = declare_passes(shape);
        let mut passes = Vec::with_capacity(declared.len());
        let mut written: Vec<FrameTarget> = Vec::new();
        for (index, declaration) in declared.iter().enumerate() {
            let later = &declared[index + 1..];
            let mut planned = PlannedPass {
                pass: declaration.pass,
                label: declaration.label,
                colors: Vec::new(),
                depth: None,
                stencil: None,
                depth_read_only: false,
                reads: Vec::new(),
            };
            for (target, usage) in &declaration.uses {
                let attachment = |load_cached: bool| Attachment {
                    target: *target,
                    load: if load_cached || written.contains(target) { Load::Keep } else { Load::Clear },
                    store: target.read_by_later_frames() || later.iter().any(|pass| pass.touches(*target)),
                };
                match usage {
                    Use::Color => planned.colors.push(attachment(false)),
                    Use::Depth | Use::DepthTest => {
                        let cached = *target == FrameTarget::ShadowAtlas && shape.point_atlas == Some(Load::Keep);
                        let mut depth = attachment(cached);
                        if *usage == Use::DepthTest {
                            depth.load = Load::Keep;
                        }
                        if *target == FrameTarget::SceneStencil {
                            planned.stencil = Some(depth);
                        } else {
                            planned.depth = Some(depth);
                        }
                    }
                    Use::DepthReadOnly => planned.depth_read_only = true,
                    Use::Read | Use::Write => planned.reads.push(*target),
                }
                if matches!(usage, Use::Color | Use::Depth | Use::Write) && !written.contains(target) {
                    written.push(*target);
                }
            }
            passes.push(planned);
        }
        Self {
            passes,
        }
    }

    /// The planned pass of kind `pass`, if the frame runs it.
    pub(crate) fn pass(&self, pass: FramePass) -> Option<&PlannedPass> {
        self.passes.iter().find(|planned| planned.pass == pass)
    }

    /// Every pass in recording order.
    #[cfg(test)]
    pub(crate) fn passes(&self) -> &[PlannedPass] {
        &self.passes
    }
}

/// Each pass of a frame of `shape`, in order, with what it does to each target.
fn declare_passes(shape: &FrameShape) -> Vec<DeclaredPass> {
    use FrameTarget::*;
    let mut passes = Vec::new();

    // Shadow maps first, so every lit pass samples this frame's depth.
    for cascade in &shape.cascades {
        passes.push(DeclaredPass::new(
            FramePass::Cascade(*cascade),
            "shadow cascade",
            vec![(Cascade(*cascade), Use::Depth)],
        ));
    }
    if shape.point_atlas.is_some() {
        passes.push(DeclaredPass::new(FramePass::PointAtlas, "point shadow atlas", vec![(ShadowAtlas, Use::Depth)]));
    }
    let shadow_reads = || {
        let mut reads: Vec<(FrameTarget, Use)> =
            shape.cascades.iter().map(|cascade| (Cascade(*cascade), Use::Read)).collect();
        if shape.point_atlas.is_some() {
            reads.push((ShadowAtlas, Use::Read));
        }
        reads
    };
    if shape.fog {
        let mut uses = shadow_reads();
        uses.push((FogVolume, Use::Write));
        passes.push(DeclaredPass::new(FramePass::Volumetrics, "volumetric fog", uses));
    }

    // Opaque surfaces: depth first, then deferred surfaces and their lighting, then forward shading.
    if shape.depth_prepass {
        passes.push(DeclaredPass::new(
            FramePass::DepthPrepass,
            "depth pre-pass",
            vec![(SceneDepth, Use::Depth), (SceneStencil, Use::Depth)],
        ));
    }
    let opaque_colors = |uses: &mut Vec<(FrameTarget, Use)>| {
        uses.push((SceneColor, Use::Color));
        if shape.motion {
            uses.push((Motion, Use::Color));
        }
    };
    if shape.deferred {
        let mut uses = Vec::new();
        opaque_colors(&mut uses);
        uses.extend([(GBuffer, Use::Color), (SceneDepth, Use::Depth), (SceneStencil, Use::Depth)]);
        uses.extend(shadow_reads());
        passes.push(DeclaredPass::new(FramePass::GBuffer, "g-buffer", uses));
        let mut uses = vec![(SceneColor, Use::Color), (SceneDepth, Use::DepthReadOnly), (GBuffer, Use::Read)];
        uses.extend([(SceneDepth, Use::Read), (SceneStencil, Use::Read)]);
        uses.extend(shadow_reads());
        passes.push(DeclaredPass::new(FramePass::DeferredLighting, "deferred lighting", uses));
    }
    let mut uses = Vec::new();
    opaque_colors(&mut uses);
    uses.push((SceneDepth, Use::Depth));
    uses.extend(shadow_reads());
    passes.push(DeclaredPass::new(FramePass::Scene, if shape.deferred { "scene forward" } else { "scene" }, uses));

    // Fog over the opaque scene, transparency (unless the scene pass drew it), then the composite effects and post.
    if shape.fog {
        passes.push(DeclaredPass::new(
            FramePass::Fog,
            "volumetric fog apply",
            vec![(SceneColor, Use::Write), (SceneDepth, Use::Read), (FogVolume, Use::Read)],
        ));
    }
    if !shape.transparency_in_scene {
        // Soft particles sample the depth they test against, which a pass may do only while depth is read-only.
        let depth = if shape.soft_particles { Use::DepthReadOnly } else { Use::DepthTest };
        let mut uses = vec![(SceneColor, Use::Color), (SceneDepth, depth)];
        uses.extend(shadow_reads());
        passes.push(DeclaredPass::new(FramePass::Transparency, "scene transparency", uses));
    }
    if shape.composite {
        let mut uses = vec![(SceneColor, Use::Write)];
        if shape.motion {
            uses.push((Motion, Use::Read));
        }
        passes.push(DeclaredPass::new(FramePass::Composite, "scene effects composite", uses));
    }
    passes.push(DeclaredPass::new(
        FramePass::Post,
        "post-process",
        vec![(SceneColor, Use::Read), (Output, Use::Write)],
    ));
    passes
}

/// The views a frame's planned attachments resolve to, and their clear values.
pub(crate) struct FrameTargets<'a> {
    pub scene_color: &'a wgpu::TextureView,
    pub scene_depth: &'a wgpu::TextureView,
    pub motion: Option<&'a wgpu::TextureView>,
    pub gbuffer: Option<&'a wgpu::TextureView>,
    pub cascades: &'a [wgpu::TextureView],
    pub atlas: &'a wgpu::TextureView,
    /// The background the scene color clears to.
    pub clear_color: LinearColor,
}

impl FrameTargets<'_> {
    /// The view an attachment of `target` binds.
    fn view(&self, target: FrameTarget) -> &wgpu::TextureView {
        match target {
            FrameTarget::SceneColor => self.scene_color,
            FrameTarget::SceneDepth | FrameTarget::SceneStencil => self.scene_depth,
            FrameTarget::Motion => self.motion.expect("motion passes run only while the attachment exists"),
            FrameTarget::GBuffer => self.gbuffer.expect("g-buffer passes run only while the g-buffer exists"),
            FrameTarget::Cascade(index) => &self.cascades[index],
            FrameTarget::ShadowAtlas => self.atlas,
            FrameTarget::FogVolume | FrameTarget::Output => {
                unreachable!("fog and the output are written by their own passes, never as renderer attachments")
            }
        }
    }

    /// Begin a planned render pass.
    pub(crate) fn begin<'encoder>(
        &self,
        encoder: &'encoder mut wgpu::CommandEncoder,
        planned: &PlannedPass,
    ) -> wgpu::RenderPass<'encoder> {
        let colors: Vec<Option<wgpu::RenderPassColorAttachment<'_>>> = planned
            .colors
            .iter()
            .map(|attachment| {
                let clear = match attachment.target {
                    FrameTarget::SceneColor => color(self.clear_color),
                    _ => wgpu::Color::TRANSPARENT,
                };
                Some(wgpu::RenderPassColorAttachment {
                    view: self.view(attachment.target),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: match attachment.load {
                            Load::Clear => wgpu::LoadOp::Clear(clear),
                            Load::Keep => wgpu::LoadOp::Load,
                        },
                        store: store_op(attachment.store),
                    },
                    depth_slice: None,
                })
            })
            .collect();
        let depth_target = planned.depth.or(planned.stencil).map(|attachment| attachment.target);
        let depth_stencil = if planned.depth_read_only {
            Some(wgpu::RenderPassDepthStencilAttachment {
                view: self.scene_depth,
                depth_ops: None,
                stencil_ops: None,
            })
        } else {
            depth_target.map(|target| wgpu::RenderPassDepthStencilAttachment {
                view: self.view(target),
                depth_ops: planned.depth.map(|attachment| wgpu::Operations {
                    load: match attachment.load {
                        Load::Clear => wgpu::LoadOp::Clear(1.0),
                        Load::Keep => wgpu::LoadOp::Load,
                    },
                    store: store_op(attachment.store),
                }),
                stencil_ops: planned.stencil.map(|attachment| wgpu::Operations {
                    load: match attachment.load {
                        Load::Clear => wgpu::LoadOp::Clear(0),
                        Load::Keep => wgpu::LoadOp::Load,
                    },
                    store: store_op(attachment.store),
                }),
            })
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(planned.label),
            color_attachments: &colors,
            depth_stencil_attachment: depth_stencil,
            ..Default::default()
        })
    }
}

fn store_op(store: bool) -> wgpu::StoreOp {
    if store { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard }
}

fn color(value: LinearColor) -> wgpu::Color {
    wgpu::Color {
        r: f64::from(value.red),
        g: f64::from(value.green),
        b: f64::from(value.blue),
        a: f64::from(value.alpha),
    }
}

#[cfg(test)]
#[path = "frame_plan.test.rs"]
mod tests;
