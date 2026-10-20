# Rust FORM
The FORM procedural language, the `joy-form` crate (`crates/joy-form/`, re-exported as `joy_engine::form`). It parses `.form` scripts and returns plain geometry records, preserving the established language and asset contracts. It is renderer-free (the content layer may not use rendering): no GPU, window, worker, timer, or host handle is involved, and it builds identically for native and `wasm32-unknown-unknown` targets. Joy Engine's renderer consumes its output.

## API
```rust
use joy_engine::form::{CompiledScript, mesh_box};
use serde_json::json;

let script = CompiledScript::parse(source)?;
let geometry = script.evaluate(&json!({"width": 8}), None)?;
for material in geometry.form_materials() {
    // Typed name, base color, metallic, roughness, emissive, alpha mode, cutoff, and textures.
}
let unit_box = mesh_box(1.0, 1.0, 1.0)?;
```

- `CompiledScript::parse(&str) -> Result<CompiledScript, Diagnostic>` validates syntax and source budgets without executing anything. `CompiledScript::evaluate(&self, overrides, materials) -> Result<GeometryData, Diagnostic>` runs it; `compile(source, overrides, materials)` does both.
- `overrides` is a JSON object keyed by parameter name. Unknown names are ignored, numeric values clamp to declared bounds without step quantization, and mesh or material values use the serialized FORM resource records. `materials` is an optional palette of serialized material records; `None` yields the single Default material.
- `Diagnostic { message, line, column }` implements `Error` and `Display`. Lines are one-based and columns count characters from one, matching FORM LAB's editor. Failures without a source position report line 1, column 1.
- `GeometryData` holds indexed, Y-up geometry in script units: flat `positions`, `normals` (zero means flat-shaded), `uvs`, `colors`, `alphas`, `triangles`, `points`, per-primitive material indices, serialized `materials`, `parameters`, `project`, and optional `userData` and `outputs`. Its JSON field names preserve the established document schema; absent user data and outputs are omitted and an absent project is `null`.
- `GeometryData::form_materials()` returns typed `FormMaterial` records (with `AlphaMode` serialized as `OPAQUE`, `MASK`, or `BLEND`, and optional embedded texture URIs) in palette order, so triangle and point material indices address it directly. Missing or malformed fields fall back to `default_material` values.
- Tools validate document values with the same rules evaluation uses: `read_materials` (palette with Default first, 128 materials, embedded PNG, JPEG, or WebP maps within `DOCUMENT_TEXTURE_BYTES` each and `DOCUMENT_TOTAL_TEXTURE_BYTES` together), `default_material`, `read_parameter_value` (scalars, XYZ vectors, RGBA colors, and mesh or material resources, with integral numbers kept as JSON integers), `read_mesh`, `read_material_value`, and the resource constructors `mesh_box_value` and `mesh_sphere_value`; failures are `FormValueError`s. `project_name_tokens(source)` locates the quoted name of each top-level `project meta(name="…")`, so FORM LAB renames a project without reformatting its metadata.
- `mesh_box(width, height, depth)` and `mesh_sphere(radius, segments)` return indexed primitive geometry (`DEFAULT_SPHERE_SEGMENTS` is 20), with FORM's default vertex tint and Default material. They return a `Diagnostic` for nonpositive sizes or out-of-range segment counts instead of panicking.

## Ownership
Every evaluation creates its own geometry, random stream, material-identity registry, procedural store, and allocation and work budgets. A `CompiledScript` retains only immutable syntax, so evaluations are independent and deterministic for identical inputs. Returned data is owned by the caller and never borrows the overrides, the palette, or earlier results. Geometry bindings alias within an evaluation; `copy`, resource conversion, and named outputs take owned snapshots.

## Budgets
Source capacity is 200,000 characters and 40,000 tokens, with nesting limits of 48 blocks and 64 expressions. Evaluation allows 2,000,000 operations, 150,000 emitted vertices, 180,000 emitted triangles, and 150,000 cumulative resource vertices. Procedural allocation separately tracks 150,000 points, 180,000 faces, and 2,000,000 attribute components. Coordinates must stay finite and within ±1,000,000. Texture limits are 4 MB each and 32 MB per palette; material, parameter, named-output, and user-data budgets are also enforced. Flat binary and component chains are evaluated and dropped iteratively, so documents near the token limit do not overflow a native or browser stack. Authored content never panics; limit violations, malformed values, and unsupported functions return a `Diagnostic`.

## Numerical and Text Boundaries
Quoted text must contain valid Unicode scalar values; unpaired surrogate escapes are rejected. Diagnostics count character columns, and a leading byte-order mark takes no column. User data fields keep their authored order; `__proto__`, `prototype`, and `constructor` stay reserved because exported user data reaches JavaScript glTF consumers. Procedural transforms reject nonfinite computed normals at their source location while preserving legitimate zero-normal sentinels. Floating-point JSON may write an integral value as `5.0`; its numerical value remains unchanged. Recorded-reference comparisons use explicit tolerances for transcendental rounding.

## Validation
The tests (`joy-form` unit tests and the `form_*` modules in `crates/joy-form/tests/integration/`, run with `cargo test -p joy-form`) cover parse and evaluation isolation, ownership, parameter clamping, precedence and numerical rounding, resources and transforms, scoped user data, diagnostics and operation limits, procedural topology and attribute rules, wrangles, retained instances, shapes and scatter, material and texture validation, typed materials, primitive parity with recorded reference values, and the evaluator's request framing.

```sh
cargo +nightly fmt --check
cargo clippy --all-targets -- -D warnings
cargo test form
cargo build --target wasm32-unknown-unknown --lib
```

FORM LAB preserves a frozen 69-case reference corpus covering shipped examples, recipes, structures, semantics, and rejections. The retained Node harness drives the Rust `joy-form-evaluate` JSON-lines CLI (built with `cargo build -p joy-form --bin joy-form-evaluate`, without the renderer or window stack) and compares geometry, numeric tolerances, and diagnostic locations against those fixtures; it does not require a second compiler implementation.

```bash
npm run test:parity --workspace @joy-games/form-lab
```

The CLI accepts one source/overrides/materials request per line and returns success geometry/timing or a message with line and column. Its request limit is 32 MiB. The harness builds the CLI before comparison and writes ignored evidence artifacts.
