# FORM Destruction
A Joy Engine example for seeded chunk structures and cumulative damage. Click a surface to project an impact from the camera onto the nearest active chunk. Repeat an impact to break weakened chunks, drag to orbit, scroll to zoom, and press R or **Reset** to restore the structure and clear debris. **INTEGRITY** shows damage resistance; **SUPPORT** shows the ground connection. Clicking an active view button returns to material colors. CONFIG adjusts impact radius and damage, and toggles impact debug drawing. There is no player or movement simulation.

## Run
From the JoyGames root:
```bash
npm run dev:example:form-destruction
npm run build:example:form-destruction
npm run test --workspace @joy-engine/example-form-destruction
npm run capture:example:form-destruction
npm run capture:example:form-destruction -- --built
```
From the engine repository, run `npm run dev --workspace @joy-engine/example-form-destruction`. The hosting platform also serves it at `/examples/form-destruction/` in development and production.

WebGPU is preferred, with WebGL 2 fallback. `?backend=webgl` and `?backend=webgpu` select a backend explicitly. The renderer's public `screenRay()` helper converts client CSS coordinates using the current camera and backend clip convention, so clicks remain aligned at different display densities. Empty-space clicks do nothing, and orbit drags, canceled gestures, and secondary clicks do not apply damage.

## Implemented boundary
`generateStructure()` creates a stable wall and shallow building shell from closed chunks. Coordinate-derived IDs remain stable; the seed changes material variation. Structure dimensions, damage, physics, integrity colors, and camera controls are named configuration in `src/config/destruction-config.ts`.

The default integrity view visualizes actual committed resistance from `ChunkStructure.damageState()`: cyan solid masonry starts at six hit points, stronger blue frame chunks at ten, and damage progresses through yellow to red before release. The last impacted chunk has a resistance meter and remaining-hit estimate using the current damage setting. A default impact deals one damage unit, so intact masonry takes six hits and frames ten; already damaged chunks take fewer. The view uses neutral lighting and slightly inset visual faces to reveal individual chunks, without changing their colliders. Turning integrity off restores material colors and lighting while preserving all damage. The integrity colors measure damage resistance. The separate SUPPORT view colors ground anchors cyan, vertically supported chunks green, and sideways-braced chunks amber.

Input queues a hit; the next simulation update commits it and refreshes the integrity colors even when nothing breaks. The same active chunk set supplies visible geometry and CPU collision/raycast queries. Removed chunks become rigid debris through Joy Engine's existing `PhysicsWorld`: box bodies tumble, collide with one another, and settle on a solid 200 × 200 meter ground slab whose top is at world Y = 0. The rendered ground and static collider share dimensions. Gravity is deliberately brisk at 24 m/s² with limited upward launch and low restitution. Every released chunk remains until reset or disposal: no expiry or oldest-piece eviction. One body per chunk ID bounds the example to its 126 generated chunks. Surviving chunks also have matching static physics colliders, so debris collides with the remaining structure as well as the ground. Static colliders are removed before their replacement dynamic bodies are created.

Support is an opt-in engine rule evaluated in the same `ChunkStructure.commit()` boundary as damage. Face contacts carry support upward from ground anchors. Horizontal displacement across either vertical or sideways contacts spends a configurable 1.05-meter cantilever allowance; climbing does not replenish that allowance, and corner contact cannot hold a chunk up. Chunks with no permitted ground path detach even with full health. They keep their full dimensions and start falling at rest. Directly broken chunks receive an impact impulse and shrink to 90% size in both mesh and collider, representing a fractured outer shell and leaving enough clearance to escape tightly fitted neighbors. The returned `detached` list is a subset of `removed`, and `supportState()` exposes copied committed support diagnostics. This is a geometric masonry approximation, not a stress/torque solver or a bonded rigid-body simulation. Detached chunks become independent bodies and do not reattach to the static support graph when they land.

Impact debug draws show a white hit cross, a yellow incoming-ray segment, and three cyan rings outlining the spherical damage radius. They are depth-tested world-space meshes, so orbiting reveals their position and direction. At most eight markers remain visible for six seconds; disabling debug drawing or resetting releases them immediately. Dimensions, colors, limits, and lifetimes are named configuration values.

`window.formDestruction.snapshot()` returns copied diagnostics: the last world-space impact, active chunks, debris count, debris body positions and velocities, ground height, impact-debug visibility/count, integrity visibility and per-chunk committed health, support paths and detached chunk IDs, static collider count, CPU render-submission time, destruction-update time, impact-query time, optional Chromium heap bytes, and the renderer backend. `sample(frames)` collects at most 600 live frames. Timings do not represent GPU execution time.

The engine's separate solver-neutral `PhysicsBackend` and removal-mutation contracts remain available for future solver adapters. This example combines CPU chunk queries with the existing engine physics wrapper without adding dependencies.

## Browser checks
The capture scenario uses actual pointer input to verify distinct left/right surface hits, cumulative damage and the six-hit breach threshold, integrity and support views, healthy-chunk collapse after a broad undercut, and the material-view toggle, impact markers and their visibility toggle, empty-space misses, orbit without damage, debris settling on the ground and persisting beyond the former expiry, reset, configuration, and resize. It also checks development startup diagnostics and production silence. `CAPTURE_BROWSER=webkit` uses Playwright WebKit; `CAPTURE_CHANNEL=chrome` or `msedge` selects an installed Chromium browser. `CAPTURE_BACKEND=fallback` disables WebGPU to exercise automatic WebGL 2 fallback, and `CAPTURE_DPR=2` exercises high-DPI picking. Native Safari requires a separate Safari run; Playwright WebKit is not Safari.

## Rust Version
`main.rs` and `src/*.rs` port the demo to the Rust engine: the same seeded structure through `physics::ChunkStructure` with the support rule, integrity and support views, rigid debris through the engine `PhysicsWorld` (Rapier instead of cannon-es), impact markers in the transparent pass, and an egui HUD laid out like the page. From the repository root:

```sh
cargo run --example form-destruction
node scripts/rust/build-web.mjs --example form-destruction --release
```

The web build writes a static page under `target/web/form-destruction/`; serve it and append `?backend=webgl` or `?backend=webgpu`. Click to strike, drag to orbit, scroll to zoom, and press R or **RESET** to restore the structure. `cargo test` ports the Node tests and drives the demo headlessly with pointer and key input through damage, a breach, the views, a miss, a reset, an undercut, and an orbit (`test/rust/example_form_destruction.rs`). The Rust demo has no `window.formDestruction` API (its snapshot is a Rust method used by the tests), renders at the display's full pixel ratio rather than capping it at 2, and R resets even with modifier keys held. Debris settles like the TypeScript build, including chunks that wedge in the breach.
