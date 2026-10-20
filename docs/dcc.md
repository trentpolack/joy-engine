# DCC Interchange
`joy_engine::dcc::parse_glb_mesh` parses an indexed triangle mesh from GLB or embedded glTF bytes into a portable `render_types::Mesh`. The caller owns loading bytes, choosing a mesh, and applying project scale/orientation. The importer does not own a catalog, filesystem, network service, renderer, materials, or animation.

See [content formats](rust-content.md) for supported accessor layouts and explicit unsupported formats. FORM LAB's GLB export is a separate authored-geometry boundary; exporting a mesh does not preserve its procedural recipe.
