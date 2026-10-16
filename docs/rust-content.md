# Content Formats
The content layer covers `dcc`, `project`, and `scripting`, alongside [FORM](rust-form.md). These modules return plain data, perform no host I/O, and compile for native and WebAssembly targets. Callers own reading bytes and creating rendering resources.

## GLB Meshes
`dcc::parse_glb_mesh(bytes, mesh_name)` returns `core::IndexedMesh3D`: a name, flat `f64` positions/normals, and `u32` indices. Supported inputs include GLB JSON/BIN chunks, embedded base64 buffers, buffer/accessor offsets, interleaved strides, sparse substitution, and accessors without buffer views.

Select a named mesh or the first mesh. The selected mesh must have one triangle primitive, float VEC3 positions/normals with matching counts, finite values, complete triangles, and in-range indices. Unsigned indices are supported; unindexed geometry gets sequential indices. Scene transforms, materials, and animation are not interpreted.

External buffer URIs return `GlbError::ExternalBuffer`; Draco, meshopt, and quantization extensions return `UnsupportedExtension`. Corrupt containers/accessors return typed errors. The caller must fetch or embed bytes before parsing.

## Project Manifests
`project::parse_project_manifest` validates `project.joyproject`: schema version 1, required trimmed text, optional `$schema`, normalized relative preview/asset paths, unique capability IDs, extensions, and feature flags. Absolute paths, drive letters, and parent traversal are rejected. Serialized names and ordering remain compatible with existing documents.

## Script Language Information
`ScriptLanguageCatalog` and `ScriptLanguageSymbol` carry names, kinds, details, documentation, signatures, and writability independently of an editor UI. `form::form_language_catalog()` and `particles::particle_language_catalog()` supply language-owned catalogs. The [authoring kit](rust-authoring.md) uses them for highlighting, completion, and hover help.

## Verification
Rust tests use recorded GLB fixtures for strides, sparse accessors, index widths, embedded glTF, and rejection behavior. Manifest tests cover shipped documents, normalization, and rejected paths. These fixtures remain useful without the retired runtime.
