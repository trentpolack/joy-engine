// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! A seeded random stream that scripts draw from: script type `Random`, with `next_unit()` and `range(min, max)`.
//!
//! The host keeps owning its [`SeededRandom`] and lends it for each batch of hook calls, so gameplay randomness stays one
//! stream shared by Rust and script code in a fixed order, and a cosmetic stream can be lent through a second handle.

use joy_core::RandomSource;
use joy_core::SeededRandom;
use joy_core::random_range;

use crate::loan::ScriptLoan;
use crate::loan::ScriptResult;

/// Script type name of [`ScriptRandom`].
pub const RANDOM_TYPE_NAME: &str = "Random";

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

    /// Lend `random` while `call` runs; every draw scripts make advances it, in call order.
    pub fn lend<R>(&self, random: &mut SeededRandom, call: impl FnOnce() -> R) -> R {
        self.stream.lend_copy(random, call)
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
