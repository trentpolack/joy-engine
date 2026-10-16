// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Action and axis input for one player, matching the TypeScript `InputManager`.
//!
//! Games describe named actions bound to keys, mouse buttons, gamepad buttons, or virtual (on-screen) buttons, and
//! axes built from a positive and a negative action plus an optional analog stick. The shell feeds device events in and
//! polls gamepads; the game reads one snapshot per frame. Keys use DOM `KeyboardEvent.code` names (`"KeyW"`,
//! `"ArrowUp"`, `"Space"`) and gamepads use the W3C standard mapping, so bindings read the same as TypeScript ones.
//!
//! Actions aggregate by maximum value and count as held above [`PRESS_THRESHOLD`]; a press and release completed
//! between two frames still reports `pressed`. Every action and axis belongs to one or more contexts (by default
//! [`GAMEPLAY_CONTEXT`]); only actions in an active context respond. Focus loss, hidden windows, and UI text entry
//! suspend every device, and gamepads must return to neutral after any release before they count again.

use std::collections::{HashMap, HashSet};

use glam::Vec2;

/// The default input context.
pub const GAMEPLAY_CONTEXT: &str = "gameplay";

/// Action values above this read as held.
pub const PRESS_THRESHOLD: f32 = 0.15;

/// Default analog dead zone, rescaled so values leave it at zero.
pub const DEFAULT_DEAD_ZONE: f32 = 0.15;

/// Common DOM key codes, matching the TypeScript `KEY_CODE` constants.
pub mod key {
    /// The escape key.
    pub const ESCAPE: &str = "Escape";
    /// The enter key.
    pub const ENTER: &str = "Enter";
    /// The tab key.
    pub const TAB: &str = "Tab";
    /// The space bar.
    pub const SPACE: &str = "Space";
    /// The backspace key.
    pub const BACKSPACE: &str = "Backspace";
    /// The delete key.
    pub const DELETE: &str = "Delete";
    /// The up arrow.
    pub const ARROW_UP: &str = "ArrowUp";
    /// The down arrow.
    pub const ARROW_DOWN: &str = "ArrowDown";
    /// The left arrow.
    pub const ARROW_LEFT: &str = "ArrowLeft";
    /// The right arrow.
    pub const ARROW_RIGHT: &str = "ArrowRight";
    /// The home key.
    pub const HOME: &str = "Home";
    /// The end key.
    pub const END: &str = "End";
    /// The page up key.
    pub const PAGE_UP: &str = "PageUp";
    /// The page down key.
    pub const PAGE_DOWN: &str = "PageDown";
    /// The left shift key.
    pub const SHIFT_LEFT: &str = "ShiftLeft";
    /// The right shift key.
    pub const SHIFT_RIGHT: &str = "ShiftRight";
    /// The left control key.
    pub const CONTROL_LEFT: &str = "ControlLeft";
    /// The right control key.
    pub const CONTROL_RIGHT: &str = "ControlRight";
    /// The backquote key, which toggles development tools.
    pub const BACKQUOTE: &str = "Backquote";
    /// The A key.
    pub const KEY_A: &str = "KeyA";
    /// The B key.
    pub const KEY_B: &str = "KeyB";
    /// The C key.
    pub const KEY_C: &str = "KeyC";
    /// The D key.
    pub const KEY_D: &str = "KeyD";
    /// The E key.
    pub const KEY_E: &str = "KeyE";
    /// The F key.
    pub const KEY_F: &str = "KeyF";
    /// The G key.
    pub const KEY_G: &str = "KeyG";
    /// The H key.
    pub const KEY_H: &str = "KeyH";
    /// The I key.
    pub const KEY_I: &str = "KeyI";
    /// The J key.
    pub const KEY_J: &str = "KeyJ";
    /// The K key.
    pub const KEY_K: &str = "KeyK";
    /// The L key.
    pub const KEY_L: &str = "KeyL";
    /// The M key.
    pub const KEY_M: &str = "KeyM";
    /// The N key.
    pub const KEY_N: &str = "KeyN";
    /// The O key.
    pub const KEY_O: &str = "KeyO";
    /// The P key.
    pub const KEY_P: &str = "KeyP";
    /// The Q key.
    pub const KEY_Q: &str = "KeyQ";
    /// The R key.
    pub const KEY_R: &str = "KeyR";
    /// The S key.
    pub const KEY_S: &str = "KeyS";
    /// The T key.
    pub const KEY_T: &str = "KeyT";
    /// The U key.
    pub const KEY_U: &str = "KeyU";
    /// The V key.
    pub const KEY_V: &str = "KeyV";
    /// The W key.
    pub const KEY_W: &str = "KeyW";
    /// The X key.
    pub const KEY_X: &str = "KeyX";
    /// The Y key.
    pub const KEY_Y: &str = "KeyY";
    /// The Z key.
    pub const KEY_Z: &str = "KeyZ";
    /// The 1 key on the main row.
    pub const DIGIT1: &str = "Digit1";
    /// The 2 key on the main row.
    pub const DIGIT2: &str = "Digit2";
    /// The 3 key on the main row.
    pub const DIGIT3: &str = "Digit3";
    /// The 4 key on the main row.
    pub const DIGIT4: &str = "Digit4";
    /// The 5 key on the main row.
    pub const DIGIT5: &str = "Digit5";
    /// The 6 key on the main row.
    pub const DIGIT6: &str = "Digit6";
    /// The 7 key on the main row.
    pub const DIGIT7: &str = "Digit7";
    /// The 8 key on the main row.
    pub const DIGIT8: &str = "Digit8";
    /// The 9 key on the main row.
    pub const DIGIT9: &str = "Digit9";
    /// The 0 key on the main row.
    pub const DIGIT0: &str = "Digit0";
}

