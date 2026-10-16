# Rust Content
The content layer of Joy Engine's Rust crate beyond FORM ([Rust FORM](rust-form.md)): digital content creation imports (`src/dcc`), project manifests (`src/project`), and scripting language information (`src/scripting`). Like FORM, these modules are renderer-free, may use only the foundation and simulation layers, perform no file or network I/O (loading bytes is the caller's job), and build identically for native and `wasm32-unknown-unknown` targets.

## Module Map
| Module | Contents | TypeScript source |
| --- | --- | --- |
| `dcc` | `parse_glb_mesh(bytes, mesh_name) -> Result<IndexedMesh3D, GlbError>` | `dcc/glb-mesh-loader.ts` (`parseGlbMesh`) |
| `project` | `ProjectManifest`, `ProjectPreview`, `ProjectCapability`, `parse_project_manifest`, `parse_project_manifest_text`, `JOY_PROJECT_MANIFEST`, `ProjectManifestError` | `project/project-manifest.ts` |
| `scripting` | `ScriptLanguageCatalog`, `ScriptLanguageSymbol`, `ScriptSymbolKind` | `scripting/language-information.ts` |

## GLB Meshes
`parse_glb_mesh` decodes one indexed triangle mesh from binary GLB or JSON glTF 2.0 bytes into `core::IndexedMesh3D` (name, flat XYZ positions and normals as `f64`, and `u32` indices). The TypeScript loader delegates decoding to loaders.gl; the Rust module implements the needed subset of the glTF specification directly instead of adding a dependency: GLB containers (JSON chunk plus BIN chunk), buffers embedded in the BIN chunk or as base64 `data:` URIs, buffer-view and accessor offsets, interleaved strides, sparse substitution, and accessors without a buffer view. Selection and validation match the TypeScript loader: the named mesh (or the first; an empty name also selects the first), exactly one primitive, triangle mode, float VEC3 positions and normals, optional unsigned indices, sequential indices for unindexed triangles, matching counts, complete triangles, finite values, and in-range indices, with the same messages. Scene transforms, materials, and animation are not interpreted.

Differences: `loadGlbMesh(url)` is not ported because fetching is the caller's job; external buffer URIs return `GlbError::ExternalBuffer`; assets requiring Draco, meshopt, or quantization extensions return `GlbError::UnsupportedExtension`; and container or accessor corruption returns typed errors (loaders.gl threw its own messages, eagerly for every accessor, where Rust decodes only the accessors the selected mesh uses).

## Project Manifests
`parse_project_manifest` validates and copies a parsed `project.joyproject`: schema version 1, trimmed required text, an optional `$schema` reference kept for tools, preview and asset paths normalized to forward slashes (a leading `./` removed) that must stay inside the project folder (no absolute paths, drive letters, or `..` segments), unique kebab-case capability ids, at least one lowercase dot-notation extension per capability, and feature flags converted by JavaScript truthiness. Messages match the TypeScript engine, and `ProjectManifest` serializes in the TypeScript key order. Every JoyGames project manifest is a test fixture.

## Scripting Language Information
`ScriptLanguageCatalog` and `ScriptLanguageSymbol` are the editor-neutral symbol records (name, kind, detail, documentation, display signatures, and writability) editors use for completion and hover help; they exchange as JSON with the TypeScript editor. The catalogs themselves belong to their language modules and return these records: `form::form_language_catalog()` (with `FORM_KEYWORDS`, `FORM_COMMANDS`, and `FORM_ATTRIBUTE_DOMAINS`, from `form/language-information.ts`) and `particles::particle_language_catalog()` (from `particles/language-information.ts`). The authoring kit's script editor highlights, completes, and documents scripts from them ([authoring kit](rust-authoring.md)).

## Parity
GLB decoding is compared exactly against the TypeScript loader on small generated fixtures (`test/fixtures/glb/`: a multi-mesh GLB covering strides, sparse accessors, index widths, and every failure message, an embedded JSON glTF, and a FORM LAB GLB export), and manifest parsing against every JoyGames manifest plus normalization and rejection cases.
