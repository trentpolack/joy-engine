# Postprocessing
Joy Engine owns HDR scene attachments, multiscale bloom, and display processing. `PostProcessingConfig` supplies complete renderer settings; versioned profiles resolve engine defaults, a named preset, and authored overrides. Projects keep profiles in their level assets so the appearance travels with content.

The display chain applies bloom, full and vignette blur, exposure and AgX/ACES/linear tone mapping, hue/saturation, brightness/contrast, vibrance, vignette, antialiasing, and film grain. Bloom supports two to five pyramid levels, bicubic reconstruction, anamorphic radii, and horizontal streaks. Intermediate HDR uses `Rgba16Float` where rendering, filtering, and blending permit it, with a capability-dependent fallback.

Colors enter the scene in linear space. Radius and viewport conventions belong to each control's documented contract; frame time is seconds. Validate a complete update before replacing accepted settings. Resources and temporal history belong to the renderer, and resizing rebuilds dependent targets.

## Profiles and Development Controls
The engine's `core` profile/configuration types validate authored fields and presets without a renderer. `development::PostProcessingEditor` and the development dock expose draft editing, comparison, reset, and history. Persistence uses the engine's platform adapters; static browser builds cannot write source files. See [live tuning](tuning.md) and public rustdoc for save behavior and units.

Scene effects operate before display processing and have their own depth, motion, and capability requirements. See [scene effects](scene-effects.md). Frame cadence and CPU submission diagnostics are not GPU execution measurements.
