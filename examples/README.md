# Joy Engine examples
These small feature demonstrations live beside the engine rather than among playable projects. They consume the public `joy-engine` package, keep assets and controls local, and remain npm workspaces so root checks and tests cover them. Site discovery and automatic release builds/captures exclude them; build and run each example explicitly.

## Terrain
The [terrain example](terrain/README.md) demonstrates resident indexed CLOD terrain, CPU/WebGPU selection parity, and MacBook-friendly fly/orbit/pan controls.

```bash
npm run dev:example:terrain
npm run build:example:terrain
npm run capture:example:terrain
npm run capture:example:terrain -- --built
```

Run commands from the repository root. Add future examples under `joy-engine/examples/<feature>/` with a private npm package, a README, focused checks, explicit cleanup, and an optional browser capture command. Do not add `gameSite` metadata or main-site previews. Promote stable mechanisms into `joy-engine/src` when at least two consumers need them; engine runtime modules must never import example code.
