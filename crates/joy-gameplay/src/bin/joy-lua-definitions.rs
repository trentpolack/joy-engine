// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Emit the shared Lua API without starting a window, loading a game, or executing callbacks.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = joy_gameplay::entity_script_engine(Default::default());
    print!("{}", engine.lua_definitions()?);
    Ok(())
}
