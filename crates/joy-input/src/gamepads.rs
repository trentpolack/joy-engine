// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Gamepad polling for the shell: gilrs natively and `navigator.getGamepads()` in browsers, both reported in the W3C
//! standard mapping so bindings agree across platforms.

use crate::GamepadState;
#[cfg(not(target_arch = "wasm32"))]
use crate::gamepad;

/// Polls every connected gamepad once per frame. Platforms without gamepad support report none.
pub struct GamepadPoller {
    #[cfg(not(target_arch = "wasm32"))]
    gilrs: Option<gilrs::Gilrs>,
    pads: Vec<GamepadState>,
}

impl Default for GamepadPoller {
    fn default() -> Self {
        Self::new()
    }
}

impl GamepadPoller {
    /// Open the platform's gamepad service; failures are logged and leave gamepads unavailable.
    pub fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let gilrs = match gilrs::Gilrs::new() {
                Ok(gilrs) => Some(gilrs),
                Err(error) => {
                    log::warn!("Gamepads are unavailable: {error}");
                    None
                }
            };
            Self {
                gilrs,
                pads: Vec::new(),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {
                pads: Vec::new(),
            }
        }
    }

    /// Read every pad's current state.
    pub fn poll(&mut self) -> &[GamepadState] {
        self.pads.clear();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(gilrs) = &mut self.gilrs {
            // Drain events so gilrs updates its cached state; the poll below reads that state.
            while gilrs.next_event().is_some() {}
            self.pads.extend(gilrs.gamepads().map(|(_, pad)| native_state(&pad)));
        }
        #[cfg(target_arch = "wasm32")]
        crate::web_gamepad::poll(&mut self.pads);
        &self.pads
    }
}

/// Standard-mapping button order for gilrs buttons.
#[cfg(not(target_arch = "wasm32"))]
const NATIVE_BUTTONS: [(u16, gilrs::Button); gamepad::BUTTON_COUNT] = [
    (gamepad::BUTTON_SOUTH, gilrs::Button::South),
    (gamepad::BUTTON_EAST, gilrs::Button::East),
    (gamepad::BUTTON_WEST, gilrs::Button::West),
    (gamepad::BUTTON_NORTH, gilrs::Button::North),
    (gamepad::BUTTON_LEFT_BUMPER, gilrs::Button::LeftTrigger),
    (gamepad::BUTTON_RIGHT_BUMPER, gilrs::Button::RightTrigger),
    (gamepad::BUTTON_LEFT_TRIGGER, gilrs::Button::LeftTrigger2),
    (gamepad::BUTTON_RIGHT_TRIGGER, gilrs::Button::RightTrigger2),
    (gamepad::BUTTON_SELECT, gilrs::Button::Select),
    (gamepad::BUTTON_START, gilrs::Button::Start),
    (gamepad::BUTTON_LEFT_STICK, gilrs::Button::LeftThumb),
    (gamepad::BUTTON_RIGHT_STICK, gilrs::Button::RightThumb),
    (gamepad::BUTTON_DPAD_UP, gilrs::Button::DPadUp),
    (gamepad::BUTTON_DPAD_DOWN, gilrs::Button::DPadDown),
    (gamepad::BUTTON_DPAD_LEFT, gilrs::Button::DPadLeft),
    (gamepad::BUTTON_DPAD_RIGHT, gilrs::Button::DPadRight),
    (gamepad::BUTTON_HOME, gilrs::Button::Mode),
];

/// One gilrs pad in the standard mapping. gilrs sticks point up for positive Y; the standard mapping points down.
#[cfg(not(target_arch = "wasm32"))]
fn native_state(pad: &gilrs::Gamepad<'_>) -> GamepadState {
    let mut buttons = vec![0.0; gamepad::BUTTON_COUNT];
    for (index, button) in NATIVE_BUTTONS {
        buttons[usize::from(index)] = pad.button_data(button).map_or(0.0, |data| data.value().clamp(0.0, 1.0));
    }
    let axes = vec![
        pad.value(gilrs::Axis::LeftStickX),
        -pad.value(gilrs::Axis::LeftStickY),
        pad.value(gilrs::Axis::RightStickX),
        -pad.value(gilrs::Axis::RightStickY),
    ];
    GamepadState {
        connected: pad.is_connected(),
        axes,
        buttons,
    }
}
