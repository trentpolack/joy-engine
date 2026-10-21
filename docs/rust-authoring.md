# Rust Authoring Kit
The authoring kit is the shared egui toolkit that FORM EDITOR, JOYFX EDITOR, and Joy Editor build their UI from. It is the `joy-authoring` crate (platform layer), reached through the engine's `development` module, and includes the [`AuthoringTool`](../crates/joy-authoring/src/authoring_tool.rs) hosting contract, so tools depend only on `joy-engine`. Its egui widgets use explicit ownership: controls borrow authored values and return what happened, and tools own their documents.

Keep tool UI in modules separate from document and model logic; the kit follows the same split (`lab_values.rs` holds the UI-free rules behind `lab_controls.rs`), so a later UI technology change touches only UI modules.

## Look
`runtime::theme::apply_editor(ctx)` applies the editor/lab preset to an egui context. Its values come from two editable assets: [`assets/ui/theme-default.json`](../assets/ui/theme-default.json) owns game/editor palettes, syntax colors, and font choices; [`assets/ui/layout.json`](../assets/ui/layout.json) owns shared type sizes, spacing, corners, borders, and control heights. Cargo generates the Rust constants when either file changes, for native and WebAssembly builds. Edit the JSON and rebuild; no separate generation command or duplicated Rust values are needed. Invalid values stop the build with the asset and field name.

Use `runtime::theme::colors` for editor, lab, and development-overlay colors, `runtime::theme::layout` for shared dimensions, and `runtime::theme::text_styles` for semantic styles (`display`, `title`, `caption`, `status`). Games keep `runtime::theme::apply` and the top-level game colors, also generated from the theme asset. Screen composition, tool-specific dimensions, and saved pane arrangements remain in the tools; these shared assets do not describe whole screens. This is a pair of build-time presets over egui, not a runtime theme picker or hot-reload system. See the [UI asset guide](../assets/ui/README.md).

The default editor faces are Archivo Medium for body text, Archivo Bold for emphasized text, Archivo Black for titles (SIL Open Font License), and Meslo LG S for code (Apache 2.0), with egui's symbol fallbacks. The licensed font catalogue stays bundled in `crates/joy-ui/fonts/`, and web builds include its notices.

`development::components` adds the pieces egui has no style slot for. Application chrome: `app_title` (product name in display type with an outlined version badge), `header_rule`, `project_marker`, and `status_bar`. Lists and text: `list_row`, `panel_heading` (`01 COMPOSE`), `caption`, `field_title`, `guidance`, `error_text`, `status_badge`, and `spaced` letter-spaced runs. Actions and frames: `primary_button`, `danger_button`, `toggle_button`, `panel_frame`, and `toolbar_frame`. Build tool chrome from these rather than painting it per screen.

## Lab Controls
Discrete controls edit the same `&mut f64` values: `toggle_control` (zero or one), `choice_control` (an option index), and `seed_control` (a whole-number seed with a reroll button; `rerolled_seed` is the reproducible next seed). Each commits immediately, like an arrow-key step.

