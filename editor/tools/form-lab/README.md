# FORM LAB
This workspace contains the playable project, its project-owned content, and its build configuration.

FORM SCRIPT uses flat face normals by default, preserving hard edges. `normal(x, y, z)` sets a normalized direction for subsequently authored vertices; `normal()` restores flat shading. `surface()` and `sphere()` generate UV coordinates automatically; `uv(u, v)` sets coordinates for subsequently authored vertices. Materials accept PNG, JPEG, or WebP maps for base color, normal, emissive, metallic, roughness, and ambient occlusion data.

New scripts use object-first geometry: `box(width,height,depth)`, `sphere(radius,segments)`, and `grid(...)` return editable values; `translate`, `rotate`, `scale`, and `color` modify an explicit geometry value. Named wrangles such as `wrangle points in terrain as point` expose `point.position`, `point.color`, and `point.index` without implicit current-element syntax. Wrangles require both `in` and a named `as` binding: `wrangle points in terrain as point { ... }`. Attribute reads and assignments require an existing field; explicitly declare custom fields with `addAttribute(terrain, "points", "density", 0)` and point colors with `color(terrain, rgba(1,1,1,1))`. Older scripts must add `in` and `as point`, replace `@name` with `point.name`, and replace implicit attribute creation with explicit declarations. The retired `@` syntax reports a migration diagnostic. Existing immediate geometry commands remain supported.

See the [project documentation](https://github.com/trentpolack/JoyGames/blob/develop/docs/projects/form-lab/README.md) for the application overview, controls, architecture, development workflow, and validation guidance.


### Workspace navigation
Drag the pane dividers to resize the workbench. Balanced, Script, and Preview profiles remain implemented and saved preferences still load, but the profile toolbar is hidden for now. Focus a divider and use arrow keys to resize; double-click restores Balanced. Desktop widths persist separately from your project. The inspector separates parameter and material editing, includes parameter search, and keeps exports available. Use Maximize in the preview header to expand the entire lab while retaining script and Inspector panels; Restore returns to the previous layout. Use Recipes for appendable starting points and Go to error to focus a compiler diagnostic.

### Reusable takes and profile tools
The Variants inspector saves up to 16 named sets of explicit parameter overrides in `.formlab` files and autosave. Apply replaces current overrides, while the script and global material palette stay shared. Mesh and material parameter overrides are included. Unchanged parameters continue to use the script's defaults; removed parameter names are ignored. Use a new name to save another take, or Rename to rename the selected take.

The Vessel workshop and Path workshop demonstrate generated profiles and paths without requiring external art. Recipes includes smaller appendable examples. All operations return ordinary editable geometry and work in the renderer-independent `joy-engine/form` runtime:

| Operation | Behavior |
| --- | --- |
| `revolve(profile, segments)` | Revolves ordered `(radius,height,0)` points about Y. Nonnegative radii, 3–96 segments; bottom-to-top order faces outward. Zero-radius endpoints close the surface, other ends stay open. |
| `tube(path, radius, sides)` | Sweeps ordered XYZ points with a positive radius and 3–64 sides. Open ends, projected frames; exact reversals and adjacent duplicates are rejected. Sharp bends can self-intersect. |
| `smoothNormals(g)` | Copies geometry and replaces normals with area-weighted point normals. Preserves topology, colors, custom attributes, and corner UV seams. Isolated or degenerate points receive zero normals. |

Profiles and paths contain 2–512 points. Their topology and attributes are not transferred to generated surfaces. Generated UVs use circumference for U and point-order fraction for V; edit corner UVs for another mapping. All generated data, copies, and outputs count against evaluation budgets. Profile operations are building blocks, not watertight-solid or collision solvers.

### Compiler lifecycle
Compiler module startup has a bounded 10-second loading window. Once the worker begins compiling, the existing two-second script execution budget applies. Rejected or interrupted builds retain the last accepted geometry. Run `npm run test:worker-browser --workspace=@joy-games/form-lab` for the delayed-start, execution-limit, retained-preview and recovery browser regression; set `CAPTURE_EXECUTABLE` when using an installed local Chrome.
