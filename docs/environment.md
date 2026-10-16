# Environment Simulation and Rendering
`EnvironmentSystem` is renderer-independent state for solar time, sun and moon directions, skylight, wind, atmosphere, and authored environmental policy. A game owns it and advances it with elapsed seconds. A zero day length pauses solar time while wind continues; analytic wind never consumes gameplay randomness.

The resulting `EnvironmentFrame` supplies common state to rendering and gameplay. Directions use Y-up world coordinates, distances use world units, and colors are linear. Keep simulation clocks independent from rendering cadence and avoid separately maintained sun or wind state in passes.

Pass environment data into the renderer's `FrameInputs`. The atmosphere draws before opaque geometry and participates in the same HDR/display pipeline. Lighting, fog, and optional scene effects consume frame data through their documented contracts; enabling policy in a document does not itself add a rendering pass.

See [simulation](rust-simulation.md), [scene effects](scene-effects.md), and the native/WebAssembly tests in `test/rust/`.
