// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Action and axis input for one player, matching the TypeScript `InputManager`.
//!
//! Games describe named actions bound to keys, mouse buttons, or virtual (on-screen) buttons, and axes built from a
//! positive and a negative action. The shell feeds device events in; the game reads one snapshot per frame. Keys use
//! DOM `KeyboardEvent.code` names (`"KeyW"`, `"ArrowUp"`, `"Space"`), so bindings read the same as TypeScript ones.
//! Actions aggregate by maximum value; a press and release completed between two frames still reports `pressed`.

use std::collections::{HashMap, HashSet};

use glam::Vec2;

/// Common DOM key codes, matching the TypeScript `KEY_CODE` constants.
pub mod key {
    /// The W key.
    pub const KEY_W: &str = "KeyW";
    /// The A key.
    pub const KEY_A: &str = "KeyA";
    /// The S key.
    pub const KEY_S: &str = "KeyS";
    /// The D key.
    pub const KEY_D: &str = "KeyD";
    /// The P key.
    pub const KEY_P: &str = "KeyP";
    /// The R key.
    pub const KEY_R: &str = "KeyR";
    /// The up arrow.
    pub const ARROW_UP: &str = "ArrowUp";
    /// The down arrow.
    pub const ARROW_DOWN: &str = "ArrowDown";
    /// The left arrow.
    pub const ARROW_LEFT: &str = "ArrowLeft";
    /// The right arrow.
    pub const ARROW_RIGHT: &str = "ArrowRight";
    /// The space bar.
    pub const SPACE: &str = "Space";
    /// The tab key.
    pub const TAB: &str = "Tab";
    /// The escape key.
    pub const ESCAPE: &str = "Escape";
    /// The backquote key, which toggles development tools.
    pub const BACKQUOTE: &str = "Backquote";
}

/// One way to trigger an action.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InputBinding {
    /// A keyboard key by DOM code name.
    Key(String),
    /// A mouse button: 0 primary, 1 middle, 2 secondary.
    MouseButton(u16),
    /// A named on-screen button driven by the game's UI.
    Virtual(String),
}

impl InputBinding {
    /// Convenience constructor for a key binding.
    pub fn key(code: &str) -> Self {
        Self::Key(code.to_owned())
    }

    /// Convenience constructor for a virtual-button binding.
    pub fn virtual_button(name: &str) -> Self {
        Self::Virtual(name.to_owned())
    }
}

/// An axis built from two actions; the stronger direction wins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputAxis {
    /// Action whose value pushes the axis toward +1.
    pub positive: Option<String>,
    /// Action whose value pushes the axis toward -1.
    pub negative: Option<String>,
    /// Magnitudes at or below this read as zero.
    pub dead_zone: f32,
}

/// Named actions and axes; the manager copies it at construction.
#[derive(Clone, Debug, Default)]
pub struct InputMap {
    actions: Vec<(String, Vec<InputBinding>)>,
    axes: Vec<(String, InputAxis)>,
}

impl InputMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) an action and its bindings.
    pub fn action(mut self, name: &str, bindings: impl IntoIterator<Item = InputBinding>) -> Self {
        self.actions.retain(|(existing, _)| existing != name);
        self.actions.push((name.to_owned(), bindings.into_iter().collect()));
        self
    }

    /// Add (or replace) an axis from a positive and a negative action.
    pub fn axis(mut self, name: &str, positive: &str, negative: &str) -> Self {
        self.axes.retain(|(existing, _)| existing != name);
        self.axes.push((
            name.to_owned(),
            InputAxis { positive: Some(positive.to_owned()), negative: Some(negative.to_owned()), dead_zone: 0.0 },
        ));
        self
    }
}

/// Per-frame state of one action.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ActionState {
    /// Held this frame.
    pub down: bool,
    /// Became held since the previous frame (including taps released before the frame).
    pub pressed: bool,
    /// Stopped being held since the previous frame.
    pub released: bool,
    /// Strongest binding value in `[0, 1]`.
    pub value: f32,
}

/// Pointer position in logical viewport pixels (top-left origin).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerState {
    /// Last known position.
    pub position: Vec2,
    /// Whether the pointer is over the viewport.
    pub active: bool,
}

/// Owns device state and per-frame action edges for one player.
pub struct InputManager {
    // Configuration, copied at construction.
    map: InputMap,

    // Raw device state between frames.
    keys: HashSet<String>,
    mouse_buttons: HashSet<u16>,
    virtual_buttons: HashMap<String, f32>,
    tapped: HashSet<InputBinding>,
    pointer: PointerState,
    touches: indexmap::IndexMap<u64, Vec2>,

