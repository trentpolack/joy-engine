# TypeScript and JavaScript Coding Standards
The reading model is a small procedural composition root, stateful owners with documented lifetimes, and plain records for data. Use TypeScript for reusable runtime contracts and core functionality. JavaScript remains appropriate for build scripts, configuration, and browser composition roots, and is checked with strict JSDoc. Keep browser-native modules and the repository's existing two-space formatting. The goal is that a reader can follow startup, one frame, and disposal without reconstructing hidden state.

## Reference Mapping
This guidance comes from the supplied `joygl-ts` source, specifically `src/client/client.ts`, `src/renderer/renderer.ts`, `src/main.ts`, and `src/ui/ui.ts`, plus JoyCore's `SystemicHealthComponent.h`. These references inform organization and contracts; they do not require porting Three.js or Unreal architecture.

| Reference practice | Application here |
| --- | --- |
| Client initialization proceeds through recognizable stages | Resolve elements, create diagnostics, await rendering resources, connect gameplay and input, resize, then start the loop. Keep this order visible. |
| Renderer groups core, scene, lighting, and optional resources | Group owned resources, borrowed collaborators, configuration, and transient state, with short comments explaining each group. |
| Client and renderer have explicit disposal methods | Each instance releases its own listeners, animation loop, DOM, audio, and GPU resources. Pair setup with teardown. |
| Renderer methods document intent, parameters, and returns | Public JSDoc describes units, coordinate space, mutation, failure, and ownership when relevant. |
| Main defines editable geometry parameters as named data | Put authored game tuning in project configuration; expose reusable mechanism settings as documented options. |
| UI has a distinct owner around the debug interface | Debug tools own their interface and subscriptions; games publish plain diagnostic values through the public engine API. |
| JoyCore distinguishes configuration, transient state, and notifications | Make defaults and mutable state recognizable; use callbacks to report changes across ownership boundaries. |

Adapt the reference with judgment. Avoid its static client singleton, positional boolean option lists, redundant accessors, unchecked non-null assumptions, abandoned commented code, and TODOs that stand in for required cleanup. Prefer independent instances, named options, validated boundaries, and completed lifecycle paths. Do not introduce inheritance solely to resemble C++.

## Module and Class Order
Start with copyright, imports, local contract definitions, and named defaults. Put the primary export and public control path before narrow implementation helpers. A stateful owner should read as constructor/setup, public operations, disposal, then private event handlers and helpers. Keep closely related methods together; do not mechanically reorder a whole project to satisfy a template.

Group constructor fields by responsibility. Explicitly declare optional resources with `null` and JSDoc types instead of creating properties opportunistically in distant methods. Keep handler identities on the owner when they must be removed later. Use `@private` for implementation methods that are outside the consumer contract.

```ts
// Owned rendering resource; null until asynchronous initialization succeeds.
/** @type {GpuRenderer | null} */
this.renderer = null;

// Borrowed input; its creator remains responsible for destruction.
this.input = input;

// Configuration is distinct from transient sample state.
this.sampleWindowMilliseconds = sampleWindowMilliseconds;
this.frameCount = 0;
```

## Control Flow and Naming
Use descriptive nouns for values and verbs for operations. Include units where ambiguity matters: `elapsedSeconds`, `timestampMilliseconds`, `worldPosition`, `pixelRatio`. Conventional local geometry names such as `x`, `y`, and `z` remain appropriate. Use `is`, `has`, `can`, or `enabled` when they clarify a boolean's meaning.

Use braces for multi-step branches and callbacks. Keep one meaningful action per line. Prefer early returns and named intermediate values over nested ternaries, inline side effects, or dense callback chains. A short pure `map` is useful; a pipeline that mutates owners, allocates resources, and registers callbacks should become an explicit loop or named operation.

Keep the frame sequence visible. Name callbacks when they contain decisions, coordinate conversion, resource handling, or several steps. A one-line adapter that forwards a notification is fine. Never synthesize a browser event merely to call application behavior; pointer and keyboard handlers should invoke the same named method directly.

```ts
/**
 * Update the game sim and UI.
 */
function updateFrame() {
  syncPointer();

  // Update the game sim.
  game.update();
}

/** 
 * Render the current frame and update the dev tool state.
 * @param {number} timestampMilliseconds Animation-frame clock.
 */
function renderFrame(timestampMilliseconds) {
  refreshDisplayDensity();

  // Update the dev tool frame data.
  devTools.frame(timestampMilliseconds);

  // Render the current frame.
  renderer.render(game, timestampMilliseconds);
}
```

