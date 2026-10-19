# FORM Destruction
A Rust example of queued chunk damage, geometric support, and rigid debris. `ChunkStructure` commits one revision for visible geometry and collision, and the engine's `PhysicsWorld` owns debris bodies.

```bash
npm run dev --workspace @joy-engine/example-form-destruction
npm run build --workspace @joy-engine/example-form-destruction
npm run test --workspace @joy-engine/example-form-destruction
npm run capture --workspace @joy-engine/example-form-destruction -- --built
```

Native development runs the Cargo example; browser output is `dist/` and the site route is `/examples/form-destruction/`. Click a surface to strike, drag to orbit, wheel to zoom, and R or RESET to restore. Empty-space clicks and camera gestures do not apply damage.

## Damage and Support
Stable chunk IDs keep damage and support diagnostics coherent. Integrity colors show committed remaining resistance; SUPPORT distinguishes anchors, vertical support, and sideways bracing. Hits queue until the next update, and repeated hits accumulate before breach.

Support propagates through face contacts from ground anchors, spending the configured cantilever allowance on horizontal displacement. Unsupported healthy chunks detach in the same commit as directly broken chunks. This is a geometric masonry approximation, not stress/torque simulation. Detached pieces become independent bodies and do not reattach after landing.

Surviving chunks have static colliders. Released chunks become bounded-by-structure rigid debris; direct breakage shrinks pieces and adds impact, while detached pieces begin at rest. Debris persists until reset. Impact markers are bounded, temporary, depth-tested geometry.

## Verification
`tests/integration/example_form_destruction.rs` drives damage, breach, integrity/support views, misses, reset, undercut collapse, and orbit through the real application. Browser captures add canvas startup and image evidence. Native and browser rendering share shadows, display processing, and UI. CPU timing does not measure GPU execution.
