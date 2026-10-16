# Joy Editor
Joy Editor is a Rust authoring application for local game projects. It keeps levels, reusable objects, FORM meshes, and particle effects in one document workspace, while source remains usable from VS Code and ordinary files.

## Run
From the engine root:

```bash
cargo run -p joy-editor -- --workspace=<hosting-workspace>
cargo test -p joy-editor
npm run build --workspace @joy-games/joy-editor
```

In JoyGames, `npm run dev:editor` or `npm run dev:joy-editor` runs the native editor. Project discovery uses `--workspace`, `JOY_EDITOR_WORKSPACE`, or the nearest `joy.joyconfig`; projects contain `project.joyproject` manifests. Open Folder accepts another compatible folder for the session.

The static browser build opens bundled examples and supports document downloads. It cannot discover or save project filesystem assets. Game previews and Play destinations are not implemented here; run a game through its own development command.

## Documents and Ownership
The workspace owns open files, saved revisions, dirty state, conflicts, and recovery. FORM LAB and PARTICLE LAB implement `joy_engine::development::AuthoringTool` and draw inside tabs using the same engine renderer and authoring kit as their standalone applications. Inactive tabs receive no frames. UI and document/model modules remain separate.

Native file operations in `server/project_files.rs` accept manifest-supported assets under `assets/`, reject symlinks and traversal, compare revisions before saving, and preserve replaced files in `.joy-editor-recovery/`. External edits refresh clean documents and require an explicit choice for dirty ones. Closing with unsaved work asks first. Session storage remembers project, open paths, active tab, and panel widths; it does not silently save source buffers.

Raw `.form` saves preserve source exactly; inspector parameter overrides remain temporary. `.formlab` retains overrides, materials, imported meshes, and variants. `.joyfx`, `.joylevel`, and `.joyobject` retain their document formats. Unsupported or malformed structured assets can be repaired in source view. Save and baked export are distinct operations.

## Level Authoring
The level tool has an outliner, schema-driven property inspector, Advanced Source, orbit/pan/zoom, selection, a move gizmo with axis/plane handles, grid snapping, and undo/redo. Entity transforms use world units and display degrees while storing radians. Component schemas supply defaults, bounds, units, conditions, and asset choices; custom data keeps generic controls.

Reusable objects let an instance follow source components while retaining local identity/transforms and explicit overrides. Reset Overrides and Detach make that relationship visible. FORM meshes, JOYFX effects including textures, lights, and primitives draw through `AssetScene` and `LevelScene`; physics remains separately authored. Scene nodes do not imply object-document nesting or automatic collision cooking.

The shared grid is an editor aid, independent of authored ground, export, and simulation. Preview lighting, camera, panel visibility, and maximization remain editing state. Numeric drafts stay separate from accepted values and must be repaired before operations that replace authored state.

## Modules and Validation
`site/src/workspace/` owns workspace models/UI and tool hosting; `site/src/level/` owns level authoring; `server/` owns native file access; `tools/` contains the labs. Shared controls and script language presentation live in the engine's [authoring kit](../docs/rust-authoring.md).

Rust tests under `test/rust/` cover documents, file boundaries, conflicts/recovery, level operations, and headless UI interactions. The canonical capture script checks browser canvas startup and screenshots. Use interactive native checks for file dialogs and project editing; browser captures cannot verify native filesystem workflows.

## Production Availability
The hosting workspace's `joy.joyconfig` controls `editors.joyEditor`, `editors.formLab`, and `editors.particleLab`. Joy Editor defaults off; both labs default on. Enabling the editor also enables its hosted labs. Production browser publication remains static and does not add a filesystem service. Development commands remain available regardless of these flags.
