# FORM Destruction
A Rust example of queued chunk damage, geometric support, and rigid debris. `ChunkStructure` commits one revision for visible geometry and collision, and the engine's `PhysicsWorld` owns debris bodies.

```bash
npm run dev --workspace @joy-engine/example-form-destruction
npm run build --workspace @joy-engine/example-form-destruction
npm run test --workspace @joy-engine/example-form-destruction
npm run capture --workspace @joy-engine/example-form-destruction -- --built
```

Native development runs the Cargo example; browser output is `dist/` and the site route is `/examples/form-destruction/`. Click a surface to strike, drag to orbit, wheel to zoom, and R or RESET to restore the wall and the ground. C, or the STRIKE and CHARGE buttons, switches what a click does: CHARGE blasts a crater into the ground under the aimed point (beneath the aimed chunk when the click hits the wall). Empty-space clicks and camera gestures do not apply damage.

## Damage and Support
Stable chunk IDs keep damage and support diagnostics coherent. Integrity colors show committed remaining resistance; SUPPORT distinguishes anchors, vertical support, and sideways bracing. Hits queue until the next update, and repeated hits accumulate before breach.

Support propagates through face contacts from ground anchors, spending the configured cantilever allowance on horizontal displacement. Unsupported healthy chunks detach in the same commit as directly broken chunks. Detached pieces become independent bodies and do not reattach after landing.

## Stress and Undermining
Every chunk carries its own mass plus everything resting on it, and passes that load down through the chunks beneath it, or sideways at a quarter of its strength when nothing is beneath it. Damage lowers what a chunk can carry, so weakening a base can overload it even before it breaks. STRESS colors load over capacity: green is comfortable, amber is strained, and red is overloaded. In every view except SUPPORT, strained chunks lean amber, and overloaded chunks pulse and shake harder through a 1.5 second failure delay, then crumble out of the wall, handing their load to the next row. Relieving an overloaded chunk (breaking what it carries) cancels its countdown.

Charges use the engine's terrain crater profile. The wall re-anchors against the cratered ground: columns beyond the cantilever reach fall at once, and the columns beside them hang from one side, overload, and fail in stages. This is a readable gameplay rule, not an elastic or fracture simulation.

Surviving chunks keep static colliders through the engine's `StructureColliders`, and the ground is a heightfield patch inside flat apron slabs, rebuilt after each charge. Released chunks become rigid debris in the same world through the engine's `DebrisPool`: direct breakage shatters a chunk into Voronoi shards (`fracture_chunk`) that launch with impact and spin shared by volume, overloaded chunks shrink and lean out of the wall, and detached pieces keep a sliver of clearance and begin at rest. Debris lands on surviving chunks, sleeps when it settles, and freezes into solid rubble once many pieces are moving; it persists until reset. Impact and charge markers are bounded, temporary, depth-tested geometry.

## Verification
`tests/integration/example_form_destruction.rs` checks the staged collapse a charge causes, and drives damage, breach, integrity/support/stress views, misses, reset, undercut collapse, a charge whose overloaded edges fail after their delay, and orbit through the real application. Browser captures add canvas startup and image evidence. Native and browser rendering share shadows, display processing, and UI. CPU timing does not measure GPU execution.
