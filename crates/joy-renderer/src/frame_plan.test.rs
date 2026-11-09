// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/frame_plan.rs`.

use super::*;

fn attachment(target: FrameTarget, load: Load, store: bool) -> Attachment {
    Attachment {
        target,
        load,
        store,
    }
}

fn pass(plan: &FramePlan, pass: FramePass) -> &PlannedPass {
    plan.pass(pass).unwrap_or_else(|| panic!("{pass:?} is planned"))
}

/// The first pass that writes each target clears it, later ones load it, and a pass discards only what no later pass
/// (or frame) reads, across the frame shapes the renderer draws.
#[test]
fn attachments_clear_once_load_after_and_store_only_what_is_read() {
    use FrameTarget::*;

    // Forward with the depth pre-pass and no effects: the pre-pass clears depth, the scene pass loads and discards it.
    let forward = FramePlan::new(&FrameShape {
        cascades: vec![0, 1, 2],
        point_atlas: Some(Load::Clear),
        depth_prepass: true,
        transparency_in_scene: true,
        ..FrameShape::default()
    });
    for cascade in 0..3 {
        assert_eq!(
            pass(&forward, FramePass::Cascade(cascade)).depth,
            Some(attachment(Cascade(cascade), Load::Clear, true))
        );
    }
    assert_eq!(pass(&forward, FramePass::PointAtlas).depth, Some(attachment(ShadowAtlas, Load::Clear, true)));
    let prepass = pass(&forward, FramePass::DepthPrepass);
    assert_eq!(prepass.depth, Some(attachment(SceneDepth, Load::Clear, true)));
    let scene = pass(&forward, FramePass::Scene);
    assert_eq!(scene.colors, vec![attachment(SceneColor, Load::Clear, true)]);
    assert_eq!(scene.depth, Some(attachment(SceneDepth, Load::Keep, false)));
    assert!(forward.pass(FramePass::GBuffer).is_none() && forward.pass(FramePass::Transparency).is_none());

    // Deferred with the pre-pass and motion: the G-buffer pass clears the colors and loads depth and stencil, lighting
    // binds depth read-only, and the forward pass loads everything the G-buffer pass wrote.
    let deferred = FramePlan::new(&FrameShape {
        point_atlas: Some(Load::Keep),
        depth_prepass: true,
        deferred: true,
        motion: true,
        composite: true,
        ..FrameShape::default()
    });
    assert_eq!(pass(&deferred, FramePass::PointAtlas).depth, Some(attachment(ShadowAtlas, Load::Keep, true)));
    assert_eq!(pass(&deferred, FramePass::DepthPrepass).stencil, Some(attachment(SceneStencil, Load::Clear, true)));
    let gbuffer = pass(&deferred, FramePass::GBuffer);
    assert_eq!(
        gbuffer.colors,
        vec![
            attachment(SceneColor, Load::Clear, true),
            attachment(Motion, Load::Clear, true),
            attachment(GBuffer, Load::Clear, true)
        ]
    );
    assert_eq!(gbuffer.depth, Some(attachment(SceneDepth, Load::Keep, true)));
    assert_eq!(gbuffer.stencil, Some(attachment(SceneStencil, Load::Keep, true)));
    let lighting = pass(&deferred, FramePass::DeferredLighting);
    assert!(lighting.depth_read_only && lighting.depth.is_none() && lighting.stencil.is_none());
    let scene = pass(&deferred, FramePass::Scene);
    assert_eq!(scene.colors, vec![attachment(SceneColor, Load::Keep, true), attachment(Motion, Load::Keep, true)]);
    assert_eq!(scene.depth, Some(attachment(SceneDepth, Load::Keep, true)), "transparency tests the stored depth");
    assert_eq!(pass(&deferred, FramePass::Transparency).depth, Some(attachment(SceneDepth, Load::Keep, false)));

    // Deferred without the pre-pass, with fog: the G-buffer pass clears depth and stencil, and the scene pass stores
    // depth for fog and transparency.
    let fogged = FramePlan::new(&FrameShape {
        deferred: true,
        fog: true,
        composite: true,
        ..FrameShape::default()
    });
    let gbuffer = pass(&fogged, FramePass::GBuffer);
    assert_eq!(gbuffer.depth, Some(attachment(SceneDepth, Load::Clear, true)));
    assert_eq!(gbuffer.stencil, Some(attachment(SceneStencil, Load::Clear, true)));
    assert_eq!(pass(&fogged, FramePass::Scene).depth, Some(attachment(SceneDepth, Load::Keep, true)));
    let order: Vec<FramePass> = fogged.passes().iter().map(|planned| planned.pass).collect();
    assert_eq!(
        order,
        vec![
            FramePass::Volumetrics,
            FramePass::GBuffer,
            FramePass::DeferredLighting,
            FramePass::Scene,
            FramePass::Fog,
            FramePass::Transparency,
            FramePass::Composite,
            FramePass::Post
        ]
    );
}