/// W3C standard gamepad mapping indices, shared by browsers and the native gamepad backend.
pub mod gamepad {
    /// Bottom face button (A on Xbox layouts, Cross on PlayStation).
    pub const BUTTON_SOUTH: u16 = 0;
    /// Right face button (B, Circle).
    pub const BUTTON_EAST: u16 = 1;
    /// Left face button (X, Square).
    pub const BUTTON_WEST: u16 = 2;
    /// Top face button (Y, Triangle).
    pub const BUTTON_NORTH: u16 = 3;
    /// Left shoulder bumper.
    pub const BUTTON_LEFT_BUMPER: u16 = 4;
    /// Right shoulder bumper.
    pub const BUTTON_RIGHT_BUMPER: u16 = 5;
    /// Left analog trigger.
    pub const BUTTON_LEFT_TRIGGER: u16 = 6;
    /// Right analog trigger.
    pub const BUTTON_RIGHT_TRIGGER: u16 = 7;
    /// Back, view, or select.
    pub const BUTTON_SELECT: u16 = 8;
    /// Start, menu, or options.
    pub const BUTTON_START: u16 = 9;
    /// Left stick click.
    pub const BUTTON_LEFT_STICK: u16 = 10;
    /// Right stick click.
    pub const BUTTON_RIGHT_STICK: u16 = 11;
    /// Directional pad up.
    pub const BUTTON_DPAD_UP: u16 = 12;
    /// Directional pad down.
    pub const BUTTON_DPAD_DOWN: u16 = 13;
    /// Directional pad left.
    pub const BUTTON_DPAD_LEFT: u16 = 14;
    /// Directional pad right.
    pub const BUTTON_DPAD_RIGHT: u16 = 15;
    /// Home or guide.
    pub const BUTTON_HOME: u16 = 16;
    /// Number of standard buttons.
    pub const BUTTON_COUNT: usize = 17;

    /// Left stick horizontal; right is positive.
    pub const AXIS_LEFT_X: usize = 0;
    /// Left stick vertical; down is positive.
    pub const AXIS_LEFT_Y: usize = 1;
    /// Right stick horizontal; right is positive.
    pub const AXIS_RIGHT_X: usize = 2;
    /// Right stick vertical; down is positive.
    pub const AXIS_RIGHT_Y: usize = 3;
    /// Number of standard axes.
    pub const AXIS_COUNT: usize = 4;
}

/// One way to trigger an action.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum InputBinding {
    /// A keyboard key by DOM code name (TypeScript `{key}`).
    Key(String),
    /// A mouse button: 0 primary, 1 middle, 2 secondary (TypeScript `{pointer}`). Touches hold the primary button.
    MouseButton(u16),
    /// A gamepad button in the standard mapping, from any connected pad (TypeScript `{button}`); see [`gamepad`].
    GamepadButton(u16),
    /// A named on-screen button driven by the game's UI (TypeScript `{virtual}`).
    Virtual(String),
}

