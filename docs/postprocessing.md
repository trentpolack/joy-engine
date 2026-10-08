# Postprocessor
The postprocessor owns the linear scene attachments and delegates effect execution to luma.gl's `ShaderPassRenderer`. `config.js` validates controls, `effect-pipeline.js` builds the ordered pass graph, and `postprocessor.js` owns resize and disposal. Joy Engine's thin renderer wrapper selects linear sampling for intermediate color textures so fractional blur and FXAA samples interpolate correctly.

Bloom uses upstream HDR soft-knee extraction, a quality-dependent multiscale pyramid, bicubic reconstruction, and explicit intensity/scatter controls. Radius is relative to the shorter viewport axis; the kernel radius is bounded to keep taps contiguous. Anamorphic shaping stretches the halo, while the separate optional lens control adds two-spike horizontal streaks. The upstream fragment path remains available when compute/storage support is absent. `bloom` and `bloomShaderPassPipeline` remain public alternatives alongside `createBloomShaderPassPipeline` for custom pipelines.

The chain applies bloom, optional Gaussian and vignette blur, exposure/tone mapping, saturation, brightness/contrast, vibrance, vignette, FXAA, then animated grain. Upstream effects provide all matching operations. Small standalone GLSL/WGSL modules add AgX neutral/punchy, intensity-controlled FXAA, edge-localized blur composition, and animated grain based on upstream noise. Grain size is in backing pixels and time is in seconds; analog and decorrelated hash variants are available. Neither variant claims a blue-noise spectrum.

## Scene effects and backend fallbacks
`SceneEffects` integrates upstream SSAO, TAA, and motion blur before bloom and display transforms. It borrows source/depth/normal/velocity textures and owns intermediate textures and temporal history. Create it independently, or provide `sceneEffects` alongside `postProcessing` in `GpuTriangleRenderer` and supply its `SceneEffectsFrame` as the final `render` argument. Do not destroy borrowed textures until GPU submissions using them have completed. Destroy the owner before its device.

These upstream scene pipelines currently require WebGPU. Inspect `sceneEffects.capabilities.effects` for requested/available status and reasons. WebGL 2 returns the original scene from this stage and retains the portable image effects and FXAA. Current games do not enable scene effects by default: their orthographic cameras are incompatible with upstream perspective SSAO, and temporal effects require genuine motion data.

SSAO requires sampled depth and perspective near/far planes; optional view-space normals must follow the documented upstream convention. TAA requires depth, signed floating-point UV velocity (current UV minus previous UV), and current/previous jitter in UV units. Motion blur requires the same velocity convention. Inputs must match the source dimensions/device. Missing or incompatible inputs throw before temporal history advances. Reset history on camera cuts; resize also resets it. Intermediate and TAA history color preserve HDR when supported. See the public JSDoc contracts in `scene-effects.js` for exact frame requirements.

## Live Tuning
Run `npm run dev` or any project’s `dev` command and open **JOY-ENGINE** at the bottom right. All six game projects expose the same orange, square-cornered menu, including games running inside Joy Editor. The panel is development-only. Color, intensity, and animation controls update GPU uniforms on the next frame. Changes to pass activation, tone mapper, bloom quality/shape, or blur radius replace the effect graph without restarting the scene renderer or simulation. Number boxes accept valid values beyond a slider's convenient range.

- **Pause Simulation** freezes game updates and the presentation clock while rendering continues. This is independent of the game's pause menu.
- **Compare Saved** shows the last disk profile without changing your draft. **Bypass Effects** uses a neutral resolve with tone mapping, bloom, grading, grain, vignette, and edge smoothing disabled; it still converts the linear scene to display output.
- **Undo/Redo** retain up to 100 authored edits. **Reset** removes one override so the preset's value is inherited. Choosing a starting preset clears overrides as one undoable action.
- **Save to Game** writes the project profile through the development server. **Revert** returns to the saved profile. Edits made while a save is in progress remain in the draft.
- **Export Preset** downloads a self-contained version-1 JSON profile with resolved values. **Import Profile** previews one in either game; save explicitly to make it the game's authored look. No export/import operation saves pause, bypass, comparison state, or gameplay state.

