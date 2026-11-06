// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A seeded random stream that scripts draw from: script type `Random`, with `next_unit()` and `range(min, max)`.
//!
//! The host keeps owning its [`SeededRandom`] and lends it for each batch of hook calls, so gameplay randomness stays one
//! stream shared by Rust and script code in a fixed order, and a cosmetic stream can be lent through a second handle.

use joy_core::RandomSource;
use joy_core::SeededRandom;
use joy_core::random_range;

use crate::LuaSignature;
use crate::loan::ScriptLoan;
use crate::loan::ScriptResult;

/// A script handle to a host random stream. Pass it to hooks as an argument; draws advance the lent stream.
#[derive(Clone, Debug)]
pub struct ScriptRandom {
    stream: ScriptLoan<SeededRandom>,
}

impl ScriptRandom {
    /// A handle with no stream lent.
    pub fn new() -> Self {
        Self {
            stream: ScriptLoan::new("random stream"),
        }
    }

    /// A handle owning its own stream from `seed` (script type `Random`, from `seeded_random(seed)` in scripts): for
    /// procedural generation inside a script, such as a seeded contract board, independent of every host stream.
    pub fn seeded(seed: i64) -> Self {
        Self {
            stream: ScriptLoan::holding("seeded random stream", SeededRandom::new(seed)),
        }
    }

    /// Lend `random` while `call` runs; every draw scripts make advances it, in call order.
    pub fn lend<R>(&self, random: &mut SeededRandom, call: impl FnOnce() -> R) -> R {
        self.stream.lend_copy(random, call)
    }

    /// Draw from the lent stream in a host function (a host-side rule that needs randomness during a script call);
    /// a script error outside a lending call.
    pub fn with_stream<R>(&self, draw: impl FnOnce(&mut SeededRandom) -> R) -> ScriptResult<R> {
        self.stream.write(draw)
    }

    /// The next value in `[0, 1)`; a script error outside a lending call.
    pub fn next_unit(&self) -> ScriptResult<f64> {
        self.stream.write(|random| random.next_unit())
    }

    /// A value in `[minimum, maximum)` from one draw; a script error outside a lending call.
    pub fn range(&self, minimum: f64, maximum: f64) -> ScriptResult<f64> {
        self.stream.write(|random| random_range(minimum, maximum, random))
    }
}

impl Default for ScriptRandom {
    fn default() -> Self {
        Self::new()
    }
}

crate::lua_userdata! {
    ScriptRandom, "Random",
    fields |_fields| {

    }
    methods |methods| {
        methods.add_method("next_unit", LuaSignature::new(&[]), |_, random, ()| random.next_unit());
        methods.add_method(
            "range",
            LuaSignature::new(&["minimum", "maximum"]),
            |_, random, (minimum, maximum): (f64, f64)| random.range(minimum, maximum),
        );
    }
}
