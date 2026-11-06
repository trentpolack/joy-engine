# Game Scripts
`joy_engine::game_script` (the `joy-game-script` crate) runs gameplay code written in [Rhai](https://rhai.rs), a small Rust-like scripting language implemented in pure Rust. Scripts are text assets: games load them through `GameAssets`, so native development builds hot reload behavior without rebuilding, while browser and release builds embed them. The same scripts run natively and in WebAssembly. A game can keep most of its rules and balance data in scripts: engine systems and hot inner loops (collision sweeps, projectile flight, structures, rendering) stay in Rust and are offered to scripts as host functions. FRAMEBREAK runs its whole battle (units, weapons, damage, AI, and missions), frame assembly, contract board, and every balance table this way.

Rhai was chosen over Lua because the browser target is `wasm32-unknown-unknown`: Lua implementations wrap C code that needs a C toolchain and `setjmp`/`longjmp` there, while Rhai builds like any other Rust crate. It also enforces operation and depth limits, forbids file access by default, and binds Rust types directly.

## Modules
| Module | Contents |
| --- | --- |
| `engine.rs` | `ScriptEngine` (the sandboxed language, host registration through `rhai_mut`, and `register_enum`) and `ScriptLimits`. |
| `script.rs` | `Script` (compile one source or several parts, required hooks, hot reload through `replace_source`, `call`, and `call_as`), `ScriptHook`, and `ScriptState`. |
| `library.rs` | `ScriptLibrary`: every source by name, `import` between them, hook scripts, data constants read into Rust, and hot reload that follows imports. |
| `convert.rs` | `to_script_value`, `from_script_value`, and `script_vec3`, the `serde` bridge between host records and script values. |
| `loan.rs` | `ScriptLoan`, which lends host state to registered functions for one call, and `ScriptResult`. |
| `random.rs` | `ScriptRandom`, the script handle to a lent `SeededRandom`. |
| `standard_library.rs` | Exact float comparisons, portable math, `vec3`, and `Random`. |
| `error.rs` | `ScriptError` and `ScriptLocation`. |

## Hosting a Script
A host owns one `ScriptEngine`, registers its types on it, and compiles each script with the hooks it will call. Each scripted object owns a `ScriptState`, the object map a hook sees as `this`:

```rust
use joy_engine::game_script::{Script, ScriptEngine, ScriptHook, ScriptLimits, ScriptState};

const HOOKS: [ScriptHook; 2] = [ScriptHook::new("init", 0), ScriptHook::new("tick", 1)];

let mut engine = ScriptEngine::new(ScriptLimits::default());
// engine.rhai_mut().register_type_with_name::<MyView>("MyView").register_get("speed", ...);
let script = Script::compile(&engine, "assets/scripts/charger.rhai", source, &HOOKS)?;

let mut state = ScriptState::new();
script.call(&engine, "init", &mut state, ())?;
let charge: f64 = script.call_as(&engine, "tick", &mut state, (elapsed_seconds,))?;
```

```rhai
const RATE = 2.0;

fn init() {
    this.charge = 0.0;
}

fn tick(elapsed_seconds) {
    this.add_charge(elapsed_seconds * global::RATE);
    this.charge
}

fn add_charge(amount) {
    this.charge += amount;
}
```

- A script's top level runs once, at compile time, and should only declare `const` values. Functions read them as `global::NAME`; Rhai functions cannot see other top-level variables.
- Parameters are copies. Only `this` is shared, and helpers called as methods (`this.add_charge(…)`) share the caller's `this`. Return a modified value (`input = this.move(input)`) rather than expecting a parameter to change.
- Reading a state field that was never set is an error, so typos surface immediately.
- `spawn` and a few other names are reserved words in Rhai; hook names such as `init` and `think` are safe.

## Libraries, Imports, and Parts
A game with more than one script keeps them in a `ScriptLibrary`: add every source by name (usually derived from its asset path), then compile the scripts whose hooks the host calls. Sources reach each other two ways:

- `import "data/parts" as parts;` loads another library source as a namespaced module. A module's top level runs once; its `export const` values are shared by every importer (`parts::ARM_WEAPONS`) and its functions are called qualified (`parts::find_part(id)`). Imports never read files.
- `ScriptLibrary::compile_script_parts(name, parts, hooks)` assembles one hook script from several sources whose functions share a namespace. Rhai calls functions as methods (`this.units[i].advance(dt)`, which edits a unit in place) only within one namespace, so rules split across files for readability use parts rather than imports. Each part keeps its own constants and imports.

`ScriptLibrary::replace_source` recompiles the edited source, every module that imports it, and every hook script built from it or importing it, and commits only if all of them compile. A broken edit changes nothing.

## Data Modules and Records
Authored data can live in modules (`export const PARTS = [ ... ];`), with comments beside the numbers and helper functions for defaults. Rust reads it with `ScriptLibrary::constant::<T>(module, name)`, and any `serde` record moves in and out of scripts with `to_script_value` and `from_script_value`: structs become object maps, enums their variant names, `None` becomes `()`, and `f64` values stay bit for bit. Reading back is strict, so a missing field or misspelled variant is a `ScriptError::Conversion` naming the field. glam serializes `DVec3` as a plain array, so mark vector fields `#[serde(with = "script_vec3")]` (or `script_vec3::option`) to make them `vec3` values in scripts.

Records with `&'static str` fields cannot derive `Deserialize`; deserialize them through an owned spelling and intern the text, as FRAMEBREAK's `src/data/serde_static.rs` does.

## Hot Reload
`Script::replace_source` compiles the new text with the same path and hooks. If it parses, runs, and defines every hook, it replaces the program and advances `revision`; otherwise it returns the error and the previous program keeps running. States belong to the host's objects, so every object keeps its fields across a reload; an edit that adds a field should set a default where the field is first read.

## Lending Host State
Registered functions are `'static` closures and `unsafe` is forbidden, so scripts cannot borrow a host's world. `ScriptLoan<T>` moves a value into a shared slot for the length of one call and hands it back afterward. Registered functions read it with `read` or `write` and return `ScriptResult`. Handles a script keeps in `this` find the slot empty outside the call and fail with an error instead of reading stale state. Moving is a shallow copy, so lending a large owner every step is cheap. Hosts lending a field of `self` swap an empty stand-in into its place, and `lend_copy` copies small values in and back out.

`ScriptRandom` lends a `SeededRandom` this way. The host keeps owning its gameplay stream, the script draws from it with `random.next_unit()` and `random.range(min, max)`, and Rust and script draws interleave in call order; host functions draw from the same lent stream with `ScriptRandom::with_stream`. Keep separate handles for gameplay and cosmetic streams. `seeded_random(seed)` gives a script its own stream for procedural generation (a seeded contract board), independent of every host stream.

## Determinism
Scripts compute in `f64`. The engine keeps script results identical to the equivalent Rust code, so a behavior can move between Rust and script without changing seeded recordings:

- Rhai's float comparisons are approximate in builds with limits enabled: values within a relative `f64::EPSILON` compare equal, so `4.9e-17 <= 0.0` would be true. The standard library registers exact IEEE comparisons and turns off Rhai's operator fast path so they apply. Arithmetic still uses Rhai's built-ins, which are exact.
- `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `exp`, `ln`, `log2`, `log10`, `powf`, `powi`, and `hypot` call `math::deterministic`. Rhai's own math package and the floating-point `**` operator, which use platform `f64` methods, are left out; `**` is a compile error.
- `sqrt`, `floor`, `ceil`, `round`, `trunc`, `fract`, `min`, `max` (floats and integers), `clamp`, `lerp`, `PI()`, `TAU()`, `to_float`, and `to_int` match their Rust counterparts.
- `vec3(x, y, z)` wraps `DVec3`: `x`, `y`, and `z` fields; `+`, `-`, `*`, and `/` (by vectors or scalars); `==` and `!=`; and `length`, `length_squared`, `distance`, `dot`, `cross`, and `lerp` methods. Normalization is left to scripts so they choose their own zero-length fallback.
- Clock functions are compiled out (Rhai's `no_time` feature).

FRAMEBREAK's hostile AI was first ported line for line from Rust to `assets/scripts/hostile.rhai` with its six recorded battles matching exactly; its frame assembly and contract board ports still match their recordings exactly.

## Sandbox and Limits
Scripts cannot read files or the clock, and import only other sources of their library. Every hook call runs under `ScriptLimits`: an operation budget (200,000 operations by default, which stops a runaway loop in well under a millisecond), a call depth of 32, an expression depth of 64, and strings, arrays, and maps of at most 10,000 elements. Rhai checks collection sizes by walking the value being changed, so a host whose script state nests large collections (a battle record holding every unit) sets `collection_size` to zero and relies on the operation budget. A limit ends only that call with a `ScriptError::Runtime`; the script and its states stay usable. `print` and `debug` write to the `log` facade at info and debug levels.

## Performance
Rhai interprets its syntax tree, so script code runs roughly 100 to 300 times slower than the equivalent Rust. Measured in FRAMEBREAK (release, native): a scripted hostile decision costs about 30 µs, and a full battle step with seven units (AI, movement, mounts, firing, damage, and the mission) about 1.5 ms. Keep per-element inner loops (collision sweeps, projectile flight, particle updates) in Rust as host functions, and write scripts to avoid copies:

- Reading a variable, an array element, or a field into a local (`let unit = this.units[i]`, `for unit in this.units`) copies it, nested maps included. Read and write through the path instead (`this.units[i].armor`), and loop over indices.
- Parameters are copies too: pass the few values a function needs rather than whole records, and edit records through method calls on their path.
- Script functions win over host functions of the same name and arity, so name script methods distinctly from host functions (`move_body`, not `integrate`).

## Errors
`ScriptError` messages lead with `path:line:column`. Runtime errors unwrap nested script calls to the line that failed and name the call stack (`think → move_frame`). Hosts decide what a failed call means; FRAMEBREAK idles the unit for that step and reports each distinct failure once.

## Entity Scripts
Games that build their world from `joy_engine::gameplay` entities do not bind their own types: an authored `script` component names a library script, and `gameplay::ScriptSystem` runs its `init(entity)` and `update(entity, world, elapsed_seconds)` hooks (`ENTITY_SCRIPT_HOOKS`) against engine-owned `Entity` and `World` views, applying the commands scripts return (`spawn`, `remove`, `send`, `start_timer`, `set_velocity`, `apply_impulse`, `move_to`). Create the library from `gameplay::entity_script_engine`, which registers those views; the world view's queries (`within`, `nearest`, `position`, `timer_fired`, `contacts`, `events`) read a snapshot captured once per update. This is the line between script and Rust: scripts decide, the world applies, and anything that needs solver access or runs per particle stays in Rust. See [simulation](rust-simulation.md#entity-scripts).

## Binding Types
Register host types on `ScriptEngine::rhai_mut()` with Rhai's API (`register_type_with_name`, `register_get`, `register_get_set`, and `register_fn`), re-exported as `joy_engine::game_script::rhai`, so games never depend on `rhai` directly and the engine pins one version. `register_enum` registers an enumeration whose variants read as `Type::Variant` and compare with `==`. Prefer small read-only views passed by value (wrap a snapshot in `Rc` so script copies are cheap), lend owned state that scripts query, and return decisions as values the host applies, so scripts never mutate simulation state behind the host's back.
