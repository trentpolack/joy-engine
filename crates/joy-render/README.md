# joy-render
The definitive Joy Engine renderer: one wgpu renderer for games, FORM content, level previews, and tools, on WebGPU and WebGL 2 in browsers and Vulkan, Metal, DirectX 12, or OpenGL natively. Shaders are WGSL files in `shaders/`.

## API
- `GpuContext` creates the instance, adapter, and device (`new` for a surface, `headless` for offscreen work) and reports `GpuCapabilities`. HDR scene targets are used when the device can render, filter, and blend `Rgba16Float`.
- `Renderer` draws a retained `RenderScene` from a `Camera` with per-frame `FrameInputs` (environment lighting, point lights, ambient, clear color, and transient triangles). `render` submits; `encode` only records, for callers that add UI to the same frame. It owns every GPU resource it creates and releases mesh buffers once no handle references them.
- `MeshAsset` (shared geometry with vertex colors and baked material factors), `MeshInstance` (transform, tint, opacity, pass, shading, and shadow flags), `RenderScene`, and `MeshCollection` for keyed synchronization with simulation records.
- `Shading::{Lit, Pbr, Unlit}`: Lit is the engine's flat, double-sided vertex-color model; PBR is metallic-roughness GGX.
- `EnvironmentLighting` converts from `joy_simulation::EnvironmentFrame` and packs the same uniform layout as the TypeScript environment.
- `LevelScene` presents authored or runtime entities (`render` primitives and meshes, `light` point lights); `FormAssetLibrary` compiles and caches `.form` and `.formlab` assets for it.
- `OrbitCamera`, `screen_ray`, `project_to_viewport`, and ground picking; `OffscreenTarget` for native readback.

See [the Rust engine guide](../../docs/rust.md) for the frame structure and parity notes.