impl InputBinding {
    /// A key binding.
    pub fn key(code: &str) -> Self {
        Self::Key(code.to_owned())
    }

    /// A mouse-button binding.
    pub fn mouse_button(button: u16) -> Self {
        Self::MouseButton(button)
    }

    /// A gamepad-button binding.
    pub fn gamepad_button(button: u16) -> Self {
        Self::GamepadButton(button)
    }

    /// A virtual-button binding.
    pub fn virtual_button(name: &str) -> Self {
        Self::Virtual(name.to_owned())
    }
}

/// An action's bindings and the contexts it responds in.
#[derive(Clone, Debug, PartialEq)]
pub struct InputAction {
    /// Ways to trigger the action; the strongest wins.
    pub bindings: Vec<InputBinding>,
    /// Contexts the action responds in.
    pub contexts: Vec<String>,
}

/// An axis from two actions and an optional analog stick axis.
///
/// The digital value is the positive action's value minus the negative one's; a gamepad stick replaces it when its
/// rescaled magnitude is stronger. The result is clamped to `[-1, 1]`.
#[derive(Clone, Debug, PartialEq)]
pub struct InputAxis {
    /// Action whose value pushes the axis toward +1.
    pub positive: Option<String>,
    /// Action whose value pushes the axis toward -1.
    pub negative: Option<String>,
    /// Standard-mapping gamepad axis read from every connected pad; see [`gamepad`].
    pub gamepad_axis: Option<usize>,
    /// Analog magnitudes at or below this read as zero; the rest rescale to `[0, 1]`. Clamped to `[0, 0.99]`.
    pub dead_zone: f32,
    /// Contexts the axis responds in.
    pub contexts: Vec<String>,
}

impl Default for InputAxis {
    fn default() -> Self {
        Self {
            positive: None,
            negative: None,
            gamepad_axis: None,
            dead_zone: DEFAULT_DEAD_ZONE,
            contexts: vec![GAMEPLAY_CONTEXT.to_owned()],
        }
    }
}

impl InputAxis {
    /// An axis from a positive and a negative action in the gameplay context.
    pub fn new(positive: &str, negative: &str) -> Self {
        Self { positive: Some(positive.to_owned()), negative: Some(negative.to_owned()), ..Self::default() }
    }

    /// Also read a standard-mapping gamepad axis.
    pub fn gamepad_axis(mut self, axis: usize) -> Self {
        self.gamepad_axis = Some(axis);
        self
    }

    /// Replace the analog dead zone.
    pub fn dead_zone(mut self, dead_zone: f32) -> Self {
        self.dead_zone = dead_zone;
        self
    }

    /// Replace the contexts the axis responds in.
    pub fn contexts<'a>(mut self, contexts: impl IntoIterator<Item = &'a str>) -> Self {
        self.contexts = contexts.into_iter().map(str::to_owned).collect();
        self
    }
}

/// Named actions and axes; the manager copies it at construction.
#[derive(Clone, Debug, Default)]
pub struct InputMap {
    actions: Vec<(String, InputAction)>,
    axes: Vec<(String, InputAxis)>,
}

impl InputMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add (or replace) an action in the gameplay context.
    pub fn action(self, name: &str, bindings: impl IntoIterator<Item = InputBinding>) -> Self {
        self.action_in(name, bindings, [GAMEPLAY_CONTEXT])
    }

    /// Add (or replace) an action that responds in the given contexts.
    pub fn action_in<'a>(
        mut self,
        name: &str,
        bindings: impl IntoIterator<Item = InputBinding>,
        contexts: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        self.actions.retain(|(existing, _)| existing != name);
        let action = InputAction {
            bindings: bindings.into_iter().collect(),
            contexts: contexts.into_iter().map(str::to_owned).collect(),
        };
        self.actions.push((name.to_owned(), action));
        self
    }

    /// Add (or replace) a gameplay axis from a positive and a negative action.
    pub fn axis(self, name: &str, positive: &str, negative: &str) -> Self {
        self.axis_with(name, InputAxis::new(positive, negative))
    }

    /// Add (or replace) an axis with full options, such as a gamepad stick.
    pub fn axis_with(mut self, name: &str, axis: InputAxis) -> Self {
        self.axes.retain(|(existing, _)| existing != name);
        self.axes.push((name.to_owned(), axis));
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

/// One gamepad's state in the W3C standard mapping; see [`gamepad`] for indices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GamepadState {
    /// Whether the pad is connected; disconnected pads contribute nothing.
    pub connected: bool,
    /// Stick axes in `[-1, 1]`.
    pub axes: Vec<f32>,
    /// Button values in `[0, 1]`; triggers are analog.
    pub buttons: Vec<f32>,
}