| Widget | Purpose |
| --- | --- |
| `NumberField` | Exact numeric field. Shows the authored value exactly (`lab_values::format_number`), scrubs by horizontal drag (one step per 2 points), steps with arrow keys (Shift a tenth, or one for integer fields; Alt ten steps), and validates drafts on Enter or focus loss against bounds, integrality, and an optional extra `check`. |
| `ScalarControl` | Bounded scalar: a slider over the authored range plus an exact field, an optional label column, and a reset button while modified. The slider appears only for finite, distinct bounds. |
| `ComponentGroup` | XYZ (`xyz`) and RGBA (`rgba`) groups: equal columns with a component label, an exact field, and a short slider per bounded component; per-component defaults and resets. |
| `ColorControl` | Linear-RGBA swatch over a checkerboard with an sRGB picker and hex entry. Choosing converts RGB to linear within the owner's bounds and preserves alpha. |
| `lab_slider` | The lab range slider (thin rail, accent fill, round thumb) used by the controls and available alone (JOYFX EDITOR's timeline). It never clamps or rounds values it did not edit; arrow keys step it while focused. |
| `reset_button`, `modified_marker`, `is_modified` | Default/modified indication: modified values draw in the accent color, and the reset button restores the default. |

Every control returns a `ControlResponse` with `changed`, the changed `component` for groups, and an `EditPhase`:
- `Continuing`: a slider drag, number scrub, or picker drag moved the value and is still in progress.
- `Committed`: a gesture ended, or a discrete edit happened (exact entry, arrow step, reset, picker choice). Record history now.

Controls carry accessible names (`ScalarControl::name` and `ComponentGroup::name` give `power value`, `power slider`, `tint A value`; `NumberField::label` names a lone field; swatches read `<label> color picker`), which screen readers and headless tests use through egui's AccessKit output.

Values are never rounded for display. Sliders use `SliderClamping::Edits`, so a value outside its range stays untouched until the author drags; the color picker writes only when the author changes it (floating-point noise from its color-space round trip is ignored), never when it opens or refreshes.

### Numeric Drafts
`NumericDrafts` (one per tool document view) collects invalid drafts. A rejected draft never changes the value; it stays in its field in the danger color with its message until repaired by a valid entry, a slider move, a reset, or Escape back to the accepted value. Text being typed that is not yet valid also counts. Before Apply, export, switching emitters, loading examples, or anything that would replace fields, call `drafts.require_valid()` and show its message instead of continuing; the color picker refuses choices on its own while drafts are invalid. Call `drafts.clear()` when the document is replaced.

Give every field a stable `id_salt` (a parameter name, an emitter field name), so drafts follow their field when rows are added or removed.

The mouse wheel never steps a number; egui scrolls the panel instead.

## Property History
`PropertyHistory<Snapshot>` is property undo/redo over snapshots the tool chooses (for example JSON of parameters and emitter settings without scripts or texture bytes), separate from script text history, keeping 30 entries. It coalesces continuous gestures through a baseline, the snapshot at the last commit point:

```rust
let response = ScalarControl::new(name, &mut value).range(min, max).show(ui, &mut self.drafts);
if response.committed() {
    self.history.commit(self.property_snapshot());   // one entry per drag, entry, step, or reset
}
// Undo: restore what the history returns.
if let Some(snapshot) = self.history.undo(&self.property_snapshot()) {
    self.restore_properties(snapshot);
}
```

`reset(snapshot)` starts over (a new document, or a script edit); `rebase(snapshot)` moves the baseline after a structural edit that is not a property edit (adding or deleting a parameter or emitter). Undo and redo first check that the current snapshot is the entry's result and clear the history otherwise, so they never restore over unrecorded work.

## Panel Layout
`PanelLayout` places a left panel, a center, and a right panel inside any rectangle, so the same tool code works standalone and inside a Joy Editor tab area (`ToolHosting::Hosted { area }`). Widths clamp to the available space for display without changing the saved preference (the limits are left 160 to 600 points and at most 36% of the width, right 220 to 480, keeping 280 for the center), persist through `runtime::Storage` under the tool's key in its stable JSON shape, and resize by dragging a divider, by arrow keys on a focused divider (20 points), and back to the default on double-click. Panels collapse from `visibility_controls` (never all at once; the center only when `center_collapsible`), and `set_maximized` shows only the center (`MaximizeMode::Viewport`) or leaves the panes for the tool to hide its chrome (`MaximizeMode::Workspace`). Escape restores a maximized layout unless a text field has the keyboard. The toggles read left to right in either layout direction.

Draw panes first and dividers last so dividers win hit tests over pane content:

```rust
let panes = self.layout.panes(area);
if let Some(rect) = panes.left {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| self.compose_panel(ui));
}
// center and right panes
self.layout.dividers(ui, &panes);
```

## Script Editor
`ScriptEditor` is the code editor for FORM and JOYFX scripts on egui's `TextEdit`. It borrows the script text each frame and owns only presentation state:

```rust
// Once per document view:
let mut editor = ScriptEditor::new("spawn-script", ScriptSyntax::particle().with_parameters(names))
    .with_catalog(joy_engine::particles::particle_language_catalog());
// Each frame:
let response = editor.show(ui, &mut emitter.spawn);
if response.changed { /* schedule a live apply */ }
if response.run_requested { /* Ctrl/Cmd+Enter: apply now */ }
status = response.status_text();                     // "Ln 3, Col 7 · 12 lines"
editor.set_diagnostics(vec![ScriptDiagnostic::from(&error)]);
editor.go_to_line(error.line as usize);
```

- **Highlighting** comes from `ScriptSyntax`, built from the language catalogs: `ScriptSyntax::form()` (the FORM catalog plus attribute domains, strings, `param` declarations colored as parameters wherever they are used, components after `.`, and `@` names marked invalid), `ScriptSyntax::particle()`, `ScriptSyntax::json()`, `plain()`, or `from_catalog(&catalog)`; `with_parameters` colors extra names (an effect's parameters) as parameters. `script_language_tools` holds the tokenizer and the rest of the UI-free logic.
- **Completion** opens while typing an identifier and on Ctrl+Space (every symbol, catalog order), filters by prefix and then by subsequence, moves with the arrow keys, accepts with Enter, Tab, or a click, and closes with Escape. Functions insert `name()` with the cursor between the parentheses. `with_document_symbols(provider)` also completes and describes the names the script itself declares at the cursor, ranked before built-ins; FORM editors pass `form::form_document_symbols`, which reports variables and loop names in scope, parameters, functions with their signatures (including `ref` parameters), enums with their members, `use` modules, and declared attribute names.
- **Hover documentation** shows a symbol's signatures and documentation for keywords, functions, writable fields, and read-only inputs.
- **Diagnostics** (`ScriptDiagnostic`, from `form::Diagnostic` or `particles::ParticleScriptError` with one-based lines and character columns) are underlined at their word and marked in the gutter, with the message on hover; `go_to_line` moves the cursor there, focuses, and scrolls.
- **Editing** keeps egui's own undo history; `reset_history` starts a new one when the owner switches documents or emitters. Tab inserts two spaces; Ctrl/Cmd+Enter reports `run_requested` instead of inserting a line.
- **Read-only** views (`.read_only()` with `show_read_only`, for JSON) keep highlighting and selection with language tools off, on the surface fill.

The catalogs are the engine's: `form::form_language_catalog()` and `particles::particle_language_catalog()`, both returning `scripting::ScriptLanguageCatalog`.

## Viewport Input
`viewport_input(ui, rect, id)` reads camera gestures for a 3D viewport: a captured drag that orbits with the primary button and pans with Shift or any other button (the mode is fixed when the drag begins), wheel zoom in page pixels (positive toward the author), a primary click without modifiers or movement (for picking), and F, 0, and arrow shortcuts while the viewport has keyboard focus. Pressing the viewport focuses it and locks the arrow keys to it, so shortcuts never reach text fields. The caller maps `ViewportActions` onto its own camera.

## File Dialogs
`FileDialogs` opens and saves files without blocking a frame, natively and in browsers. Start one request from the UI phase that handled the author's click, then poll every frame:

```rust
// Open (text, or OpenRequest::binary for PNG textures), with a size limit:
self.dialogs.open(OpenRequest::text("Open effect", FileFilter::new("Particle effect", &["joyfx", "json"])).maximum_bytes(6_000_000))?;
// Save text (or SaveRequest::binary for GLB exports) under a suggested name:
self.dialogs.save(SaveRequest::text("Save effect", "ember-impact.joyfx", FileFilter::new("Particle effect", &["joyfx"]), text))?;
// Every frame:
match self.dialogs.poll() {
    Some(FileDialogEvent::Opened(file)) => self.open(&file.name, file.contents.text().unwrap_or_default()),
    Some(FileDialogEvent::Saved { location, .. }) => self.status = format!("Saved {location}"),
    Some(FileDialogEvent::Failed(message)) => self.error = message,
    Some(FileDialogEvent::Cancelled) | None => {}
}
```

- **Native** builds use the system dialog through [rfd](https://crates.io/crates/rfd) 0.17.2 (the version joy-apps uses), an engine dependency limited to the platform layer: the desktop portal on Linux (libdbus is loaded at run time, falling back to zenity, so builds need no system packages), the common dialogs on Windows, and panels on macOS. The dialog's asynchronous result is polled with a no-op waker each frame. Opening reads the chosen file (refusing files over the request's limit and non-UTF-8 text); saving writes the chosen path. On a Linux desktop with neither a portal nor zenity, rfd logs the problem and the request reports `Cancelled`.
- **Browsers** open through a file input clicked during the author's gesture and read with `Blob.text()` or `Blob.arrayBuffer()`; dismissing the picker reports `Cancelled` where the browser sends the input's `cancel` event. Saving downloads the file under its suggested name through `runtime::save_text_file` or the new `runtime::save_binary_file` (binary downloads use `model/gltf-binary` for `.glb` and `image/png` for `.png`).
- One request runs at a time (`FileDialogError::Busy`), and `deliver` queues an event directly, for hosts that supply files themselves and for tests.

## Headless UI Tests
`HeadlessShell::step_with_ui(game, raw_input)` runs a frame including the UI phase with synthetic egui input (pointer moves and buttons, keys, text) and paints the UI over the capture, so tool tests drive the real interface and save full screenshots without a display; `ui_context()` exposes the context for focus and memory checks. `step` still skips the UI. egui fades new areas in, so run a few idle frames (about 0.1 seconds) before comparing captured UI colors. Call `ui_context().enable_accesskit()` to receive the AccessKit tree in each frame's platform output and find widgets by accessible name and bounds. `tests/integration/development_kit.rs` and JOYFX EDITOR's `tests/integration/support.rs` are references.

For tool-wide shortcuts that should not fire while typing (Space, R), check `egui::Context::text_edit_focused()` rather than `egui_wants_keyboard_input()`, which is also true while a viewport, slider, or button has focus.
