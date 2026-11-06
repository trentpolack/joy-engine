# FORM Viewer
Inspect the engine's bundled FORM scripts and instances with live scalar parameters, Lit/Layered/Unlit shading, and time-of-day controls. Drag with the primary mouse button to orbit, use Distance to zoom, and select an asset to reframe it.

```bash
npm run dev --workspace @joy-engine/example-form-viewer
npm run build --workspace @joy-engine/example-form-viewer
npm run preview --workspace @joy-engine/example-form-viewer
npm run check --workspace @joy-engine/example-form-viewer
npm run test --workspace @joy-engine/example-form-viewer
npm run capture --workspace @joy-engine/example-form-viewer
```

Native development runs the Cargo example. The browser build writes to this workspace's `dist/`; `preview` serves that build. Capture builds a development browser version by default; append `-- --built` to capture an existing build. The example remains local, without `gameSite` metadata or automatic site publication.

The test command compiles the viewer's Cargo test target and runs the FORM crate's evaluation and document tests. The viewer has no dedicated behavioral tests; captures check browser startup, rendering, resize, and browser errors.
