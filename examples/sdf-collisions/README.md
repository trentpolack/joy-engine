# SDF Collisions
A Rust example that compares signed distance field (SDF) colliders with bounding boxes. Two bins fill with the same FORM props, dropped in the same poses on the same schedule: the left bin collides through each prop's SDF, the right through the box the engine could offer before. Chairs hook legs, jacks interlock, and staples nest on the left; on the right the same props stack as crates.

```bash
npm run dev --workspace @joy-engine/example-sdf-collisions
npm run build --workspace @joy-engine/example-sdf-collisions
npm run test --workspace @joy-engine/example-sdf-collisions
npm run bench --workspace @joy-engine/example-sdf-collisions
npm run capture --workspace @joy-engine/example-sdf-collisions -- --built
```

Drag to orbit, scroll to zoom, Space or DROP MORE to add ten props per bin, P to pause, and R to reset. The sidebar shows each bin's CPU physics time per fixed step, the SDF contact work of the latest step, and each collider's build cost and memory.

## Props and Colliders
The chair, jack, staple, and I-beam are FORM scripts in `assets/` built from axis-aligned boxes. Each evaluates once, draws through the engine's PBR path, and bakes into a 40-cell voxel distance grid with `SdfShape::from_triangle_mesh`; every body shares its prop's grid. The ring uses the analytical `SdfPrimitive::Torus`, and its FORM mesh only draws it. The box bin uses each collider's local bounds, and both bins give a prop the same mass (its SDF volume times a pine-like density).

Box-only props keep the baked colliders identical on every target: FORM's spheres and rotated instances use platform trigonometry, which can differ in the last bit between native and WebAssembly builds.

## Benchmark
`--bench` (native) runs a headless scenario, prints one line, and exits; `?bench=1` runs it in browsers before the demo starts, logs it to the console, and shows it in the sidebar. Both bins fill with 50 props over 1,200 fixed steps. The report gives CPU milliseconds per fixed step for the whole run and for the final, settled two seconds, plus a hash of every body's final position. Matching SDF hashes between a native run and a browser run show that SDF contact generation is deterministic across targets.

## Verification
`test/rust/example_sdf_collisions.rs` checks that the props bake into colliders that fit their meshes, and that the benchmark piles repeat bit for bit with no SDF prop leaving its bin. `test/rust/physics.rs` covers SDF resting contact, stacking, raycasts, validation, mesh baking, and determinism in the engine itself. Browser captures add canvas startup and image evidence. CPU timing does not measure GPU execution.
