# Game Scripts
`joy_engine::game_script` (the `joy-game-script` crate) runs gameplay code written in [Rhai](https://rhai.rs), a small Rust-like scripting language implemented in pure Rust. Scripts are text assets: games load them through `GameAssets`, so native development builds hot reload behavior without rebuilding, while browser and release builds embed them. The same scripts run natively and in WebAssembly. Systems stay in Rust; scripts hold the rules and behavior that benefit from fast iteration, such as AI decisions, mission logic, and event reactions.

Rhai was chosen over Lua because the browser target is `wasm32-unknown-unknown`: Lua implementations wrap C code that needs a C toolchain and `setjmp`/`longjmp` there, while Rhai builds like any other Rust crate. It also enforces operation and depth limits, forbids file access by default, and binds Rust types directly.

## Modules
| Module | Contents |
| --- | --- |
| `engine.rs` | `ScriptEngine` (the sandboxed language, host registration through `rhai_mut`, and `register_enum`) and `ScriptLimits`. |
| `script.rs` | `Script` (compile, required hooks, hot reload through `replace_source`, `call`, and `call_as`), `ScriptHook`, and `ScriptState`. |
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

## Hot Reload
`Script::replace_source` compiles the new text with the same path and hooks. If it parses, runs, and defines every hook, it replaces the program and advances `revision`; otherwise it returns the error and the previous program keeps running. States belong to the host's objects, so every object keeps its fields across a reload; an edit that adds a field should set a default where the field is first read.

## Lending Host State
Registered functions are `'static` closures and `unsafe` is forbidden, so scripts cannot borrow a host's world. `ScriptLoan<T>` moves a value into a shared slot for the length of one call and hands it back afterward. Registered functions read it with `read` or `write` and return `ScriptResult`. Handles a script keeps in `this` find the slot empty outside the call and fail with an error instead of reading stale state. Moving is a shallow copy, so lending a large owner every step is cheap. Hosts lending a field of `self` swap an empty stand-in into its place, and `lend_copy` copies small values in and back out.

`ScriptRandom` lends a `SeededRandom` this way. The host keeps owning its gameplay stream, the script draws from it with `random.next_unit()` and `random.range(min, max)`, and Rust and script draws interleave in call order. Keep separate handles for gameplay and cosmetic streams.

## Determinism
Scripts compute in `f64`. The engine keeps script results identical to the equivalent Rust code, so a behavior can move between Rust and script without changing seeded recordings:

- Rhai's float comparisons are approximate in builds with limits enabled: values within a relative `f64::EPSILON` compare equal, so `4.9e-17 <= 0.0` would be true. The standard library registers exact IEEE comparisons and turns off Rhai's operator fast path so they apply. Arithmetic still uses Rhai's built-ins, which are exact.
- `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `exp`, `ln`, `log2`, `log10`, `powf`, `powi`, and `hypot` call `math::deterministic`. Rhai's own math package and the floating-point `**` operator, which use platform `f64` methods, are left out; `**` is a compile error.
- `sqrt`, `floor`, `ceil`, `round`, `trunc`, `fract`, `min`, `max`, `clamp`, `lerp`, `PI()`, `TAU()`, `to_float`, and `to_int` match their Rust counterparts.
- `vec3(x, y, z)` wraps `DVec3`: `x`, `y`, and `z` fields; `+`, `-`, `*`, and `/` (by vectors or scalars); `==` and `!=`; and `length`, `length_squared`, `distance`, `dot`, `cross`, and `lerp` methods. Normalization is left to scripts so they choose their own zero-length fallback.
- Clock functions are compiled out (Rhai's `no_time` feature).

FRAMEBREAK's hostile AI was ported line for line from Rust to `assets/scripts/hostile.rhai`, and its six recorded battles still match their golden recordings exactly.

## Sandbox and Limits
Scripts cannot read files, import modules, or read the clock. Every hook call runs under `ScriptLimits`: an operation budget (200,000 operations by default, which stops a runaway loop in well under a millisecond), a call depth of 32, an expression depth of 64, and strings, arrays, and maps of at most 10,000 elements. A limit ends only that call with a `ScriptError::Runtime`; the script and its states stay usable. `print` and `debug` write to the `log` facade at info and debug levels.

## Errors
`ScriptError` messages lead with `path:line:column`. Runtime errors unwrap nested script calls to the line that failed and name the call stack (`think → move_frame`). Hosts decide what a failed call means; FRAMEBREAK idles the unit for that step and reports each distinct failure once.

## Binding Types
Register host types on `ScriptEngine::rhai_mut()` with Rhai's API (`register_type_with_name`, `register_get`, `register_get_set`, and `register_fn`), re-exported as `joy_engine::game_script::rhai`, so games never depend on `rhai` directly and the engine pins one version. `register_enum` registers an enumeration whose variants read as `Type::Variant` and compare with `==`. Prefer small read-only views passed by value (wrap a snapshot in `Rc` so script copies are cheap), lend owned state that scripts query, and return decisions as values the host applies, so scripts never mutate simulation state behind the host's back.