    // Snapshot produced by the latest update.
    states: HashMap<String, ActionState>,
    axes: HashMap<String, f32>,
}

impl InputManager {
    /// Create a manager for `map` with every action released.
    pub fn new(map: InputMap) -> Self {
        let states = map.actions.iter().map(|(name, _)| (name.clone(), ActionState::default())).collect();
        let axes = map.axes.iter().map(|(name, _)| (name.clone(), 0.0)).collect();
        Self {
            map,
            keys: HashSet::new(),
            mouse_buttons: HashSet::new(),
            virtual_buttons: HashMap::new(),
            tapped: HashSet::new(),
            pointer: PointerState::default(),
            touches: indexmap::IndexMap::new(),
            states,
            axes,
        }
    }

    /// Record a key transition by DOM code name. Repeats while held are ignored.
    pub fn handle_key(&mut self, code: &str, pressed: bool) {
        if pressed {
            if self.keys.insert(code.to_owned()) {
                self.tapped.insert(InputBinding::Key(code.to_owned()));
            }
        } else {
            self.keys.remove(code);
        }
    }

    /// Record a mouse button transition.
    pub fn handle_mouse_button(&mut self, button: u16, pressed: bool) {
        if pressed {
            if self.mouse_buttons.insert(button) {
                self.tapped.insert(InputBinding::MouseButton(button));
            }
        } else {
            self.mouse_buttons.remove(&button);
        }
    }

    /// Record the pointer position in logical pixels.
    pub fn handle_pointer_move(&mut self, position: Vec2) {
        self.pointer = PointerState { position, active: true };
    }

    /// Record the pointer leaving the viewport.
    pub fn handle_pointer_leave(&mut self) {
        self.pointer.active = false;
    }

    /// Record a touch point in logical pixels, or its end with `None`. Touch ids are unique while the touch lasts.
    pub fn handle_touch(&mut self, id: u64, position: Option<Vec2>) {
        match position {
            Some(position) => {
                self.touches.insert(id, position);
            }
            None => {
                self.touches.shift_remove(&id);
            }
        }
    }

    /// Positions of every active touch, oldest first, for hit-testing on-screen controls. Unlike UI widgets, which
    /// follow one pointer, this reports every finger, so a player can steer and accelerate at once.
    pub fn touches(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.touches.values().copied()
    }

    /// Hold or release a virtual button; `value` is clamped to `[0, 1]` and zero releases it.
    pub fn set_virtual(&mut self, name: &str, value: f32) {
        let value = value.clamp(0.0, 1.0);
        if value > 0.0 {
            if self.virtual_buttons.insert(name.to_owned(), value).is_none() {
                self.tapped.insert(InputBinding::Virtual(name.to_owned()));
            }
        } else {
            self.virtual_buttons.remove(name);
        }
    }

    /// Release every held control, for focus loss, pause, or possession changes. Edges are cleared too.
    pub fn clear(&mut self) {
        self.keys.clear();
        self.mouse_buttons.clear();
        self.virtual_buttons.clear();
        self.tapped.clear();
        self.touches.clear();
        for state in self.states.values_mut() {
            *state = ActionState::default();
        }
        for axis in self.axes.values_mut() {
            *axis = 0.0;
        }
    }

    /// Advance one frame: compute action states and axes, consuming edges. Call once per frame before reading.
    pub fn update(&mut self) {
        let mut next = HashMap::with_capacity(self.states.len());
        for (name, bindings) in &self.map.actions {
            let mut value: f32 = 0.0;
            let mut tapped = false;
            for binding in bindings {
                let binding_value = match binding {
                    InputBinding::Key(code) => f32::from(u8::from(self.keys.contains(code))),
                    InputBinding::MouseButton(button) => f32::from(u8::from(self.mouse_buttons.contains(button))),
                    InputBinding::Virtual(button) => self.virtual_buttons.get(button).copied().unwrap_or(0.0),
                };
                value = value.max(binding_value);
                tapped |= self.tapped.contains(binding);
            }
            let previous = self.states.get(name).copied().unwrap_or_default();
            let down = value > 0.0;
            next.insert(
                name.clone(),
                ActionState {
                    down,
                    pressed: (down && !previous.down) || tapped,
                    released: !down && previous.down,
                    value,
                },
            );
        }
        self.states = next;
        self.tapped.clear();

        for (name, axis) in &self.map.axes {
            let read = |action: &Option<String>| {
                action.as_ref().and_then(|name| self.states.get(name)).map_or(0.0, |state| state.value)
            };
            let positive = read(&axis.positive);
            let negative = read(&axis.negative);
            let value = if positive >= negative { positive } else { -negative };
            self.axes.insert(name.clone(), if value.abs() <= axis.dead_zone { 0.0 } else { value });
        }
    }