/// An input operation named something the map does not define.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InputError {
    /// The action does not exist.
    #[error("Unknown input action \"{0}\".")]
    UnknownAction(String),
}

/// Owns device state and per-frame action edges for one player.
pub struct InputManager {
    // Configuration, copied at construction.
    actions: Vec<(String, InputAction)>,
    axes: Vec<(String, InputAxis)>,
    contexts: HashSet<String>,

    // Raw device state between frames.
    keys: HashSet<String>,
    mouse_buttons: HashSet<u16>,
    virtual_buttons: HashMap<String, f32>,
    pointer: PointerState,
    touches: indexmap::IndexMap<u64, Vec2>,

    // Relative pointer motion accumulated between updates, the latest frame's total, and the platform pointer lock.
    motion_live: Vec2,
    motion: Vec2,
    pointer_locked: bool,
    polled_gamepads: Vec<GamepadState>,
    gamepads: Vec<GamepadState>,
    block_gamepads: bool,

    // Suspension sources.
    typing: bool,
    blurred: bool,
    hidden: bool,

    // Live states between frames (edges accumulate) and the snapshot produced by the latest update.
    live: HashMap<String, ActionState>,
    states: HashMap<String, ActionState>,
    axis_values: HashMap<String, f32>,
}

impl InputManager {
    /// Create a manager for `map` with every action released and the gameplay context active.
    pub fn new(map: InputMap) -> Self {
        let live: HashMap<String, ActionState> =
            map.actions.iter().map(|(name, _)| (name.clone(), ActionState::default())).collect();
        let axis_values = map.axes.iter().map(|(name, _)| (name.clone(), 0.0)).collect();
        Self {
            actions: map.actions,
            axes: map.axes,
            contexts: HashSet::from([GAMEPLAY_CONTEXT.to_owned()]),
            keys: HashSet::new(),
            mouse_buttons: HashSet::new(),
            virtual_buttons: HashMap::new(),
            pointer: PointerState::default(),
            touches: indexmap::IndexMap::new(),
            motion_live: Vec2::ZERO,
            motion: Vec2::ZERO,
            pointer_locked: false,
            polled_gamepads: Vec::new(),
            gamepads: Vec::new(),
            block_gamepads: false,
            typing: false,
            blurred: false,
            hidden: false,
            states: live.clone(),
            live,
            axis_values,
        }
    }

    /// Whether focus, visibility, or UI text entry keeps every device from contributing.
    pub fn suspended(&self) -> bool {
        self.typing || self.blurred || self.hidden
    }

    /// Record a key transition by DOM code name. Presses are ignored while suspended; releases always apply.
    pub fn handle_key(&mut self, code: &str, pressed: bool) {
        if pressed {
            if self.suspended() {
                return;
            }
            self.keys.insert(code.to_owned());
        } else {
            self.keys.remove(code);
        }
        self.refresh();
    }

    /// Record a mouse button transition. Presses are ignored while suspended; releases always apply.
    pub fn handle_mouse_button(&mut self, button: u16, pressed: bool) {
        if pressed {
            if self.suspended() {
                return;
            }
            self.mouse_buttons.insert(button);
        } else {
            self.mouse_buttons.remove(&button);
        }
        self.refresh();
    }

    /// Record the pointer position in logical pixels.
    pub fn handle_pointer_move(&mut self, position: Vec2) {
        self.pointer = PointerState { position, active: true };
    }

    /// Record relative pointer motion in logical pixels: raw mouse movement, which keeps arriving at the screen edge
    /// while the pointer is locked, for mouse look. Accumulates until the next [`update`](Self::update); ignored while
    /// suspended.
    pub fn handle_pointer_motion(&mut self, delta: Vec2) {
        if self.suspended() || !delta.is_finite() {
            return;
        }
        self.motion_live += delta;
    }

