# joy-foundation
Shared data and algorithms for Joy Engine's Rust workspace: the foundation layer every other crate builds on. It has no GPU, window, physics, or file-system dependencies and builds identically for native and `wasm32-unknown-unknown` targets.

## API
- `Transform` (position, XYZ Euler rotation applied X then Y then Z, and scale), `quat_from_euler`, `euler_from_quat`, and `Transform::attach` for details that follow a parent's pitch, roll, and scale.
- `SeededRandom` and `RandomSource`: the TypeScript `createSeededRandom` linear congruential stream, bit for bit. Use it wherever gameplay must be reproducible; keep cosmetic randomness on a separate stream.
- `Rgb`/`Rgba` linear colors and `parse_hex_color`; `clamp`, `smoothstep`, `random_range`, and related helpers.
- `PrimitiveShape` (`box`, `sphere`, `plane`) with the serialized spellings levels use.
- `PostProcessingConfig`, `PostProcessingProfile` (`validate`, `resolve`), presets, and the schema shared with the TypeScript post-processing helpers, so one profile file drives both renderers.

Values and serialized forms match the TypeScript engine; the unit tests compare against values recorded from it.
