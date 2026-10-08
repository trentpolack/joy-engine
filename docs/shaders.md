# Authored shaders
Reusable engine GPU programs live in `joy-engine/shaders/`, with matching WGSL and GLSL counterparts reviewed together. JavaScript selects and binds these files but does not embed shader implementations.

- `particles/` contains reusable particle materials.
- `rendering/lit-triangles/` contains reusable point-lit triangle programs with optional atmosphere directional lighting and colored skylight.
- `rendering/environment/` contains the portable procedural atmosphere pass. Its first version renders scattering, sun/moon discs, wind-driven clouds, and a forward-scattering light-shaft contribution from an `EnvironmentFrame`; depth-aware fog, occlusion-aware shafts, cloud shadows, and ambient occlusion remain follow-up scene passes.
- `rendering/postprocessor/modules/` contains supplemental Shadertools modules for AgX, linear exposure, animated grain, adjustable FXAA, and vignette-blur composition. Upstream luma.gl supplies bloom, Gaussian filtering, grading, vignette, and temporal/scene effects.
- `projects/<name>/shaders/` contains exceptional, behavior-named materials and effects that the standard scene contract cannot express. Projects import those files locally and pass their source to a low-level Joy Engine renderer; the engine never imports a project shader. Generic `scene`, `world`, or `arena` programs belong in Joy Engine instead.

`SceneRenderer` is the default composition root for games and editor viewports. It selects the engine-owned scene programs and enables the shared postprocessor by default. Pass `postProcessing: false` only for focused rendering diagnostics or a deliberately raw offscreen surface. Use `GpuTriangleRenderer` directly when a project-specific material needs a different uniform or shading contract, while retaining engine ownership of devices, targets, backend fallbacks, and cleanup.

Matching backend files must preserve bindings, vertex locations, color space, coordinate conventions, and comments describing non-obvious math.


Enable the procedural sky with `new SceneRenderer(canvas, {atmosphere: true})`, then supply `environment: environmentSystem.step(simulationSeconds)` to `render`. The renderer draws the sky before opaque geometry into the same linear scene target, leaves depth untouched, and uses the frame's primary light and skylight for scene irradiance. Omit the frame to hide the sky. Atmosphere allocations belong to the renderer and are disposed with it; the simulation remains caller-owned.