    /// Relative pointer motion during the latest frame, in logical pixels (+X right, +Y down); zero while suspended.
    pub fn pointer_motion(&self) -> Vec2 {
        self.motion
    }

    /// Record whether the platform currently holds the pointer lock; the shell reports it every frame.
    pub fn set_pointer_locked(&mut self, locked: bool) {
        self.pointer_locked = locked;
    }

    /// Whether the pointer is locked to the viewport (hidden, reporting only relative motion). Browsers can release the
    /// lock on their own (Escape), so games requesting it should watch for it ending.
    pub fn pointer_locked(&self) -> bool {
        self.pointer_locked
    }

    /// Record the pointer leaving the viewport.
    pub fn handle_pointer_leave(&mut self) {
        self.pointer.active = false;
    }

    /// Record a touch point in logical pixels, or its end with `None`. Touch ids are unique while the touch lasts.
    ///
    /// Touches move the pointer and hold the primary mouse button, as browser pointer events do; new touches are ignored
    /// while suspended.
    pub fn handle_touch(&mut self, id: u64, position: Option<Vec2>) {
        match position {
            Some(position) => {
                if self.suspended() && !self.touches.contains_key(&id) {
                    return;
                }
                self.touches.insert(id, position);
                self.handle_pointer_move(position);
            }
            None => {
                self.touches.shift_remove(&id);
            }
        }
        self.refresh();
    }

    /// Positions of every active touch, oldest first, for hit-testing on-screen controls. Unlike UI widgets, which
    /// follow one pointer, this reports every finger, so a player can steer and accelerate at once.
    pub fn touches(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.touches.values().copied()
    }

