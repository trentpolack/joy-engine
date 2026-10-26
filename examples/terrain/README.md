# Terrain Example
Dragon Pit demonstrates streamed quadtree terrain through the Rust engine: a paged tile pyramid with continuous LOD, synthesized detail below the authored two-unit spacing, craters, and physics props resting on terrain colliders. From JoyGames run `npm run dev:example:terrain` for a native window or `npm run build:example:terrain` for browser output in this workspace's `dist/`. The site route is `/examples/terrain/`. From the engine root use `cargo run --example terrain`.

Primary drag orbits, wheel zooms, and right-button flight uses WASD and Q/E. Command or Alt plus wheel pans; G selects a grazing view and O restores the overview. C blasts a crater under the pointer and P drops SDF crates and spheres near the focus. CONFIG provides tolerance, LOD visualization, synthesized detail, a camera tour, stable detail, a crater at the focus, and a prop drop. The landscape draws through the engine renderer as a scene terrain instance, lit, shadowed, and under the sky. Browser query `backend=webgpu` or `backend=webgl` selects the backend.

## Streaming
At startup the embedded version 1 asset becomes a five-level pyramid (one root tile, 64-cell tiles) served from memory after a two-update delay, so paging is visible without a network. The stable-detail pin holds authored detail around the focus; synthesized detail follows screen error and the material under it. The HUD shows drawn tiles, the finest drawn level (negative levels are synthesized detail), pool layers and uploads, resident, pending, and synthesized tiles, terrain colliders, props, craters, and the gameplay ground height under the focus.

To stream real files, package the asset and point the example at the folder (natively) or URL prefix (browsers):

```sh
cargo run -p joy-terrain --bin joy-terrain-package -- examples/terrain/assets/dragon-pit/terrain.json /tmp/dragon-pit-package
cargo run --example terrain -- --package=/tmp/dragon-pit-package
```

In a browser, serve the package folder beside the page and open `?package=<prefix>`; the page then fetches only the tiles the view and the props need. A missing or invalid package falls back to the embedded asset with a status message.

## Author to Runtime
Run these offline conversion commands from the JoyGames root, which owns the terrain conversion scripts.

The [source EXR](https://github.com/trentpolack/worldcreator/blob/main/dragon_pit_01/HeightMap.exr) is Trent's MIT-licensed World Creator export. The original is 268,566,805 bytes and is deliberately excluded from this repository. Its SHA-256 is `f64f35d93079db360de0df5740cdbd0c1d16e18dd776b274eb98c7c1257cb448`.

```sh
python3 -m venv /tmp/joy-terrain-python
/tmp/joy-terrain-python/bin/pip install OpenEXR==3.5.1 numpy==2.5.3
curl -L --fail https://media.githubusercontent.com/media/trentpolack/worldcreator/main/dragon_pit_01/HeightMap.exr -o /tmp/dragon-pit.exr
/tmp/joy-terrain-python/bin/python scripts/assets/compile-terrain.py /tmp/dragon-pit.exr joy-engine/examples/terrain/assets/dragon-pit --channel Y --size 1025 --tile-cells 64 --extent 2048 2048 --vertical-scale 1000 --source-url https://github.com/trentpolack/worldcreator/blob/main/dragon_pit_01/HeightMap.exr
/tmp/joy-terrain-python/bin/python scripts/assets/test-terrain.py
```

The source has one `Y` float32 channel, data/display window `[0,0]`–`[8191,8191]`, and 67,108,864 finite values in `[0.08760054409503937,0.5539144277572632]`. The compiler inspects all channels before conversion and requires the height channel explicitly. It rejects nonfinite samples rather than silently filling or clipping them. MIT source notice is retained beside runtime data in `LICENSE-source.txt`; OpenEXR (BSD-3-Clause) and NumPy (BSD-3-Clause) are offline tools, not browser dependencies.

Demo dimensions are explicit example units: X/Z extent 2048 × 2048, Y = source height × 1000 with offset zero. They are not inferred physical meters. Source row zero maps to world negative Z, last row to positive Z, first column to negative X, last to positive X; no row flip or transpose is applied. This preserves source raster orientation; no unprovided compass orientation is inferred. The compiler globally resamples the full 8192-square raster to 1025-square using bilinear interpolation with endpoints preserved, then applies one uint16 range derived from the full source. The maximum quantization error is 0.003558 world units. Downsampling error relative to the 8192-square source is separate and is not included in the render LOD budget. Use a larger supported `--size` (up to 4097) if source-scale detail is required; rebuild bounds and errors with every asset change. No tile is renormalized independently.

## Runtime Contract
The embedded version 1 asset uses a square power-of-two-plus-one grid, power-of-two leaf size, row-major uint16 little-endian height data, explicit scales, global quantization, and precomputed patch bounds/errors. `build_pyramid_from_asset` keeps those level-0 errors and measures coarser levels; `joy-terrain-package` writes the same pyramid as a version 2 package. See [terrain implementation](../../docs/rust-terrain.md) for selection, streaming, detail, craters, and sampling.

Synthesized detail uses `TERRAIN_CONFIG.streaming.detail`: three levels below the authored spacing (down to half a world unit), four octaves, and amplitudes of 0.35 (ground), 1.0 ridged (rock), and 0.15 (snow) world units; the streamer keeps at most 1,536 tiles and synthesizes 12 per update. Gameplay height and colliders read level -2. Craters are 7 units across with a 3.5-unit bowl. Props are SDF crates (exercising SDF contacts against terrain) and solver spheres; colliders follow the focus and every prop within a 48-unit radius.

## Boundaries and Validation
This example does not create vehicle collision, roads, or authored terrain editing in Joy Editor. Prop scatter uses its own seeded stream and never changes terrain data.

Engine Rust tests cover pyramid measurement, quadtree selection, streaming, detail, craters, collision, and headless rendering. `tests/integration/example_terrain.rs` drives the lab through key input (streaming into detail, a crater under the pointer, and props landing on the terrain) and streams a written package through the platform fetcher. `npm run capture --workspace @joy-engine/example-terrain -- --built` adds browser startup/image evidence. Inspect visuals and camera interaction on target hardware; timings and software captures are not a GPU performance benchmark.