## Records, Collaborators, and Public Contracts
Use plain records for entities, settings, events, and snapshots. Use classes when an object owns mutable state or resources over time. Do not wrap a record in trivial getters and setters unless they enforce invariants or establish a useful read-only boundary. Prefer composition and explicit constructor dependencies over global lookups.

Public API comments explain purpose, parameters, observable behavior, and return values. Document whether a returned object is owned, borrowed, copied, or read-only. State time units and coordinate spaces, especially at simulation/render boundaries. Describe mutation, supported optional features, meaningful failure conditions, and who destroys resources. Use inferred factory return types where they avoid maintaining duplicate struct definitions; do not weaken contracts with `any` or a cast that hides a missing field.

Implementation comments explain a constraint or decision that the code cannot express. Use short section comments to make real initialization phases and state groups easier to find. Avoid comments that merely restate an assignment or append a generic description to every method.

## Lifetimes and State Transitions
The creator owns cleanup unless the contract explicitly transfers ownership. Initialize dependencies before enabling input or scheduling a frame. If asynchronous startup fails partway through, release already-created resources and preserve a readable failure state. Keep debug diagnostics available early enough to identify the failing stage.

Store unsubscribe and stop functions returned by collaborators. Remove listeners with the exact registered handler. Dispose dependents before their resources, and make repeated disposal harmless. On pagehide, retain resources for a persisted back/forward-cache page. Register Vite disposal when hot replacement can retain the old owner's resources.

```js
function dispose() {
  if(disposed) {
    return;
  }
  disposed = true;

  stopLoop();
  unbindHud();
  input.destroy();
  window.removeEventListener('resize', resize);
  renderer.destroy();
  devTools.destroy();
}
```

Keep gameplay update order, default values, random draws, collision behavior, and time units unchanged during readability refactors. A capture scene can freeze simulation time while rendering continues; performance sampling must use the animation-frame clock.

## Configuration and Authored Assets
Use scoped constants for strings that identify runtime states, entity kinds, input bindings, rendering options, message types or serialized configuration properties. Keep the constants beside the subsystem that owns their meaning; games own game-specific names, and tools own tool-specific names. Prefer immutable named records such as `GARDEN_STATE.AIMING` or `PARTICLE_RENDERER.FLIPBOOK` over repeated literals. Equal spellings do not imply a shared meaning: FORM viewport modes and geometry attribute domains have separate owners.

Shared engine vocabularies are available through the data-only `joy-engine/constants` entry point. Engine internals import the owning leaf module directly. Constant modules must not import runtime owners, DOM code, rendering resources or project code. Keep existing serialized values stable when extracting constants, and migrate definitions, comparisons and dispatch sites together. Give mutable state an explicit type when a literal constant would otherwise narrow its initial value too far.

Ordinary prose, static HTML/CSS, shader source, module paths, `typeof` checks and authored JSON remain literal where that is their natural representation. Do not replace typed property access with computed keys merely to remove text. Sandboxed Electron preloads cannot require arbitrary local modules; preserve that boundary when considering IPC constants.

Use named option records for independently configurable behavior. Put reusable mechanism defaults beside that mechanism and game-specific palettes and tuning in the project's configuration module. Validate meaningful public boundaries once; avoid defensive fallbacks that silently hide broken internal contracts.

Project tuning belongs under `src/config/`, uses a descriptive `*-config.js` suffix, and is aggregated by `game-config.js` when a project needs one stable import surface. Keep configuration as strictly checked JavaScript while it benefits from comments, computed values, shared references, or JSDoc contracts. Use JSON only when an actual external editor or runtime data-loading boundary requires serialization, and pair that boundary with explicit schema validation and player-facing failure handling. File format alone does not make configuration safely editable.

Keep authored GPU programs in `.wgsl` and `.glsl` files, and substantial UI styling in readable CSS. Use multiline markup for structured views and `textContent` for variable labels. JavaScript selects, loads, and binds authored assets. Do not compress source or turn readable shaders/styles into generated strings in authored JavaScript.

## Review Checklist
- Can a reader identify each owner, borrowed dependency, and mutable record?
- Can startup, one frame, failure, and disposal be followed in order?
- Are timing, coordinates, mutation, and resource ownership documented at public boundaries?
- Are configuration defaults visible and named?
- Are complex callbacks and expressions expanded into readable operations?
- Do the changed modules actually follow these standards?
- Were behavior-preserving refactors checked separately from intentional behavior changes?
- Were actual development and production entry points exercised, with browser evidence for UI changes?
