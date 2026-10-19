# Terrain Example
Dragon Pit demonstrates resident continuous-LOD terrain through the Rust engine. From JoyGames run `npm run dev:example:terrain` for a native window or `npm run build:example:terrain` for browser output in this workspace's `dist/`. The site route is `/examples/terrain/`. From the engine root use `cargo run --example terrain`.

Primary drag orbits, wheel zooms, and right-button flight uses WASD and Q/E. Command or Alt plus wheel pans; G selects a grazing view and O restores the overview. CONFIG provides tolerance, LOD visualization, a camera tour, stable detail, and a switch between standalone terrain and the scene renderer. Browser query `backend=webgpu` or `backend=webgl` selects the backend.

## Author to runtime
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
Version 1 uses a square power-of-two-plus-one grid, power-of-two leaf size, row-major uint16 little-endian height data, explicit scales, global quantization, and precomputed patch bounds/errors. `parse_terrain` decodes loaded bytes; native `load_terrain` reads files. `sample_terrain` returns the finest gameplay surface independently of render LOD.

The indexed grid and heights remain resident. Compute-capable backends select LOD and indirect draws on the GPU; GL backends apply the same policy on the CPU. Morphing and reconciled edges keep neighboring patches connected. The scene path participates in lighting, shadows, and scene attachments. The standalone pass provides diagnostic diffuse rendering. See [terrain implementation](../../docs/rust-terrain.md).

## Boundaries and Validation
This example does not create vehicle collision, roads, streaming, or terrain editing. Any future collision/road adapter must use the same authored coordinates and finest-height sampling. Re-export bounds/errors when height data changes.

Engine Rust tests cover recorded CPU selections, CPU/GPU selection agreement, sampling, scene/shadow integration, invalid settings, and headless rendering. `npm run capture --workspace @joy-engine/example-terrain -- --built` adds browser startup/image evidence. Inspect visuals and camera interaction on target hardware; timings and software captures are not a GPU performance benchmark.
