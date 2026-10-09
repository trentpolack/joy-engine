# Changelog

This changelog summarizes meaningful work on Joy Engine, Joy Editor, FORM LAB, PARTICLE LAB and the engine examples. Entries are grouped by day, newest first, using America/Detroit dates. Work before 10-08-2026 happened inside [JoyGames](https://github.com/trentpolack/JoyGames); its [CHANGELOG](https://github.com/trentpolack/JoyGames/blob/develop/CHANGELOG.md) is the historical record for that period.

## 10-09-2026
### FORM building asset example
- Added a FORM-authored building shell with three seeded presets, editable runtime parameter snapshots, baked mesh/GLB assets, and a material-group presentation adapter. Documented compiler boundaries, material rendering limits, focused tests, and local browser evidence.

## 10-08-2026
### Runtime diagnostics
- Clarified the live development launcher as `STATS | <fps>` with stable numeric spacing so cadence changes remain easy to scan.
- Gave the shared runtime dock wider buttons and more horizontal padding so the Stats readout has room; compact layouts hide its divider alongside the FPS value.

### Joy Editor development server
- Made Joy Editor and its labs discover their active npm workspace for Vite file serving, restoring hoisted dependencies when the editor is hosted by a larger repository.

### Repository
- Clarified the agent workflow for inclusion in a larger workspace: complete file edits and validation together, leave submodule Git state to the user, and keep agent instructions independent of private hosting repositories.
- Established this repository from the Joy Engine and Joy Editor history in JoyGames (`joy-engine/` became the root, `joy-editor/` became `editor/`, the engine guide moved to `docs/`). Joy Engine 0.12.0 and Joy Editor 0.6.0 are the starting versions.
- Made the root package the npm workspace root for Joy Editor, FORM LAB, PARTICLE LAB and the examples, with its own lockfile, `.gitignore`, `AGENTS.md` house rules and coding standards. JoyGames consumes the repository as a git submodule at `joy-engine/`.

## 10-07-2026
### Engine and editor repository split preparation
- Made Joy Engine (0.12.0) a self-contained package with its own TypeScript base, unit suite, `?raw` Node loader, boundary check, API generator, guide (`joy-engine/docs`) and `joy-check-javascript` bin; it now installs, checks and tests from an isolated copy.
- Moved the CodeMirror script editor, preview fullscreen and Electron lab host from the engine into Joy Editor (0.6.0), along with the editor production-availability policy. Joy Editor now locates its hosting workspace and projects directory instead of assuming the JoyGames layout.
- Exported the config tuning, post-processing persistence and favicon Vite plugins from `joy-engine/development/vite`, sorted tests by owning package, fixed `npm run docs:api`, pinned the root engine dependency to `file:./joy-engine`, moved wrangler to devDependencies, and removed the legacy editor desktop stack.
- Documented the extraction and cut-over steps in [repository split](docs/development/repository-split.md).

### Joy Engine world objects and ECS
- Established `Entity` as the base world-object contract and added registries for data-driven runtime components, deterministic component queries and world-owned systems with explicit rollback and cleanup.
- Documented the hybrid actor/ECS migration path so existing entity subclasses and portable level component data can coexist with composable behavior.

### Joy Editor level viewport
- Restored the level viewport's scene-effects render error boundary, adaptive grid ownership, and per-frame scene submission independently of grid-spacing display updates.

## 10-02-2026
### Shared development widgets and monitoring
- Docked runtime STATS with CONFIG and JOY-ENGINE in one responsive lower-right row, coordinated panel expansion and Escape focus, and shared a development-only layout buffer with gameplay HUDs and touch controls.
- Unified runtime stats, configuration/tuning, postprocessing and example debug/help panels with near-black surfaces, Joy orange outlines and consistent hover/focus states while preserving gameplay palettes and controls.
- Added expandable, cadence-limited luma allocation/resource details with explicit shared-device scope and unavailable states; frame cadence remains distinct from CPU/GPU execution time and resets after background/resume.
- Made Particle Lab per-emitter profiling opt-in, disabled monitoring clocks and UI work when monitoring is off, exposed CPU submission/sort and vertex payload details, and replaced the speculative draw estimate with scoped scene geometry batch counts.

### Joy Editor content and lab workflows
- Shared cached, unlit line grids across Level, FORM and Particle previews with adaptive world-unit spacing, major divisions and distinct axes; added Level grid visibility and kept Particle Lab's light receiver separate and default off.
- Unified semantic theme roles and script surfaces, improved native numeric arrows, and simplified the Content Browser with multiselect filters, keyboard-accessible actions and explicit double-click opening.
- Added cancellable, self-contained emitter insertion presets, removed the generated JSON preview UI, and made lab maximization retain editing panels while restoring surrounding workspace state.
- Restored single-click file opening, clarified New FORM LAB documents versus imported source, and polished content actions, project help, preview state and FORM controls; PLY is hidden from the export menu.

### Joy Editor preview lifecycle
- Cancelled queued and in-flight automatic particle applications when Live is disabled, preserving explicit Apply and pending edits; added deterministic browser regression coverage.

### Engine and editor TypeScript migration
Migrated Joy Engine runtime implementations and public contracts to native TypeScript across rendering, physics, environment simulation, gameplay, FORM compilation, particles, input, configuration and editor services. Converted Joy Editor workspace and level authoring plus FORM/PARTICLE LAB browser modules and compiler workers. Updated package exports, all consumers, source links and API generation while preserving runtime behavior and retaining JavaScript for Node tooling and Electron launchers.

### Joy Engine terrain
Aligned the terrain example UI with the Joy orange/blue palette, square panels and controls, branded typography, and clear interaction states.
Moved the terrain demonstration into `joy-engine/examples/terrain/` with explicit command-line launch/build/capture scripts, outside the main site and automatic releases. Added camera-relative fly movement with cursor-hidden right-button look, left-drag orbit, scroll zoom and Command/Alt panning.
Documented terrain resource ownership, CPU/GPU LOD selection, shared-edge morphing, buffer layouts and matching shader logic without changing rendering behavior.

## 10-01-2026
### Joy Engine / Terrain Lab
Added an isolated Dragon Pit landscape experiment with offline EXR-to-uint16 export, resident error hierarchy, continuous shared-edge CLOD, indexed grids, WebGPU compute/indirect selection and a WebGL 2 CPU fallback. The demo documents explicit scales, stable gameplay sampling and renderer ownership; vehicle physics, road authoring and atmosphere integration remain separate.

### Joy Engine environment systems
- Added a deterministic environment simulation for solar time, sun/moon atmosphere lighting, skylight and gusting wind, with validated atmosphere, fog, cloud, light-shaft and ambient-occlusion policy.
- Added a portable environment uniform layout and standard PBR lighting adapter so future WebGPU and WebGL 2 passes share one synchronized world-state contract.
- Added the first portable atmosphere renderer for WebGPU and WebGL 2, including approximate Rayleigh/Mie scattering, sun and moon discs, wind-driven procedural clouds, and a primary-light scattering halo.

### Joy Editor workflow v2
- Corrected particle browser state/palette checks and static sprite FPS validation while retaining invalid-draft, replay-drain and preview-lighting assertions.
- Separated bounded FORM compiler startup from its script execution budget, retaining accepted previews on failure.
- Clarified document/preview/override persistence, compacted FORM metadata and emitter ownership, and added view-only scene filtering plus optional particle replay windows.
- Added bounded property undo, cancellable gestures, fine/coarse numeric adjustment and repairable inline drafts; passive disk polling preserves in-progress edits.
- Unified trackpad camera gestures, viewport focus shortcuts and numeric-field scrolling across Level, FORM and particles.
- Added explicit Play destinations, with game preview closed at startup, plus compact workspace chrome, collapsible Assets and remembered panel sizing/maximize controls.

### Joy Engine VS Code extension
- Added an installable Joy Engine extension with JSON recognition and offline schemas for custom Joy formats, plus FormScript highlighting and editing support; uses the Joy Engine image as its extension icon.

### Portable editor assets
- Replaced per-project FORM, particle, and level adapter modules with declarative manifest settings; added shared schemas and VS Code associations for Joy asset formats.
- Moved every game's post-processing profile into a main `.joylevel`, allowing procedural games to use lightweight levels for shared scene settings without serializing generated entities.
- Renamed the workspace configuration to `joy.joyconfig`, with a dedicated `.joyconfig` schema and VS Code JSON association.

### Joy Editor content workflow
- Reworked asset creation into an inline type-and-name flow, added supported-format imports, moved find and file operations into the content browser, hid recovery management, and removed the six-tab workspace limit.

### Editor production availability
- Added per-editor production flags in `joy.config.json`: Joy Editor defaults to development-only, while FORM LAB and PARTICLE LAB remain enabled independently. Enabling Joy Editor includes both labs automatically.
- Applied the flags to production builds, site exports, editor previews, tool navigation and release captures, while retaining all editors in development.

## 09-30-2026
### Editor usability v1
- Added reusable `.joyobject` definitions, linked instances, explicit overrides, typed component authoring and undo/redo, with FORM/JOYFX asset attachment and source-editor navigation.
- Shared procedural asset presentation between the level editor and Vehicle Playground, with atomic content replacement and per-instance effect ownership.
- Added an owned multi-device input manager and migrated Vehicle Playground and Magic Colors; added shared live-object tuning, grouped edits, redo and presets.
- Unified workspace and hosted editor chrome around the intentional orange/blue palette, square controls and clearer editable surfaces.

### Rendering architecture
- Fixed FORM LAB preview startup on WebGPU and WebGL 2 by removing a duplicate PBR shader constant and completing the preview shaders' post-processing interface.
- Made the engine-owned `SceneRenderer` and postprocessor the default path for standard game and editor viewports, while retaining low-level project shaders only for behavior-specific effects.
- Migrated GOD GAME off its duplicate scene shaders and onto the shared scene renderer, enabled postprocessing for the FORM LAB viewport, and renamed the remaining SPACE SCUFFLES and BLAST GARDEN shader programs after their project-specific flash behavior.

### Editor integration
- Added project manifests and project-owned FORM, particle, or level capability adapters to every playable game, plus a shared preview connection for pause control and save-triggered content refreshes.
- Associated JSON-backed FORM LAB, JOYFX, level, and project files with JSON in the repository's VS Code settings.
- Fixed level-editor shader compilation and transparent rendering when post-processing is disabled; scene templates now emit fragment color without adding unused uniform fields.

### Joy Editor workspace
- Integrated project-owned manifests into workspace discovery, added desktop opening for compatible external folders, and added manifest-supported asset creation, rename, deletion, dependency navigation, and recovery-copy retention controls.
- Standardized BLAST GARDEN capture browser overrides with the repository capture environment contract and retired completed editor follow-up documents.

### Joy Engine 0.9.0
- Added data-driven entities and type registries, explicit world/resource ownership, possessable characters, player/AI controllers and safe possession transfer.
- Added versioned level documents, transactional authoring history, primitive scene presentation and shared level runtime instantiation.
- Added engine-owned cannon-es physics with configurable rigid bodies, material properties, queries, bounded fixed stepping and raycast vehicles with data-defined wheels, suspension, steering, drive and brakes.

### Joy Editor 0.2.0
- Made the JOY-ENGINE menu available in every game’s development session, with shared orange styling, hard corners and project-owned post-processing profiles.
- Added level documents to the shared workspace with exclusive new-file creation, direct saves, revision conflicts and malformed-source repair.
- Added a 3D level tool with an entity outliner, selection/placement, snapping, transform and component editing, duplication/deletion and undo/redo.
- Workspace saves stage authored content without Vite reloading a running game; restarting a preview still loads the latest saved modules.

## 09-29-2026
### Joy Engine 0.8.0
- Added immutable mesh assets, FORM asset conversion, persistent mesh instances and keyed scene collections with explicit ownership and cleanup.
- Added engine-owned frame preparation, cached instance transforms, frustum culling, shared scene shaders, lighting uniforms, transparency ordering, canvas resizing and camera projection/picking.
- Migrated LUNA and MAGIC COLORS to the shared scene layer while preserving gameplay and project-owned effect lifetimes. Retained the existing vertex-color triangle backend; hardware instancing and per-instance PBR rendering are not part of this pass.
- Removed the unused standalone particle-system APIs in favor of the asset-driven `ParticleEffect` runtime while retaining the motion helpers used by SPACE SCUFFLES.

## 09-28-2026
### FORM LAB scripting
- Retired `@attribute` syntax with migration diagnostics and require `wrangle <domain> in <geometry> as <binding>`; updated language examples, reference material, editor highlighting, and compiler regression coverage.

### Authoring lab examples
- Added Harvest grove, Patrol circuit, and Tactical terraces FORM studies with logical entity outputs, gameplay attributes, named user-data records, and plain-language comments explaining how games can use the data.
- Migrated every bundled FORM LAB study to current object-first and explicit-attribute scripting where applicable, and added inspectable user-data records for generated content.
- Migrated every bundled PARTICLE LAB effect and the new-emitter starter to aggregate vector and color scripting, with regression checks that compile all shipped examples.

## 09-25-2026
### FORM LAB scripting
- Require explicit attribute creation before wrangle reads or assignments, add domain-specific `addAttribute` declarations, and require `in` in named and implicit wrangles; migrated bundled examples and reference material.
- Added object-first box and sphere geometry, explicit translate/rotate/scale/color operations, and named wrangle element bindings while retaining compatibility with existing immediate-mode studies.
- Updated the terrain and reusable geometry recipes to avoid implicit color state and `@attribute` iteration syntax.

### Joy Editor project workspace
- Added Stop for running or paused project previews, preserving open asset tabs and unsaved edits.
- Restored the full project workspace entry point, including asset browsing, persistent FORM and particle editors, direct saves, conflict handling, and the live game preview.
- Browser and desktop editor commands now share the source-based development host; neither requires a production build or browser folder-selection support.
- Retained the manifest-adapter prototype as unfinished work without replacing the existing authoring workflow or advertising incomplete desktop packaging.

## 09-24-2026
### Joy Editor workspace
- Added a local project workspace with searchable content, persistent FORM/particle document tabs, direct file saves, external-change detection, conflict resolution, source repair, and recoverable replaced-file backups.
- Added `dev:editor` and an Electron workspace entry, retaining the standalone tools and their file formats.
- Connected God Game mesh/effect iteration to the running preview while staging world changes behind explicit regeneration; failed content retains or restores a working runtime result.
- Completed synchronous Node raw-loader registration so engine and editor tests can import authored FORM, effect, and shader assets.
- Removed the FORM LAB and PARTICLE LAB header marks and sourced each tool's version badge from its package metadata.
- Added a shared in-tool workspace switcher with direct tool navigation, current asset actions, keyboard shortcuts, and a route back to the experiment directory while retaining focused full-page tools.
- Renamed the shared editor root to `joy-editor/`, including tool URLs, npm workspace paths, and build/test tooling.
- Updated version list/check/bump commands to follow npm workspace paths, including editor tools, while ignoring leftover build folders.
- Serve FORM LAB and PARTICLE LAB at `/joy-editor/tools/<tool>/` in development and production, including landing-page links.
- Moved FORM LAB and PARTICLE LAB into `joy-editor/tools/`, with shared Joy Editor chrome and desktop-launch behavior at the editor root.
- Updated workspace discovery, site assembly, development routing, validation paths, and documentation for the editor tool layout.

## 2026-09-23
### FORM user data and lab previews
- Added scoped `with userData(record) { ... }` groups with nested restoration and local bindings; GOD GAME uses named multiline records beside each placement group.
- Added bounded nested user-data records to FORM points and editable geometry, retained through copies, runtime outputs, inspection, and JSON export; GOD GAME layouts now identify structures through data instead of color.
- Added shared preview fullscreen controls to FORM LAB and PARTICLE LAB, with Escape exit and a window-filling fallback.
- Hid FORM LAB workspace profile controls while retaining saved layouts and pane resizing.

## 2026-09-22
### FORM reusable workflows
- Added project-owned named parameter variants, including resource overrides, with save/apply/rename/delete controls and backward-compatible document persistence.
- Added runtime profile revolution, path tube sweeps, smooth-normal rebuilding, vessel/path workshops, and appendable starter recipes.

### FORM workspace
- Added persistent adjustable workspaces, keyboard pane resizing, inspector sections, parameter search, direct recipe access, and navigation to script errors.

### FORM procedural authoring
- Added reusable geometry, point/face/corner attributes, wrangle blocks, topology editing, vector math and noise, weighted scatter, retained instance collections, and named runtime outputs.
- Added linked result/attribute inspection, scalar/color visualization, a terrain workshop example, and searchable appendable recipes while preserving the FORM LAB workbench.
- Corrected WebGL PBR index offsets so assemblies with multiple materials render completely; documented the next layout and procedural workflow capabilities.

## 2026-09-18
### FORM runtime parameters
- FORM scripts can expose mesh/material inputs, construct reusable primitive resources, transform/deform geometry, and emit instances alongside existing scalar/vector/color parameters.
- Added the renderer-independent `joy-engine/form` runtime API and a parametric bridge example. FORM LAB retains its current editor layout with resource overrides and GLB geometry input.
- Logical instances currently bake into preview/export geometry; GLB inputs use the existing single-primitive geometry loader.

## 2026-09-16
### Platform, rendering, and validation
- Promoted the game and app experiment directory to the landing page's lead panel; Recent Updates now shows only the latest change with the viewer's local date and time.
- Reorganized Joy Engine rendering modules by camera, geometry, GPU, PBR, renderer, scene, and post-processing responsibilities; upgraded every luma.gl package to 9.4.1.
- Corrected the development renderer controls, including select inputs, conditional AgX settings, independent saturation and vibrance, multiplier-based brightness, Gaussian/scatter sliders, keyboard pause, Escape handling, and coordinated TUNE/JOY-ENGINE panels.
- Simplified routine validation by separating fast static checks from focused tests and removing specialized browser probes superseded by project capture workflows.
- Standardized particle asset filenames on `.joyfx`, retaining JSON content and shared raw-import support for builds and tests.
- Integrated loaders.gl GLB/glTF decoding, Shadertools assembly, and luma.gl image-effect pipelines with HDR multiscale bloom, anamorphic/lens shaping, animated grain, and portable WebGL 2 fallbacks. Added optional WebGPU SSAO, TAA, and motion blur with explicit frame inputs and owned history.
- Kept live POST FX saves from restarting Vite and pre-optimized lazy engine dependencies to avoid first-use game reloads.
- Restored SPACE SCUFFLES rendering on WebGPU and WebGL 2 by sizing scene uniform buffers from the complete post-processing data.
- Polished FORM LAB's material editor and preview controls, added image-backed PBR material channels and script UV/normal data, and retained flat face normals by default.

### FORM LAB materials and Joy Engine PBR
- Added reusable GPU metallic/roughness material rendering on WebGPU and WebGL 2, with six texture slots, owned resource cleanup, and camera redraws that retain uploaded geometry.
- Restored flat normals by default, added script-authored normals, and preserved hard edges and explicit normals in GLB export.
- Corrected and softened slider track borders, improved number fields, aligned parameter styling, added material deletion, thumbnails and color swatches, and removed the Alpha Cutoff control.

### FORM LAB v0.5.0
- Added GLB export with flat normals, vertex RGBA, material assignments, and authored point primitives.
- Added named material editing, script selection, document persistence, and constant `gltf_pbr` MaterialX import/export with explicit unsupported-feature errors.

### Development editor launchers
- Matched TUNE to the POST FX button styling and placed the launchers side by side, with TUNE on the left and POST FX on the right in both games.

### JOY ENGINE v0.4.6
- Added a Node-safe post-processing configuration entry point so Vite configs can use engine normalization without loading browser-only CSS and shader assets.
- Resolved the package export merge conflict while retaining both development editor entry points and the Node-safe post-processing entry point.
- Kept both games' tuning-config import chains Node-safe by exposing profile helpers through `joy-engine/post-processing`, with regression coverage for loading Vite configs without browser asset loaders.

## 2026-09-15
### FORM LAB v0.3.0
- Added script-owned project name, info, and title metadata, synchronized name-field edits, parameter syntax coloring, and RGBA preview swatches. Documented compiler bounds and lexical shadowing.
- Refined export controls, moved viewport options above generation results, relocated the backend label, fixed point-size slider fill, and restored visible double-click selections.

### JOY ENGINE v0.4.5
- Added shared syntax styling for exposed parameter symbols in lab editors.

### FORM LAB v0.2.0
- Added scalar, XYZ vector, and RGBA color parameters with named `meta(min=..., max=..., step=...)` declarations, typed controls, persisted overrides, and alpha-aware preview/PLY/JSON output. Existing scalar studies remain readable.
- Fit the desktop editor to one page; aligned document-name editing, parameter cards, action buttons, script help, copy, and generated/pending preview metrics with PARTICLE LAB.
- Aligned FORM LAB's document bar, controls, responsive workbench, editor surfaces, sliders, and status treatments with the current PARTICLE LAB tool-suite conventions while preserving its geometry-specific workflow.

### PARTICLE LAB refinements
- Refined the authoring layout, optional performance views, script help, destructive actions, parameter controls, and preview status.
- Added grouped scalar/vector/color properties with compact type selectors, editable component values, collapsible value controls with compact value/color previews, collapsible bounds, matching slider tracks, warmer action hovers, and neutral preview axes.
- Added typed vector and RGBA containers to Joy Engine particle scripts, whole-value assignments and math, component access, bounded color parameters, and documented persistent custom channels.

### PARTICLE LAB v0.1.0
- Add PARTICLE LAB for authoring layered scripted particle effects, exposed parameters, child emission, seeded preview, and portable effect assets.
- Add PARTICLE LAB desktop launch and Windows/macOS/Linux packaging through the Electron shell shared with FORM LAB.
- Clarify PARTICLE LAB editable surfaces, make JSON a read-only preview, default the seed to 7777, and add background/grid controls with per-emitter CPU and workload diagnostics.
- Share FORM LAB’s CodeMirror theme and editing controls with PARTICLE LAB, including syntax colors, line numbers, cursor status and undo history; improve workbench readability.
- Added texture controls (PNG-only at the moment) and a material example in PARTICLE LAB.
- Promote particle authoring to full XYZ motion and perspective rendering with camera-facing billboards, 3D velocity streaks, view-dependent transparency ordering, and an orbit camera shared with FORM LAB.
- Align PARTICLE LAB with FORM LAB’s branded shell, numbered compose/preview/tune panels, document bar, compact controls, and responsive workbench.

### JOY ENGINE v0.4.1
- Replace per-play HTML audio cloning with predecoded Web Audio samples, bounded overlapping voices, gesture activation, and explicit context cleanup to address Safari shooting hitches in SPACE SCUFFLES.

### JOY ENGINE v0.4.0
- Complete the game particle migration to `ParticleEffect`, preserving seeded motion and draw order, with explicit effect cleanup, exhaust-budget eviction, and world-position resizing.
- Add a root documentation hub with centralized project guides and a regenerable public engine API reference (`npm run docs:api`).
- Fix display-resolve GLSL startup by keeping the version directive first.
- Separated reusable engine shaders from project-owned `shaders/` trees, separated bloom filtering from the final display resolve, moved the renderer pipeline into `rendering/postprocessor`, and renamed DCC importers to distinguish them from project content assets.
- Clarified particle runtime ownership and added explicit browser-audio cleanup.
- Add scripted particle-attached point lights, reusable opaque scene lighting, and an Orbiting Lights example with a toggleable ground receiver.
- Add shared Joy Engine effect validation, bounded numeric scripting, deterministic simulation, and particle geometry for editor/game parity.
- Add solid circles, solid squares, textured sprites and animated flipbooks to Joy Engine particles (textures in the editor only support PNG at the moment).

## 2026-09-14
### Platform and Tooling
- Added repository-owned semantic-version management for the site, Joy Engine, and each game workspace, including manifest/lockfile consistency checks.
- Reorganized repository scripts by build, capture, development, maintenance, quality, and testing responsibility.
- Improved the landing-page mobile layout and retained versioned preview images for each playable project.
- Renamed project folders to consistent kebab-case names.

### Joy Engine 0.3.1
- Formalized Joy Engine as the dedicated `joy-engine` npm workspace and enforced the boundary between reusable engine code and project-specific behavior.
- Moved authored GPU programs into standalone WGSL and GLSL files.
- Added reusable development diagnostics with startup/backend logging, FPS and frame-time reporting, optional Chromium heap statistics, responsive placement, and explicit cleanup.
- Expanded the particle system with delayed particles, curved motion, position histories, child emitters, and deterministic random sources while retaining independent system ownership.
- Refined shared bloom, tone mapping, camera, buffer, mesh-loading, geometry, color, DOM, and simulation utilities.
- Completed a repository-wide organization, naming, documentation, and readability pass.

## 2026-09-12
### Joy Engine and effects
- Deepened the luma.gl integration and introduced shared HDR-aware post-processing with bloom, vignette, film grain, saturation, contrast, exposure, and tone-mapping controls.
- Added seeded effect playback and motion scrubbing to make visual simulation repeatable and easier to inspect.
- Significantly expanded explosions with layered cores, debris, smoke, trails, and secondary effects.
- Added shared particle-dynamics and rendering regression coverage.

## 2026-09-11
### Platform
- Added the generated sandbox landing page, chronological project cards, project preview captures, and site/engine/project version displays.
- Added Cloudflare Wrangler deployment support and adjusted release builds for hosted environments.
- Added automated browser captures, image galleries, diagnostic manifests, and last-good preview preservation.
- Added explicit high-DPI and responsive viewport handling.

### Joy Engine
- Reworked particles around project-agnostic emitters and update logic.
- Added GLB mesh loading, a proper top-down orthographic camera, depth buffering, opaque/translucent rendering rules, and blending fixes.
- Consolidated common simulation, input, audio, browser, geometry, and GPU ownership behind the engine API.

## 2026-09-09
### Joy Engine rendering and repository layout
- Expanded post-processing with punchy AgX, scalable higher-quality bloom, anamorphic flare shaping, vignette blur, configurable animated grain, and curated luma.gl effect exports.
- Moved authored JSON configuration and JOYFX assets out of project source trees, made capture folders chronologically sortable, preserved development console history, and documented explicit WebGPU/WebGL 2 development selection.