## Profiles and Presets
Each game owns its post-processing profile inside its main `.joylevel` asset (`assets/main.joylevel`, or `assets/playground.joylevel` for VEHICLE PLAYGROUND). Procedural games use a lightweight level with an empty entity array, so scene-wide authored settings share the same portable boundary without forcing their generated worlds into serialized entities. The project’s config module or scene owner resolves `postProcessing` through the public engine API.

```json
{
  "version": 1,
  "name": "Main",
  "entities": [],
  "postProcessing": {
    "version": 1,
    "preset": "default",
    "overrides": {
      "exposure": 1.18,
      "bloomStrength": 0.8
    }
  }
}
```

Resolution order is engine defaults, named preset, then game overrides. `POST_PROCESSING_SCHEMA` defines defaults, allowed ranges, units, and control metadata; `POST_PROCESSING_PRESETS` provides original `default`, `soft`, and `vivid` starting points. `validatePostProcessingProfile(unknown)` rejects unknown fields, unknown presets, unsupported versions, nonfinite values, and invalid ranges. `resolvePostProcessingProfile(unknown)` returns an independent complete settings record. Version-1 profiles using the old `blue-noise` label migrate it to `decorrelated` on load; future versions need explicit migration logic.

`GpuTriangleRenderer.setPostProcessingConfig(config)` replaces settings for the next frame. Omitted fields reset to engine defaults; pass a resolved profile when inheriting a preset. The renderer must have been created with post-processing enabled. Validation occurs before assignment, so rejected updates leave its settings and GPU resources intact.

## File Editing and Conflicts
The project Vite config installs `createPostProcessingPlugin` for exactly its own profile path. It watches disk edits and sends `joy:post-processing` events instead of allowing JSON HMR to restart the game. File notifications are coalesced for 150 ms to read the final state after atomic editor saves. Invalid external files show an error while the running renderer keeps its last valid settings. Fix the file to resume updates.

Clean sessions adopt external updates. Dirty sessions preserve local edits and offer **Use Disk** or **Keep Local**. Keep Local acknowledges the newest revision but does not save; review the draft and click Save to Game. Disk writes require same-origin JSON requests, a dedicated request header, a matching revision, and a fixed regular file under the configured path. A project server serializes editor saves, rejects symlinks and assets over 1 MiB, preserves the rest of the level, and atomically renames validated temporary files. Use one development server per project when writing profiles; separate processes do not share a write queue.

If the server disconnects, export your draft or wait for reconnection. The editor reloads the disk revision on reconnect. The production build imports the same profile statically and has neither editor DOM nor a file-write endpoint.

## Ownership and Integration
The reusable editor is a development-only public subpath, `joy-engine/dev/post-processing`. Construct `PostProcessingEditor` with `{ title, profile, renderer, hot: import.meta.hot, baseUrl: import.meta.env.BASE_URL }` inside an `import.meta.env.DEV` dynamic import. The renderer is borrowed and only needs the `setPostProcessingConfig` contract, so both `GpuTriangleRenderer` and `SceneRenderer` are supported. Supply `onFocus` to clear held gameplay input. Set `pauseShortcut: false` when the game owns P; the panel’s Pause Simulation checkbox remains available. Skip simulation updates when `editor.paused` is true, and pass `editor.renderTime(timestampMilliseconds)` to presentation rendering when freezing its clock is useful. Destroy the editor before destroying the renderer; it aborts requests, removes DOM/listeners/HMR subscriptions, and restores saved settings.

Effect graph replacements handle quality and pass-shape changes; ordinary intensity controls retain the graph. Transient gameplay blending and GPU timing remain follow-ups in [#43](https://github.com/trentpolack/JoyGames/issues/43). Existing FPS/frame-interval diagnostics are not GPU execution measurements.

## Validation
Post-processing editor behavior is covered by focused unit tests and the project capture workflows. `npm run capture:engine-config` exercises every game’s development menu, live editing, undo, save without reload, desktop/mobile layout, and Escape dismissal. It temporarily edits each post-processing profile and restores its original bytes in a `finally` block; run it in an isolated checkout.

Project captures provide responsive screenshots and real-browser smoke coverage. Unit tests cover validation, inheritance, rendering contracts, resource ownership, pending-save races, stale conflicts, origin/path restrictions, and competing writes.
