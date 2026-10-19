# Scene Effects
`Renderer::set_scene_effects` configures scene processing before bloom and display transforms. `FrameInputs` supplies the scene's frame data; `development::SceneEffectsControls` exposes temporary preview controls. Effects remain opt-in and do not rewrite authored assets automatically.

Available passes include SSAO or GTAO, screen-space global illumination, reflections, outlines, height fog, clustered volumetric lighting, depth-aware blur, temporal antialiasing, motion blur, and adaptive exposure. The [scene-effects example](../examples/scene-effects/README.md) isolates those controls.

## Frame Contracts
Surface and motion attachments use the same geometry, camera, and opaque depth as the scene. Temporal effects need genuine previous object/camera state, with signed UV motion and documented jitter conventions. Translucent feedback follows the opaque effects stage. Stable identities are necessary for correspondence; reset history after discontinuous scene or camera changes.

Depth, normal, velocity, and color formats must meet the effect's capability requirements. Unsupported backends report availability rather than inventing missing inputs. The browser's WebGL fallback still provides the base scene and display path; validate optional effects on the backend that will ship them.

Screen-space techniques cannot see offscreen or occluded sources. Centroid transparency sorting does not resolve intersecting surfaces. Fog and volumetric approximations are rendering effects rather than gameplay simulation. Keep these limits visible when judging captures.

## Verification
Run engine integration tests, the example's native/browser builds, and captures on the affected backends. Headless rendering tests in `tests/integration/` protect configuration, ownership, motion, and shader contracts. Inspect temporal behavior interactively as well as still captures; CPU timing and frame cadence do not establish GPU execution time.