    /// State of an action after the latest update; unknown names read as released.
    pub fn action(&self, name: &str) -> ActionState {
        self.states.get(name).copied().unwrap_or_default()
    }

    /// Whether an action was pressed this frame.
    pub fn pressed(&self, name: &str) -> bool {
        self.action(name).pressed
    }

    /// Axis value in `[-1, 1]` after the latest update; unknown names read as zero.
    pub fn axis(&self, name: &str) -> f32 {
        self.axes.get(name).copied().unwrap_or(0.0)
    }

    /// Latest pointer state.
    pub fn pointer(&self) -> PointerState {
        self.pointer
    }

    /// Whether the map binds this key, so the shell can keep the browser from scrolling or tabbing on it.
    pub fn binds_key(&self, code: &str) -> bool {
        self.map.actions.iter().any(|(_, bindings)| {
            bindings.iter().any(|binding| matches!(binding, InputBinding::Key(bound) if bound == code))
        })
    }
}

/// DOM `KeyboardEvent.code` name for a winit key code; winit's variant names follow the same specification.
pub(crate) fn key_code_name(code: winit::keyboard::KeyCode) -> String {
    format!("{code:?}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager() -> InputManager {
        InputManager::new(
            InputMap::new()
                .action("forward", [InputBinding::key(key::KEY_W), InputBinding::virtual_button("forward")])
                .action("reverse", [InputBinding::key(key::KEY_S)])
                .action("pause", [InputBinding::key(key::ESCAPE)])
                .axis("throttle", "forward", "reverse"),
        )
    }

    #[test]
    fn edges_and_axes_follow_held_controls() {
        let mut input = manager();
        input.handle_key(key::KEY_W, true);
        input.update();
        assert!(input.action("forward").pressed && input.action("forward").down);
        assert_eq!(input.axis("throttle"), 1.0);

        input.update();
        assert!(!input.action("forward").pressed && input.action("forward").down);

        input.handle_key(key::KEY_W, false);
        input.handle_key(key::KEY_S, true);
        input.update();
        assert!(input.action("forward").released);
        assert_eq!(input.axis("throttle"), -1.0);
    }

    #[test]
    fn taps_between_frames_still_press() {
        let mut input = manager();
        input.handle_key(key::ESCAPE, true);
        input.handle_key(key::ESCAPE, false);
        input.update();
        assert!(input.pressed("pause"));
        assert!(!input.action("pause").down);
        input.update();
        assert!(!input.pressed("pause"));
    }

    #[test]
    fn virtual_buttons_touches_and_clear() {
        let mut input = manager();
        input.set_virtual("forward", 0.5);
        // Two fingers at once: on-screen controls hit-test every touch, not one pointer.
        input.handle_touch(1, Some(Vec2::new(10.0, 20.0)));
        input.handle_touch(2, Some(Vec2::new(300.0, 20.0)));
        input.handle_touch(1, Some(Vec2::new(12.0, 22.0)));
        input.update();
        assert_eq!(input.axis("throttle"), 0.5);
        assert_eq!(input.touches().collect::<Vec<_>>(), [Vec2::new(12.0, 22.0), Vec2::new(300.0, 20.0)]);
        input.handle_touch(2, None);
        assert_eq!(input.touches().count(), 1);

        // Focus loss and pauses release everything, including fingers still down.
        input.clear();
        input.update();
        assert_eq!(input.axis("throttle"), 0.0);
        assert!(!input.action("forward").down);
        assert_eq!(input.touches().count(), 0);
    }

    #[test]
    fn winit_key_names_match_dom_codes() {
        use winit::keyboard::KeyCode;
        assert_eq!(key_code_name(KeyCode::KeyW), key::KEY_W);
        assert_eq!(key_code_name(KeyCode::ArrowUp), key::ARROW_UP);
        assert_eq!(key_code_name(KeyCode::Space), key::SPACE);
        assert_eq!(key_code_name(KeyCode::Escape), key::ESCAPE);
        assert_eq!(key_code_name(KeyCode::Backquote), key::BACKQUOTE);
    }
}
