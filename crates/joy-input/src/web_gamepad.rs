// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Browser gamepads through `navigator.getGamepads()`, which already reports the W3C standard mapping.

use wasm_bindgen::JsCast;

use crate::GamepadState;

/// Append the state of every pad the page can see. Browsers expose pads only after a button press on the page.
pub(crate) fn poll(pads: &mut Vec<GamepadState>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(list) = window.navigator().get_gamepads() else {
        return;
    };
    for entry in list.iter() {
        let Ok(pad) = entry.dyn_into::<web_sys::Gamepad>() else {
            continue;
        };
        let axes = pad.axes().iter().map(|value| value.as_f64().unwrap_or(0.0) as f32).collect();
        let buttons = pad
            .buttons()
            .iter()
            .map(|button| button.dyn_into::<web_sys::GamepadButton>().map_or(0.0, |button| button.value() as f32))
            .collect();
        pads.push(GamepadState { connected: pad.connected(), axes, buttons });
    }
}
