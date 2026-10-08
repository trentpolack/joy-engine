# Joy Editor
Joy Editor opens a game project from the local JoyGames checkout and keeps its levels, FORM meshes and particle assets in one workspace. VS Code continues to own game logic; both applications work on the same content files.

## Package and hosting workspace
Joy Editor lives in the Joy Engine repository as its own package with its own dependencies (see the JoyGames [repository split](https://github.com/trentpolack/JoyGames/blob/develop/docs/development/repository-split.md) notes). FORM LAB and PARTICLE LAB live under `tools/`. It depends only on `joy-engine` (`file:..`) and exports the pieces other packages consume:

| Import | Purpose |
| --- | --- |
| `@joy-games/joy-editor` | Shared authoring UI: CodeMirror script editor and preview fullscreen, used by the workspace and its labs. |
| `@joy-games/joy-editor/desktop` | Electron host for bundled labs (`launchDesktopApplication`). |
| `@joy-games/joy-editor/server` | Vite-only project file service (`createWorkspacePlugin`, `WorkspaceAssetUpdates`). |
| `@joy-games/joy-editor/tooling/policy`, `/tooling/vite` | `joy.joyconfig` production-availability policy for site builds and editor Vite configs. |

The editor authors content that belongs to a *hosting workspace*: the game repository whose development server installs the file service, mounts the editor at `/editor/` and its games at `/games/<id>/`. JoyGames is that workspace today (`scripts/development/vite-site.config.mjs`), with games under `projects/`. A different workspace passes `{projectsDirectory}` to `WorkspaceAssetUpdates`; the desktop host reads `JOY_EDITOR_WORKSPACE` and `JOY_EDITOR_WORKSPACE_VITE_CONFIG` (both default to the JoyGames layout). The production policy finds `joy.joyconfig` by walking up from the Vite root.

## Run
From the repository root:
```bash
npm ci
npm run dev:editor
```

`dev:editor` opens the full workspace at `/editor/` in your browser. Choose a project from the **PROJECT** selector; the local service discovers project manifests and their assets without requiring a browser folder API. `npm run dev:joy-editor` is an alias for the same workflow.

`npm run desktop:editor` (or `npm run desktop:joy-editor`) opens that same workspace in Electron and adds a native **Open Folder** picker for any compatible folder with a `project.joyproject` manifest. Both modes start the project and tool development servers from source with Vite hot reload. Neither mode requires an editor build or a game build. External projects use their manifest-owned asset capabilities; their production preview is not mounted into the repository development server.

The workspace, level editor, shared browser controls, and FORM/PARTICLE LAB implementations use native TypeScript contracts. Vite compiles their browser and worker entries; strict no-emit checking covers each workspace. Node file-service middleware, Vite configuration, tests and Electron launchers remain JavaScript.

Shared editor chrome and desktop-launch behavior live at this level. Tool-specific source, assets, tests, and build configuration live in [`tools/`](tools/).

## Production availability
[`joy.joyconfig`](https://github.com/trentpolack/JoyGames/blob/develop/joy.joyconfig) controls which editors ship in production. Development commands always enable all three editors. Restart development servers after changing the file; rebuild before publishing production changes.

```json
{
  "editors": {
    "joyEditor": { "productionEnabled": false },
    "formLab": { "productionEnabled": true },
    "particleLab": { "productionEnabled": true }
  }
}
```

Joy Editor is excluded by default; FORM LAB and PARTICLE LAB remain public. Each lab can be disabled independently. Enabling Joy Editor overrides both lab flags because its document workspace requires them. Missing settings use these defaults; invalid flags stop the build.

`npm run build` and `npm run build:release` skip disabled editor workspaces, remove their old workspace build output, and omit their routes, cards and tool-switcher links from the exported site. Release captures skip disabled editors too. Direct editor build commands, including `npm run build:editor`, reject disabled production targets; their Vite preview servers also reject access to stale output. The enabled workspace is exported at `/editor/`, and labs keep `/joy-editor/tools/<tool>/`.

The Joy Editor flag publishes its browser UI only. The project discovery and local file-editing API remains a development service and is not deployed by a static build; enabling the UI does not make local filesystem authoring available on public hosting. Use `npm run dev:editor` or the desktop host for that workflow. Game editor bridges and development menus retain their development-only guards.

## Project workflow
Choose a project, search its content, and open a `.joylevel`, `.joyobject`, `.form`, `.formlab`, or `.joyfx` asset. Create assets by choosing their type and naming them in the content browser, or import an existing supported text asset without overwriting project content. Tabs preserve editing state and preview cameras while open; inactive previews suspend simulation, so the workspace does not impose an arbitrary open-tab limit.

Save File (Ctrl/Command+S) writes directly to the existing project file. External edits refresh clean tabs. If the document has unsaved changes, the workspace offers Use Disk Version or Keep My Edits; keeping edits still requires an explicit save. Unsaved files prompt before closing or changing projects. The next session restores project, open paths, active tab, and preview width, but not unsaved source buffers. Invalid structured assets open a source-repair view instead of displaying an editable stale example.

Raw `.form` files retain their exact script format. Parameter sliders are temporary preview overrides; edit script defaults to persist them. Document-only materials and variants are available in `.formlab` documents. Save does not export or convert the document. Standalone FORM and PARTICLE tools remain available at `/joy-editor/tools/<tool>/` with their existing export workflow.

The local service limits edits to manifest-supported assets beneath the selected project's `assets/` directory and rejects symlinks and path traversal. Saves compare content revisions and serialize writes. Replaced source files are retained in an ignored `.joy-editor-recovery/` folder beside the asset, including writes that overlap another editor's save; recovery management remains hidden from the primary authoring flow for now. **New** defaults to an editable FORM LAB document and also creates starter levels, reusable objects, or particle effects without overwriting existing files. FORM LAB keeps script source, parameter overrides, materials and variants together. **Import** copies existing `.form`, `.formlab`, `.joyfx`, `.joylevel` and `.joyobject` files while retaining their format. Raw `.form` files remain procedural source; saving them does not persist slider overrides. Workspace Import does not accept GLB; FORM mesh parameters have a separate geometry-only GLB import that does not preserve a full scene or its materials.

Single-click or Enter opens an asset. Rename is available through the row actions menu, secondary click, or F2. Rename and delete leave closed files closed; the trash action confirms deletion. Save or discard edits before renaming or deleting an open document. The search-row Filter icon combines checked asset types; All types clears restrictions. Refresh rescans the list immediately; Cmd/Ctrl+P reveals and focuses search, including from a tool. **Documentation** opens the workspace guide beside Project Details.

## Level authoring
Choose **VEHICLE PLAYGROUND** and open `assets/playground.joylevel` for an authored physics playground. The level tool provides a 3D viewport, entity outliner, select/move modes, grid snapping, orbit/pan/zoom and focus. Add boxes or spheres, duplicate/delete entities, and use undo/redo to restore authored edits. Transform fields use meters and degrees; rotations are stored as radians. Typed component controls expose renderer, collider, vehicle and custom gameplay properties. Nested records, arrays, booleans and numeric values can be edited without JSON. FORM/JOYFX asset selectors expose declared parameter defaults automatically. Invalid fields block saving or changing selection; Advanced Source remains available for unusual data and repair.


### Editor grids
Level, FORM LAB and PARTICLE LAB use the same bounded, unlit line-grid aid. Minor spacing follows 1/2/5 powers of ten as the camera zooms; major lines mark five divisions. Orange-brown lines mark X and blue lines mark Z. Lines retain readable widths in CSS pixels, test against authored scene depth and do not write depth or become opaque ground surfaces. The finite grid follows camera pan; axes remain at world zero. Level units are meters; FORM and JOYFX retain their existing world units without conversion. Level uses Y=0, FORM places its grid just below the accepted geometry, and Particle Lab uses its existing reference height below the effect. Each preview has an independent grid visibility toggle; this is editor state and does not affect documents, exports, snapping, particles or lighting.

Particle Lab's existing **Light receiver (editor only)** is a distinct optional lit surface, off by default. The terrain example's LOD/topology visualization and authored game ground remain unchanged.

### Reusable objects
Compose a primitive or FORM mesh, attach a JOYFX effect, add physics or custom properties, then choose **Save As Object**. The resulting `.joyobject` is a reusable single-entity definition; the selected entity becomes a linked instance. Choose it in the toolbar and use **+ Instance**, or duplicate an existing instance. Names and world transforms remain local. Component edits become explicit instance overrides; unchanged fields follow the saved source. **Reset Overrides**, **Detach**, and their undo/redo actions make the relationship explicit.

**Edit Source** opens the referenced object, FORM or JOYFX in its appropriate tool. The workspace's return button restores the owning document. Save source changes to refresh dependent previews. Arrays override as a whole; removing an inherited nested property requires editing the source or detaching. Removing a whole component is supported. Object definitions do not nest and do not introduce parent-child transforms.

`AssetScene` is shared by the editor and Vehicle Playground. FORM meshes and colored particle effects compose on the same object; each placed effect owns its simulation. Textured and flipbook effects currently require PARTICLE LAB and produce an explicit content error in the shared scene. Invalid assets retain the last valid visual result. Physics shapes remain separately authored; attaching a mesh does not cook a collider.

The viewport uses Joy Engine's retained scene renderer. `render.size` supplies full primitive dimensions; entity scale multiplies them. Physics colliders use the corresponding local half-extents or sphere radius. The initial tool supports world-space primitive placement; it does not yet provide prefab inheritance, parent-child transforms, solid wheel colliders or mesh collision cooking.

**Run Project** starts the playable example. Saved level edits are staged; **Regenerate World** explicitly starts a new session from them. Stop and Run also provide a fresh session. Simulation transforms never write into the authoring document. The example exposes keyboard/touch driving, player/AI possession switching, reset and pause; see [Vehicle Playground](https://github.com/trentpolack/JoyGames/blob/develop/projects/vehicle-playground/README.md).

## God Game preview
The workspace opens with game preview closed. **Play** uses the selected destination: Docked, Joy Editor Tab (initial default), Browser Tab or Standalone Window. Selecting a destination changes the preference without launching. Repeated Play focuses the existing session; **Restart** applies the selected destination with a fresh simulation. Stop or closing Game Preview returns to authoring without closing documents.

Standalone Window offers 720p, 1080p, Fit to Screen and custom CSS content sizes. Browser popup placement and sizing depend on the browser/display. The desktop host owns native preview windows; Browser Tab there is explicitly independent, with connected controls unavailable. Close the external tab before using **Finish External Preview**. Managed destinations expose Pause only after a runtime acknowledgement and disable connected controls when the host disconnects.

Assets and tool panels support pointer/arrow-key resizing with remembered widths. The Content Browser can collapse. FORM and particle Maximize retain all editing panels and temporarily hide the Content Browser and docked game pane; Restore or switching documents returns the surrounding layout without changing saved widths. Level retains viewport-only Maximize. Camera controls share primary drag to orbit, Shift-primary/right/middle drag to pan, wheel zoom, and canvas-focused F to frame / 0 to reset. Scrolling over numeric fields scrolls the property panel without stepping values.

Every playable project supplies a `project.joyproject` manifest and manifest-owned capability settings, so its supported FORM, particle, or level documents are explicit and portable. Game previews report their pause state to the workspace. GOD GAME applies supported content transactionally, VEHICLE PLAYGROUND stages level changes for regeneration, and the remaining games reload their preview after a successful asset save so edits are visible without closing the workspace.

God Game supplies the state-preserving project-specific content adapter:
- Saved villager, home, tree, and shrine FORM meshes replace visual templates without resetting the simulation.
- Saved rain, fire, and blessing effects are checked for runtime execution and renderer support before replacing live effects. Cosmetic effects restart; the simulation is retained. Later execution failures roll back to the retained baseline effect.
- Saved `world.form` changes are validated and staged. Regenerate World explicitly resets using the current seed.
- Invalid content retains the previous game result and reports a content error. The source remains saved so it can be repaired.

The current God Game mesh renderer uses vertex colors; FORM PBR materials and textured/flipbook particles are not supported by this adapter. Game-specific mappings stay in the project; the file service and document host do not interpret gameplay concepts.

## Architecture
- `workspace/`: document state, project UI, tool protocol, responsive shared layout.
- `server/`: local Vite middleware and project file operations; absent from production hosting.
- `desktop/workspace.cjs`: local Electron host for the same Vite workspace, file API, tools, and game previews used in the browser.
- `tools/`: existing specialist editors, mounted as same-origin document frames in hosted mode. Each frame owns its editor, compiler and GPU resources and disposes them when closed.
- `projects/god-game/src/editor-preview.js`: development-only runtime adapter.

### Project manifests
The workspace reads `project.joyproject` from the selected project root. These files contain standard JSON; the `.json` extension is unnecessary. Existing manifests named `joy.project.json` should be renamed to `project.joyproject`.

Manifests may include a top-level `$schema` reference to [the shared project schema](../schemas/joyproject.schema.json). God Game uses `"$schema": "../../joy-engine/schemas/joyproject.schema.json"`; relative references resolve from the manifest's directory. The schema describes preview paths, declarative capability settings, asset extensions and feature flags. Runtime validation also checks constraints such as unique capability IDs, and preserves schema metadata without loading it.

The repository's VS Code settings associate the JSON-backed `*.formlab`, `*.joyfx`, `*.joylevel`, and `*.joyproject` formats with JSON syntax highlighting. Project manifests additionally use the shared schema for completion and validation. Other editors, or VS Code opened at a project subfolder, may need equivalent language associations; a manifest's `$schema` identifies its schema after the editor recognizes the document as JSON. Raw `*.form` files contain FormScript rather than JSON, so they require dedicated FormScript language support for syntax-aware highlighting.

## Validation
```bash
npm run check
npm run test:editor
npm run build:editor
npm run capture:editor
npm run capture:level-editor
npm run capture:editor-usability
CAPTURE_CHANNEL=chrome node joy-engine/editor/test/scene-browser.mjs
```
`capture:editor` changes and restores three God Game fixture assets in the current checkout. Run it in an isolated checkout. It tests direct saves, live mesh replacement, persistent tabs, conflicts, external reload, staged regeneration, malformed particle repair, and active-tab restoration. It writes screenshots and a gallery to ignored `artifacts/joy-editor/`. Browser executable and single-process overrides use the existing `CAPTURE_EXECUTABLE` and `CAPTURE_SINGLE_PROCESS` environment variables.

`scene-browser.mjs` verifies shader compilation and transparent scene pixels on WebGPU and WebGL, with post-processing enabled and disabled. It starts a temporary development server, requires both GPU backends, and writes a per-run gallery to ignored `artifacts/captures/`. Use `CAPTURE_CHANNEL` or `CAPTURE_EXECUTABLE` to select an installed Chromium browser with WebGPU support.

### Property editing and preview state
FORM and particle property history is separate from script undo. Type directly; Shift + arrows adjusts continuous values finely, Alt + arrows coarsely. Integer counts retain whole-number steps. A completed slider drag adds one property undo entry; Escape, pointer cancellation or focus loss restores its starting value. Invalid drafts remain visible and block explicit capture until repaired. Disk polling uses passive captures and does not commit focused drafts or interrupt gestures.

Raw FORM parameter overrides remain temporary; Save writes script source. FORM LAB persists overrides and materials. Geometry exports use the last accepted preview, with failures retaining that preview. Neutral inspection lighting/background is editor-only. Particle effect parameters belong to the whole asset, while the selected emitter owns its settings/scripts. Replay Full retains effect-drain looping; 2s, 5s and custom windows replay at the chosen endpoint without changing exported data. The light receiver is an optional editor-only audition surface.

Particle **+ Add** opens a one-time insertion chooser for Scratch, Soft Spray, Spark Burst, Ring Flash and Smoke. Each creates one detached emitter without replacing the effect or importing shared parameters. Cancel leaves the document unchanged. Property undo remains separate from scripts and emitter topology changes. Generated JSON is available through normal asset saving/export rather than a redundant preview tab.

Shared appearance roles live in `site/css/theme.css`; tool palettes and script highlighting consume those roles. Fullscreen is hidden in the labs while whole-workspace Maximize provides a consistent editing flow.
