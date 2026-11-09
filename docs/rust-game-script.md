# Game Scripts
`joy_engine::game_script` runs gameplay rules and authored data in [Lua 5.4](https://www.lua.org/manual/5.4/), through a pinned [mlua](https://github.com/mlua-rs/mlua) binding. Enable the facade's `game-script` feature to use it. Games load `.lua` text through `GameAssets`: native development reloads edits, while browser and release builds embed the same source. Engine systems and hot inner loops remain Rust. FRAMEBREAK uses scripts for its battle rules, AI, assembly, missions, and balance tables; SPACE SCUFFLES for its weapon profiles; and VECTORS TO VICTORY for its tactical AI scoring.

## Build Requirements
The binding revision is `a0ad4535fc72ba89967a931c5be57b162b5f0f3f`, with `lua54`, `vendored`, and `serde` enabled. The locked `lua-src` package `551.1.0` supplies Lua **5.4.9**. Native builds need a C compiler; Cargo builds the vendored Lua source.

Browser builds keep Rust's `wasm32-unknown-unknown` target. Install WASI SDK 34 and set `WASI_SDK_PATH` (Linux x86-64 hosts can run `export WASI_SDK_PATH=$(bash joy-engine/scripts/rust/install-wasi-sdk.sh /path/to/cache)` from the hosting workspace). The wasm-bindgen CLI must match the locked Rust version (`0.2.129`). The existing npm browser commands configure the C compiler, archiver, C flags, and Rust linker flags before Cargo runs. Lua uses WASI's standard WebAssembly exception support for `setjmp` and `longjmp`. For direct browser checks from the engine root, run `node scripts/rust/lua-wasm-toolchain.mjs check --workspace --all-features --target wasm32-unknown-unknown --locked`.

The browser workflow preserves WebAssembly feature metadata through binding generation, exports `__wasm_call_ctors`, and supplies the WASI adapter before startup. Release output removes debug names after binding generation. The platform crate selects a shared libc heap through `libc_alloc` and a small C alignment adapter. No authored unsafe Rust, C++ runtime, or Emscripten is required. The browser adapter exposes no preopened files or directories, process arguments, or environment variables. Games that do not enable `game-script` need no WASI build tools.

## Hosting Hooks
A host owns one `ScriptEngine` and registers bindings before compiling. Each script runs in a private global environment; sources compiled as parts share their function declarations. Lexical locals belong to their own source chunk. A hook's first argument is explicitly `this`; `ScriptHook` arity counts only the remaining host arguments.

```rust
use joy_engine::game_script::{Script, ScriptEngine, ScriptHook, ScriptState};

let engine = ScriptEngine::default();
let hooks = [ScriptHook::new("init", 0), ScriptHook::new("tick", 1)];
let script = Script::compile(&engine, "scripts/charger.lua", source, &hooks)?;
let mut state = ScriptState::new();
script.call(&engine, "init", &mut state, ())?;
let charge: f64 = script.call_as(&engine, "tick", &mut state, (0.5,))?;
```

```lua
local RATE = 2.0

function init(this)
    this.charge = 0.0
end

function tick(this, elapsed_seconds)
    add_charge(this, elapsed_seconds * RATE)
    return this.charge
end

function add_charge(this, amount)
    this.charge = this.charge + amount
end
```

`ScriptState::new()` has no VM until its first call or `set(lua, field, value)`. After binding it accepts only that VM. `from_value` adopts an existing table and reports a non-table value as an error. `get`, `get_as`, and `with_field` read fields; `edit_field` writes a replacement and reports conversion failures. A cloned state shares its table. States keep their VM alive, and state survives accepted hot reloads. An error ends the current call but keeps any mutations already made to state. Failed hooks close pending Lua 5.4 `<close>` locals while the same instruction budget and host loans remain active. Cleanup failures preserve the original runtime error; exhausted budgets can stop cleanup before it finishes.

Lua tables use reference semantics. Passing a record, returning it, or assigning it to another field shares that table. Explicitly copy records when creating independent units or retaining historical snapshots. Array indices start at one; convert zero-based Rust collection IDs at the array access boundary. A missing field reads as `nil`, so arithmetic on a misspelled field errors, but optional fields need deliberate `nil` or `null` handling.

## Libraries and Reloads
`ScriptLibrary` owns sources by name, a module cache, and compiled hook scripts. A module returns an export table:

```lua
local PARTS = {
    {id = "rifle", damage = 12.5, pellets = 1},
    {id = "shotgun", damage = 4.0, pellets = 8},
}
return {PARTS = PARTS}
```

Other sources use `local parts = require("data/parts")`. The name must match a source added by the host; imports never read arbitrary files. Exported tables, nested tables, table keys, and metatables are frozen, including cyclic graphs. Frozen tables retain their identity through private backing tables; writes, `rawset`, and table mutation functions cannot edit them. `pairs`, `ipairs`, `next`, length, and raw reads remain usable, while iteration and metatable inspection cannot expose the backing. Read frozen records from Rust through `from_script_value` or `constant`; direct mlua raw table traversal sees the empty proxy. The host conversion preserves serde array markers. Copy a catalog entry before changing an instance. Module functions may close over local constants; module initialization should only declare rules and data, with gameplay mutations owned by hook state. Reload candidates rebuild their imported module cache so a rejected edit cannot mutate an accepted module’s captured local values. Mutable module upvalues are not persistent gameplay state; retain that state in `ScriptState`.

`compile_script_parts` runs several sources in one private namespace so `update_unit(this.units[index], dt)` can call a global helper declared in another part. Imports are normally top-level declarations. The library records the dependency graph while resolving imports. `replace_source` validates affected modules and every transitive hook importer before committing the new programs and source. A failed replacement restores the previous cache and dependency graph. An accepted replacement increments affected scripts' revisions; their host-owned object states are unchanged.

`library.constant::<T>("data/parts", "PARTS")` reads an export into a serde record. Read and cache large catalogs during startup or accepted reloads, rather than converting them every frame.

## Shipped Sources and Development Edits
`ScriptSources` owns a game's shipped scripts: a list of `ScriptSource::new(name, include_str!(…))` entries under one project-relative folder (`assets/scripts/`), where each name is both the import spelling and the path below the folder (`rules/combat` is `assets/scripts/rules/combat.lua`). `embedded_assets()` hands the paths and shipped text to `GameAssets::new`, `library(engine)` builds a library over the current text, and `library_from(engine, sources)` builds one over any candidate set. When `GameAssets::poll_changes` reports an edit, `apply_edit(path, text, accept)` offers it: paths outside the set return `None`, and otherwise `accept` validates the whole candidate set (compile the hook scripts, read the data the host caches, run a sample call). Only an accepted edit becomes current; a rejected one returns a `ScriptEditError` naming the file and keeps the previous revision. Hosts that keep a compiled library alive keep it in step with `replace_source` using the accepted name, or rebuild it from `current()`. FRAMEBREAK hosts its battle, AI, and rules this way; SPACE SCUFFLES reads its weapon profiles and VECTORS TO VICTORY scores its tactical AI through the same set.

Data records that keep `&'static str` fields (so every system can hold catalog entries by reference) deserialize through an owned spelling and convert with `intern(&text)`, which shares one allocation per distinct text for the life of the process. Reloading a module leaks only text that changed; reserve it for authored vocabulary, never unbounded run-time text.

## Host Bindings and Loans
Use the re-exported `mlua` crate, so host functions, tables, and userdata agree with the engine's pinned ABI:

```rust
use joy_engine::game_script::LuaSignature;

engine.bindings().function(
    "armor_damage",
    LuaSignature::new(&["damage", "armor"]),
    |_, (damage, armor): (f64, f64)| Ok((damage - armor).max(0.0)),
)?;
```

Rust host view types use `lua_userdata!` to implement `mlua::UserData` and describe the same fields and methods to LuaLS; methods are called with Lua's colon syntax (`world:unit_position(id)`). Use `ScriptVec3(position)` to return an engine `DVec3`. `register_enum` builds a readonly table from host values implementing `IntoScriptValue` and `LuaType`; userdata variants define equality and display in their own `UserData` implementation. Typed hook results use `FromScriptValue`: scalars, original Lua handles, vectors, options, sequences, and maps. Frozen collections are read through their private backing storage; returning a `Table` or `Value`, including inside a collection, preserves its original identity and never exposes the backing. Custom host result types implement `FromScriptValue` and can delegate userdata conversion to their existing `mlua::FromLua` implementation. Typed collection traversal follows the finite Rust result type; serde record conversion below retains its graph cycle and depth checks.

Host hook arguments use `IntoScriptArguments`: tuples of checked scalar values, Lua handles, `ScriptRandom`, `ScriptVec3`, and recursively checked Rust collections. Wrap custom host userdata as `ScriptUserData(my_view)` in the argument tuple; mlua callbacks continue using their normal `UserData` return values. Nested foreign handles are rejected before mlua can push them, including on browser builds where Rust panics abort. Binding registration and direct calls through `lua()` are trusted host operations, which can bypass the script sandbox and hook budget.

`ScriptLoan<T>` safely moves a host owner into a temporary slot for a batch of calls. Registered callbacks capture a cloned loan and use `read` or `write`; access outside a lending call, or reentrant conflicting access, returns `mlua::Error`. `lend` returns the owner and callback result. `lend_copy` is for small copied values such as a random stream. Nested loans restore their outer value, including when a Rust callback unwinds. Retaining a script handle never extends access to a completed loan. Host callbacks must bound their own CPU and Rust allocations.

## Serde and Numeric Contracts
`to_script_value(lua, context, &record)` and `from_script_value(lua, context, &value)` make VM ownership explicit. Tables and userdata from another VM are errors. Host vectors annotated with `#[serde(with = "script_vec3")]`, or `script_vec3::option`, become immutable `ScriptVec3` userdata. Other arrays stay arrays. Cyclic records and nesting deeper than 64 tables are conversion errors, avoiding recursive stack overflow on authored values.

`Option::None` becomes the global `null` sentinel, including inside arrays. Unlike `nil`, it keeps a one-based slot present and preserves length. Nullable script fields written back to Rust should use `null`. An array such as `{unit, null, unit}` retains all three entries. Sequences serialized from Rust or passed through checked `Vec`, slice, and array arguments carry mlua's array marker, preserving empty-array shape through serde. Typed `Option` results accept both `nil` and `null`.

Lua 5.4 has signed 64-bit integers and `f64` floating-point numbers. Integer operations retain Lua integer semantics; mixing floats promotes arithmetic to floating point. Engine vector components and portable scalar helpers use `f64`. Integer IDs within `i64` retain exact values; converting large integers to floats can lose precision. Use explicit integer formatting for IDs and labels when a portable helper returns a float. The standard library exposes portable `sin`, `cos`, `tan`, `atan2`, `exp`, `ln`, `powf`, `powi`, `hypot`, and other engine math helpers, plus `sqrt`, `floor`, `round`, `abs`, `min`, `max`, `clamp`, `lerp`, `PI()`, and `TAU()`. The engine's angle and distance helpers (`wrap_angle`, `angle_difference`, `approach_angle`, `damp_factor`, `normalize_or`, `horizontal_distance`, and `point_segment_distance`) are globals too, computing exactly what `joy_engine::math` computes in Rust, so scripts never reimplement them. The `math` namespace routes its transcendental functions to the same portable helpers. Use `fmod` for truncating remainder when porting Rust `%`; Lua `%` uses floor remainder.

Hooks and modules compile as text only. The platform `^` operator is rejected before compilation; use `powf(base, exponent)`. Dynamic source loading is unavailable, so scripts cannot compile around this restriction. Quoted strings, long strings, and comments are ordinary Lua text. `vec3(x, y, z)` creates engine f64 userdata with arithmetic and colon methods (`length`, `distance`, `dot`, `cross`, `lerp`, and normalization). Vectors are immutable: rebuild a vector to edit a component. Assigning a vector to a second field is safe because arithmetic produces a new value.

`ScriptRandom` exposes `random:next_unit()` and `random:range(minimum, maximum)`. Each method advances the engine's seeded stream exactly once. `random.lend(&mut host_stream, || ...)` preserves host/script draw order. `seeded_random(seed)` creates a separate script-owned stream. Gameplay and cosmetic streams remain separate; unseeded `math.random` and clocks are unavailable.

## Limits and Diagnostics
`ScriptLimits` exposes two honest Lua limits:

| Field | Default | Meaning |
| --- | --- | --- |
| `instructions_per_call` | 2,000,000 | Lua VM instructions, including nested imports and protected calls. Native library work and Rust callbacks are outside this count. Zero disables it. |
| `memory_bytes` | 64 MiB | Total VM allocations for scripts, modules, and persistent states, excluding trusted host Rust allocations. Zero disables it. |

Each hook and top-level script evaluation starts a new budget. A hook executes on a fresh host-owned coroutine. A count hook raises an error when the instruction budget is exhausted. Sandboxed `pcall` and `xpcall` check the budget before running their handlers or continuing, so protected calls cannot swallow exhaustion, including inside nonyieldable native callbacks such as `table.sort` comparators and `string.gsub` replacements. Imports share their caller's budget. The next host call starts fresh.

Coroutines and authored `__gc` finalizers are unavailable: Lua disables instruction hooks during garbage-collection finalizers. Standard library tables and the shared string metatable are protected from script edits. Allocation failure reports a script error; releasing state and collecting garbage can recover space. Native library operations and trusted host callbacks must bound their own CPU work; this instruction budget is not a wall-clock deadline. Lua's built-in parser and VM stack limits are not exposed as configurable expression or call-depth settings.

`ScriptError` distinguishes compilation, missing hooks, undeclared hooks, return conversion, runtime errors, and serde conversion. Errors retain the source path and Lua traceback, including Lua's usual omission of eliminated tail-call frames. Lines are one-based; columns are zero when unavailable. A rejected reload leaves the previous program active, making these diagnostics suitable for live authoring overlays.

## Validation
Run `cargo test -p joy-game-script` from the engine workspace for hook state, source parts, transitive reload acceptance and rollback, safe loans, null slots, cyclic and deep tables, foreign VM rejection, deterministic math and random draws, allocation ceilings, and protected-call budget recovery. Consumers additionally retain their existing gameplay recordings and browser captures; changing runtimes does not justify re-recording unexplained simulation differences.


## Lua Editor Definitions
The engine generates LuaLS `*.d.lua` libraries from its Rust registrations, without another dependency. Run these commands in the engine checkout, or use the same commands from JoyGames to include FRAMEBREAK's additional API:

```bash
npm run generate:lua-definitions
npm run check:lua-definitions
npm run watch:lua-definitions
```

Generation writes ignored `.lua-definitions/joy-engine.d.lua`; a consuming game's additions go in its own `.lua-definitions/` folder. The checked-in `.luarc.json` selects Lua 5.4, loads those folders through `workspace.library`, and disables the lowercase-global style warning for the engine's intentional private script namespaces. Generated files are editor metadata and must never be loaded as gameplay sources. `npm run check` refreshes them before checking Rust. JoyGames and FRAMEBREAK also refresh them at development startup. Keep the watch command running while changing Rust bindings for continuous updates; it scans only the relevant source trees and manifests once per second, coalesces changes, and keeps the previous output if compilation fails. The explicit check command is read-only and fails for missing or stale files.

### Registering Functions and Types
Use `engine.bindings().function(...)` for exposed callbacks. Callback argument and return types supply the Lua types; `LuaSignature` supplies argument names and descriptions that Rust does not retain:

```rust
engine.bindings().function(
    "distance_scale",
    LuaSignature::new(&["distance", "scale"]).doc("Distance is in world meters."),
    |_, (distance, scale): (f64, f64)| Ok(distance * scale),
)?;
```

`LuaSignature::argument(index, type)` and `returns(type)` describe dynamic `mlua::Value` or table contracts beside their conversions. Scalars, vectors, arrays, optional values, tuples, and the supported userdata types derive their signatures from Rust. A Rust `Option<T>` maps to `T|nil` through ordinary mlua callbacks; the explicit engine `null` sentinel has the separate `JoyNull` type. A callback returning that sentinel must describe it explicitly, such as `returns("Vec3|JoyNull")`. This does not change the serde record conversion rules described in this guide.

Use `lua_userdata!` to declare each userdata's fields and methods once. The macro implements mlua's `UserData` and the engine's `LuaType`, running the same declarations against either the runtime or a definition collector. Callbacks are not executed by the collector:

```rust
joy_engine::game_script::lua_userdata! {
    Counter, "Counter",
    fields |fields| {
        fields.add_field_method_get("value", |_, counter| Ok(counter.value));
    }
    methods |methods| {
        methods.add_method_mut("advance", LuaSignature::new(&["amount"]), |_, counter, amount: i64| {
            counter.value += amount;
            Ok(counter.value)
        });
    }
}
```

Include userdata passed only through hook arguments with `engine.definitions().include::<Counter>()`; types referenced by described callbacks and fields are discovered automatically. Dynamic table shapes can be named with `definitions().record(name, fields)`. Those shapes are explicit authoring contracts, not reflection over arbitrary table-building or serde code. Keep them beside the owning conversions and validate important record behavior through the existing gameplay tests.

`engine.lua_definitions()` audits the actual public globals and one level of namespace members before emitting stable, sorted definitions. New, removed, replaced, or shadowed globals outside the registration layer fail the audit. Internal sandbox setup explicitly records removed standard bindings. Register callbacks in globals or one existing namespace, such as `math.helper`; deeper namespaces are intentionally outside this small API. Keep private VM plumbing on `engine.lua()`. The trusted raw handle remains available, but new public functions belong in `bindings()`.

### Game Extensions
The shared exporter uses `entity_script_engine` to include portable helpers, vectors, random streams, and the entity/world APIs. A game's native exporter builds its normal registrations and calls `lua_definitions_extension(&base_engine)` to emit only additions, rejecting conflicting shared signatures. FRAMEBREAK's exporter is a working example; its userdata remains in the game and reaches the generator through the public engine API.

A consuming npm package declares `luaDefinitions.package` and `luaDefinitions.binary`, then runs `node <engine>/scripts/lua-definitions.mjs --project <game-directory>`. The binary writes only its metadata to stdout; the launcher owns output paths, atomic file replacement, freshness checks, and watching. Point the consumer's LuaLS library configuration at both generated directories. The engine never imports a game.

### Editor Limits
LuaLS understands declared types, methods, and vector operators, but its field annotations cannot enforce runtime read-only fields or separate getter and setter types. Generated field comments state those restrictions, including FRAMEBREAK's nullable `target_id` getter whose setter accepts an integer. Runtime tests continue to protect those contracts. Generic tables remain generic unless their registration supplies a structural description.

LuaLS's standard basic and math libraries still describe functions that the sandbox removes, such as `load` and `math.random`; definitions are not a sandbox validator. The workspace disables the unavailable `os`, `io`, `debug`, and `coroutine` libraries. Engine compilation and runtime checks remain authoritative for sandbox restrictions, including restricted imports and the portable-power rule.
