# Authored Shaders
Engine GPU programs are standalone WGSL files under `crates/joy-renderer/src/shaders/` and `crates/joy-renderer/src/scene_effects/shaders/`. Rust loads them with `include_str!`; wgpu translates them for WebGPU, WebGL 2, Vulkan, Metal, and DirectX 12.

Materials own their shader bindings, vertex layouts, depth/blend state, and texture contracts. The renderer owns frame targets, pass order, and resource lifetime. Keep reusable scene, lighting, particle, terrain, and display programs in the engine.

A project may own a behavior-specific WGSL program when the standard scene material cannot express its needs. It supplies that program through the engine renderer and never creates its own backend device. Explain coordinate conventions, packed uniforms, color space, and non-obvious numerical choices in shader comments. Do not generate substantial shader source from formatted strings.
