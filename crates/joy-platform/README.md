# joy-platform
The Joy Engine game shell for Rust: it owns a native window or browser canvas, the GPU device and renderer, input, the frame clock, and development UI, and runs a `Game`'s phases every frame. The same game code runs natively and in browsers.

## API
- `Game`: `start`, then every frame `input`, `simulation`, `presentation`, `render`, `ui` (egui), and `after_frame`, plus `visibility_changed`. Only `render` is required. Phase errors stop the loop and are shown to the player.
- `run(ShellOptions, game)`: blocks natively; in browsers it hands the loop to the page. `ShellOptions` sets the title, renderer options, input map, native window size, browser canvas ID, and maximum frame time (0.1 seconds by default).
- `InputMap` and `InputManager`: named actions bound to DOM key codes, mouse buttons, and virtual buttons; axes from action pairs; per-frame press edges that survive taps between frames; and every active touch point for on-screen controls.
- `DevTools`: the development overlay (frame time, backend, renderer statistics, and game-published values). Backquote toggles details.
- `HeadlessShell` (native): the same phases offscreen with a fixed step, for tests and captures.

Browsers start asynchronously, choose WebGPU or WebGL 2 (`?backend=` overrides), and translate input for egui with the engine's own translator. Hidden windows reset the clock; focus loss releases held input.