    /// Every active touch with its id, oldest first. Ids stay the same while a finger is down, so on-screen sticks and
    /// look pads can follow the finger that started on them after it leaves their bounds.
    pub fn touch_points(&self) -> impl Iterator<Item = (u64, Vec2)> + '_ {
        self.touches.iter().map(|(id, position)| (*id, *position))
    }

    /// Hold or release a virtual button; `value` is clamped to `[0, 1]` (non-finite values release) and zero releases
    /// it. Ignored while suspended.
    pub fn set_virtual(&mut self, name: &str, value: f32) {
        if self.suspended() {
            return;
        }
        let value = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
        if value > 0.0 {
            self.virtual_buttons.insert(name.to_owned(), value);
        } else {
            self.virtual_buttons.remove(name);
        }
        self.refresh();
    }

    /// Supply the latest gamepad poll; the shell calls this before every [`update`](Self::update). All connected pads
    /// contribute.
    pub fn set_gamepads(&mut self, gamepads: &[GamepadState]) {
        self.polled_gamepads.clear();
        self.polled_gamepads.extend_from_slice(gamepads);
    }

    /// Replace the active contexts and release every hold; an empty list disables all actions.
    pub fn set_contexts<'a>(&mut self, contexts: impl IntoIterator<Item = &'a str>) {
        self.contexts = contexts.into_iter().map(str::to_owned).collect();
        self.clear();
    }

    /// Whether a context is active.
    pub fn context_active(&self, context: &str) -> bool {
        self.contexts.contains(context)
    }

    /// Replace one action's bindings and release every hold.
    pub fn rebind(&mut self, name: &str, bindings: impl IntoIterator<Item = InputBinding>) -> Result<(), InputError> {
        let Some((_, action)) = self.actions.iter_mut().find(|(existing, _)| existing == name) else {
            return Err(InputError::UnknownAction(name.to_owned()));
        };
        action.bindings = bindings.into_iter().collect();
        self.clear();
        Ok(())
    }

    /// Record window focus; losing it releases every hold and suspends input until focus returns.
    pub fn set_focused(&mut self, focused: bool) {
        self.blurred = !focused;
        if !focused {
            self.clear();
        }
    }

    /// Record window or page visibility; any change releases every hold, and hidden windows suspend input.
    pub fn set_visible(&mut self, visible: bool) {
        self.hidden = !visible;
        self.clear();
    }

    /// Record whether UI text entry has keyboard focus; starting to type releases every hold and suspends input.
    pub fn set_typing(&mut self, typing: bool) {
        let started = typing && !self.typing;
        self.typing = typing;
        if started {
            self.clear();
        }
    }

    /// Release every held control, for focus loss, pause, or possession changes. Gamepads must pass through neutral
    /// before they count again.
    pub fn clear(&mut self) {
        self.keys.clear();
        self.mouse_buttons.clear();
        self.virtual_buttons.clear();
        self.touches.clear();
        self.motion_live = Vec2::ZERO;
        self.gamepads.clear();
        self.block_gamepads = true;
        self.refresh();
    }

    /// Advance one frame: poll gamepads, snapshot action states and axes, and consume edges. Call once per frame before
    /// reading.
    pub fn update(&mut self) {
        let pads = std::mem::take(&mut self.polled_gamepads);
        if self.block_gamepads && !pads.iter().any(gamepad_active) {
            self.block_gamepads = false;
        }
        self.gamepads = if self.suspended() || self.block_gamepads { Vec::new() } else { pads.clone() };
        self.polled_gamepads = pads;
        self.refresh();
        let motion = std::mem::take(&mut self.motion_live);
        self.motion = if self.suspended() { Vec2::ZERO } else { motion };

        self.states.clone_from(&self.live);
        for state in self.live.values_mut() {
            state.pressed = false;
            state.released = false;
        }

        for (name, axis) in &self.axes {
            let read = |action: &Option<String>| {
                action.as_ref().and_then(|name| self.states.get(name)).map_or(0.0, |state| state.value)
            };
            let value = if self.suspended() || !self.enabled(&axis.contexts) {
                0.0
            } else {
                let mut value = read(&axis.positive) - read(&axis.negative);
                let dead_zone = axis.dead_zone.clamp(0.0, 0.99);
                for pad in self.gamepads.iter().filter(|pad| pad.connected) {
                    let raw = axis.gamepad_axis.and_then(|index| pad.axes.get(index)).copied().unwrap_or(0.0);
                    let analog = raw.signum() * ((raw.abs() - dead_zone) / (1.0 - dead_zone)).max(0.0);
                    if analog.abs() > value.abs() {
                        value = analog;
                    }
                }
                value.clamp(-1.0, 1.0)
            };
            self.axis_values.insert(name.clone(), value);
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
        self.axis_values.get(name).copied().unwrap_or(0.0)
    }

    /// Latest pointer state.
    pub fn pointer(&self) -> PointerState {
        self.pointer
    }

    /// Whether an action in an active context binds this key, so the shell can keep the browser from scrolling or
    /// tabbing on it.
    pub fn binds_key(&self, code: &str) -> bool {
        self.actions.iter().any(|(_, action)| {
            self.enabled(&action.contexts)
                && action.bindings.iter().any(|binding| matches!(binding, InputBinding::Key(bound) if bound == code))
        })
    }

    /// Whether any of `contexts` is active.
    fn enabled(&self, contexts: &[String]) -> bool {
        contexts.iter().any(|context| self.contexts.contains(context))
    }

    /// Current value of one binding.
    fn binding_value(&self, binding: &InputBinding) -> f32 {
        let held = |held: bool| f32::from(u8::from(held));
        match binding {
            InputBinding::Key(code) => held(self.keys.contains(code)),
            InputBinding::MouseButton(button) => {
                held(self.mouse_buttons.contains(button) || (*button == 0 && !self.touches.is_empty()))
            }
            InputBinding::Virtual(name) => self.virtual_buttons.get(name).copied().unwrap_or(0.0),
            InputBinding::GamepadButton(button) => self
                .gamepads
                .iter()
                .filter(|pad| pad.connected)
                .filter_map(|pad| pad.buttons.get(usize::from(*button)).copied())
                .fold(0.0, f32::max),
        }
    }

    /// Recompute every live action state from device state, recording edges until the next update consumes them.
    fn refresh(&mut self) {
        let suspended = self.suspended();
        let values: Vec<f32> = self
            .actions
            .iter()
            .map(|(_, action)| {
                if suspended || !self.enabled(&action.contexts) {
                    return 0.0;
                }
                action.bindings.iter().map(|binding| self.binding_value(binding)).fold(0.0, f32::max)
            })
            .collect();
        for ((name, _), value) in self.actions.iter().zip(values) {
            let state = self.live.entry(name.clone()).or_default();
            let down = value > PRESS_THRESHOLD;
            if down && !state.down {
                state.pressed = true;
            } else if !down && state.down {
                state.released = true;
            }
            state.down = down;
            state.value = value;
        }
    }
}

