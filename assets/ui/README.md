# UI Appearance and Layout
These files are the source of the shared UI values for native and browser builds. Cargo reads them through `crates/joy-ui/build.rs` and writes generated Rust into its ignored build directory. Change an asset and rebuild normally; do not edit generated output. There is no runtime file loading, theme picker, or hot reload.

- `theme-default.json`: `game` contains the base HUD palette; `editor` contains editor, lab, syntax, and development-overlay colors. Colors use `#RRGGBB`. `fonts` selects bundled faces for editor body text, strong text, display text, code, and game strong text. `opacity` controls the game panel's opacity.
- `layout.json`: numeric dimensions in logical pixels. `type` sets shared text sizes, `space` sets spacing, `radius` sets corners, `border` sets stroke widths, `size` sets shared control dimensions, and `game_panel` sets the base HUD frame geometry. These shared metrics do not replace each tool's screen composition, specific dimensions, or saved pane arrangement.

For example, change `theme-default.json` → `editor.panel` to recolor editor panels, `fonts.code` to select another bundled code face, or `layout.json` → `type.code` to resize code text. Values apply on the next build to Joy Editor, FORM LAB, and PARTICLE LAB through the shared UI implementation.

Rust consumers use `theme::colors` for the editor palette and `theme::layout` for dimensions. `theme::apply_editor` configures the editor/lab preset; `theme::apply` configures game UI using the top-level game colors. Stable egui family/style IDs and the binding of licensed font files remain in Rust because they are implementation contracts, not tuning values.

Available faces are Archivo, Archivo SemiBold, Archivo Medium, Archivo Bold, Archivo Black, and Meslo LG S. Adding a face requires bundling the font, registering it in the runtime catalogue and build validation, and including its license in build notices. Selecting an existing face only requires editing `theme-default.json`.
