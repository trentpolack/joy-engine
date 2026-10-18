# Environment Simulation and Rendering
`EnvironmentSystem` (`joy-environment`) is renderer-independent state for solar time, the sun and moon, skylight, wind, and authored environmental policy. A game owns it and advances it with elapsed seconds. A zero day length pauses solar time while wind continues; analytic wind never consumes gameplay randomness.

The resulting `EnvironmentFrame` supplies common state to rendering and gameplay. Directions use Y-up world coordinates, distances use world units, and colors are linear. Keep simulation clocks independent from rendering cadence and avoid separately maintained sun or wind state in passes.

Sky presentation settings belong to the renderer: `SkyOptions` groups atmosphere scattering and fog, the cloud layer, light shafts, and ambient occlusion, with `SkyOptions::validate` for authored values. `environment_lighting(&frame, &sky)` (`joy-scene`) combines a frame with those settings into the `EnvironmentLighting` record passed in the renderer's `FrameInputs`. The atmosphere draws before opaque geometry and participates in the same HDR/display pipeline. Lighting, fog, and optional scene effects consume frame data through their documented contracts; enabling policy in a document does not itself add a rendering pass.

See [simulation](rust-simulation.md), [scene effects](scene-effects.md), and the `joy-environment`, `joy-renderer`, and `joy-scene` test suites.