/// Whether a pad has any button or stick outside neutral.
fn gamepad_active(pad: &GamepadState) -> bool {
    pad.connected
        && (pad.buttons.iter().any(|value| *value > PRESS_THRESHOLD)
            || pad.axes.iter().any(|value| value.abs() > PRESS_THRESHOLD))
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
                .action("brake", [InputBinding::key(key::SPACE), InputBinding::gamepad_button(gamepad::BUTTON_SOUTH)])
                .action("fire", [InputBinding::mouse_button(0)])
                .action_in("menu", [InputBinding::key(key::ENTER)], ["menu"])
                .axis("throttle", "forward", "reverse")
                .axis_with("steer", InputAxis::new("forward", "reverse").gamepad_axis(gamepad::AXIS_LEFT_X)),
        )
    }

    fn pad(axes: [f32; 4], south: f32) -> GamepadState {
        let mut buttons = vec![0.0; gamepad::BUTTON_COUNT];
        buttons[usize::from(gamepad::BUTTON_SOUTH)] = south;
        GamepadState { connected: true, axes: axes.to_vec(), buttons }
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

        // Opposing digital directions cancel, as in TypeScript.
        input.handle_key(key::KEY_W, true);
        input.update();
        assert_eq!(input.axis("throttle"), 0.0);
    }

    #[test]
    fn taps_between_frames_still_press() {
        let mut input = manager();
        input.handle_key(key::ESCAPE, true);
        input.handle_key(key::ESCAPE, false);
        input.update();
        assert!(input.pressed("pause"));
        assert!(input.action("pause").released);
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
        assert_eq!(
            input.touch_points().collect::<Vec<_>>(),
            [(1, Vec2::new(12.0, 22.0)), (2, Vec2::new(300.0, 20.0))],
            "ids follow their fingers"
        );
        // Touches hold the primary pointer button, like browser pointer events.
        assert!(input.action("fire").down);
        assert_eq!(input.pointer().position, Vec2::new(12.0, 22.0));
        input.handle_touch(2, None);
        assert_eq!(input.touches().count(), 1);

        // Focus loss and pauses release everything, including fingers still down.
        input.clear();
        input.update();
        assert_eq!(input.axis("throttle"), 0.0);
        assert!(!input.action("forward").down);
        assert!(!input.action("fire").down);
        assert_eq!(input.touches().count(), 0);
    }

    #[test]
    fn values_at_the_press_threshold_do_not_hold() {
        let mut input = manager();
        input.set_virtual("forward", PRESS_THRESHOLD);
        input.update();
        assert!(!input.action("forward").down && !input.pressed("forward"));
        assert_eq!(input.action("forward").value, PRESS_THRESHOLD);
        input.set_virtual("forward", 0.2);
        input.update();
        assert!(input.pressed("forward"));
    }

    #[test]
    fn gamepads_rescale_dead_zones_and_wait_for_neutral_after_release() {
        let mut input = manager();
        // Analog: 0.575 with the default 0.15 dead zone rescales to 0.5; buttons count from any pad.
        input.set_gamepads(&[pad([0.575, 0.0, 0.0, 0.0], 1.0), GamepadState::default()]);
        input.update();
        assert!((input.axis("steer") - 0.5).abs() < 1e-6);
        assert!(input.pressed("brake"));
        // Sticks inside the dead zone read zero; disconnected pads contribute nothing.
        input.set_gamepads(&[pad([0.1, 0.0, 0.0, 0.0], 0.0), GamepadState { connected: false, ..pad([1.0; 4], 1.0) }]);
        input.update();
        assert_eq!(input.axis("steer"), 0.0);
        assert!(input.action("brake").released);

        // After a release, a pad still held stays blocked until it returns to neutral.
        input.set_gamepads(&[pad([0.0; 4], 1.0)]);
        input.update();
        assert!(input.action("brake").down);
        input.clear();
        input.set_gamepads(&[pad([0.0; 4], 1.0)]);
        input.update();
        assert!(!input.action("brake").down);
        input.set_gamepads(&[pad([0.0; 4], 0.0)]);
        input.update();
        input.set_gamepads(&[pad([0.0; 4], 1.0)]);
        input.update();
        assert!(input.pressed("brake"));
    }

    #[test]
    fn contexts_gate_actions_axes_and_bound_keys() {
        let mut input = manager();
        assert!(input.binds_key(key::SPACE) && !input.binds_key(key::ENTER));
        input.handle_key(key::ENTER, true);
        input.update();
        assert!(!input.action("menu").down);

        input.set_contexts(["menu"]);
        assert!(input.binds_key(key::ENTER) && !input.binds_key(key::SPACE));
        input.handle_key(key::ENTER, true);
        input.handle_key(key::KEY_W, true);
        input.update();
        assert!(input.pressed("menu"));
        assert!(!input.action("forward").down);
        assert_eq!(input.axis("throttle"), 0.0);

        input.set_contexts([]);
        input.update();
        assert!(!input.action("menu").down);
    }

    #[test]
    fn rebind_replaces_bindings_and_rejects_unknown_actions() {
        let mut input = manager();
        input.handle_key(key::ESCAPE, true);
        input.rebind("pause", [InputBinding::key(key::KEY_P)]).unwrap();
        input.update();
        assert!(!input.action("pause").down, "rebinding releases holds");
        input.handle_key(key::KEY_P, true);
        input.update();
        assert!(input.pressed("pause"));
        assert_eq!(input.rebind("missing", []), Err(InputError::UnknownAction("missing".to_owned())));
    }

    #[test]
    fn suspension_ignores_presses_but_applies_releases() {
        let mut input = manager();
        input.set_focused(false);
        input.handle_key(key::KEY_W, true);
        input.set_virtual("forward", 1.0);
        input.set_gamepads(&[pad([1.0, 0.0, 0.0, 0.0], 1.0)]);
        input.update();
        assert!(!input.action("forward").down && !input.action("brake").down);
        assert_eq!(input.axis("steer"), 0.0);

        input.set_focused(true);
        input.handle_key(key::KEY_W, true);
        input.update();
        assert!(input.action("forward").down);
        input.set_typing(true);
        input.update();
        assert!(input.action("forward").released, "starting to type releases holds");
        input.set_typing(false);
        input.set_visible(false);
        input.handle_key(key::KEY_W, true);
        input.update();
        assert!(!input.action("forward").down);
    }

    #[test]
    fn pointer_motion_accumulates_per_frame_and_stops_while_suspended() {
        let mut input = manager();
        input.handle_pointer_motion(Vec2::new(3.0, -1.0));
        input.handle_pointer_motion(Vec2::new(2.0, 4.0));
        assert_eq!(input.pointer_motion(), Vec2::ZERO, "motion reads after the next update");
        input.update();
        assert_eq!(input.pointer_motion(), Vec2::new(5.0, 3.0));
        input.update();
        assert_eq!(input.pointer_motion(), Vec2::ZERO, "one frame consumes its motion");

        input.handle_pointer_motion(Vec2::new(1.0, 1.0));
        input.clear();
        input.update();
        assert_eq!(input.pointer_motion(), Vec2::ZERO, "clearing drops pending motion");
        input.set_focused(false);
        input.handle_pointer_motion(Vec2::new(1.0, 1.0));
        input.update();
        assert_eq!(input.pointer_motion(), Vec2::ZERO, "suspended input ignores motion");
        input.set_focused(true);
        input.handle_pointer_motion(Vec2::new(f32::NAN, 1.0));
        input.update();
        assert_eq!(input.pointer_motion(), Vec2::ZERO, "non-finite motion is rejected");

        assert!(!input.pointer_locked());
        input.set_pointer_locked(true);
        assert!(input.pointer_locked());
    }

    #[test]
    fn winit_key_names_match_dom_codes() {
        use winit::keyboard::KeyCode;
        assert_eq!(key_code_name(KeyCode::KeyW), key::KEY_W);
        assert_eq!(key_code_name(KeyCode::ArrowUp), key::ARROW_UP);
        assert_eq!(key_code_name(KeyCode::Space), key::SPACE);
        assert_eq!(key_code_name(KeyCode::Escape), key::ESCAPE);
        assert_eq!(key_code_name(KeyCode::Backquote), key::BACKQUOTE);
        assert_eq!(key_code_name(KeyCode::Digit1), key::DIGIT1);
        assert_eq!(key_code_name(KeyCode::ShiftLeft), key::SHIFT_LEFT);
    }
}
