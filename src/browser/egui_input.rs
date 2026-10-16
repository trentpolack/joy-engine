// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Browser translation of winit window events into egui input.
//!
//! egui-winit does not build for WebAssembly in the pinned release, so browsers use this small translator. It covers
//! what HUDs and development panels need: pointer and touch presses, wheel scrolling, keys, text entry, clipboard
//! copy, cut, and paste, and focus. Touches drive the primary pointer, so on-screen buttons work on phones.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_time::Instant;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::window::Window;

use crate::runtime::egui_layer::claims_press;

/// A page clipboard listener, kept so it can be removed.
type ClipboardListener = (&'static str, Closure<dyn FnMut(web_sys::ClipboardEvent)>);

/// Accumulates egui events between frames for one browser canvas.
pub(crate) struct WebEguiInput {
    started: Instant,
    pointer: Option<egui::Pos2>,
    modifiers: egui::Modifiers,
    focused: bool,
    cursor_visible: bool,
    max_texture_side: usize,
    events: Vec<egui::Event>,
    /// Copy, cut, and paste events from the page, which winit does not report.
    clipboard: Rc<RefCell<Vec<egui::Event>>>,
    clipboard_listeners: Vec<ClipboardListener>,
}

impl WebEguiInput {
    pub fn new(max_texture_side: usize) -> Self {
        let clipboard: Rc<RefCell<Vec<egui::Event>>> = Rc::default();
        let mut clipboard_listeners = Vec::new();
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            for kind in ["copy", "cut", "paste"] {
                let events = clipboard.clone();
                let listener: Closure<dyn FnMut(web_sys::ClipboardEvent)> =
                    Closure::new(move |event: web_sys::ClipboardEvent| {
                        let event = match kind {
                            "copy" => egui::Event::Copy,
                            "cut" => egui::Event::Cut,
                            _ => match event.clipboard_data().and_then(|data| data.get_data("text/plain").ok()) {
                                Some(text) if !text.is_empty() => egui::Event::Paste(text),
                                _ => return,
                            },
                        };
                        events.borrow_mut().push(event);
                    });
                if document.add_event_listener_with_callback(kind, listener.as_ref().unchecked_ref()).is_ok() {
                    clipboard_listeners.push((kind, listener));
                }
            }
        }
        Self {
            started: Instant::now(),
            pointer: None,
            modifiers: egui::Modifiers::default(),
            focused: true,
            cursor_visible: true,
            max_texture_side,
            events: Vec::new(),
            clipboard,
            clipboard_listeners,
        }
    }

    /// Carry out egui's platform requests: cursor visibility, clipboard writes, and opening links.
    pub fn handle_platform_output(&mut self, canvas_window: &Window, output: egui::PlatformOutput) {
        // A hidden cursor (for example, while a camera flies) is the only cursor request applied; others keep the page's.
        let cursor_visible = output.cursor_icon != egui::CursorIcon::None;
        if cursor_visible != self.cursor_visible {
            canvas_window.set_cursor_visible(cursor_visible);
            self.cursor_visible = cursor_visible;
        }

        let Some(window) = web_sys::window() else {
            return;
        };
        for command in output.commands {
            match command {
                // The asynchronous write needs a secure context (HTTPS or localhost); failures leave the clipboard as is.
                egui::OutputCommand::CopyText(text) => {
                    let _ = window.navigator().clipboard().write_text(&text);
                }
                egui::OutputCommand::OpenUrl(url) => {
                    let target = if url.new_tab { "_blank" } else { "_self" };
                    let _ = window.open_with_url_and_target(&url.url, target);
                }
                egui::OutputCommand::CopyImage(_) => {}
            }
        }
    }

    /// Record a window event. Returns whether egui currently wants that kind of input, so gameplay can ignore it.
    pub fn on_window_event(&mut self, context: &egui::Context, window: &Window, event: &WindowEvent) -> bool {
        // winit reports pointer positions in CSS pixels times its own ratio; dividing by it yields points.
        let pixels_per_point = window.scale_factor() as f32;
        let to_points = |x: f64, y: f64| egui::pos2(x as f32 / pixels_per_point, y as f32 / pixels_per_point);

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let point = to_points(position.x, position.y);
                self.pointer = Some(point);
                self.events.push(egui::Event::PointerMoved(point));
                false
            }
            WindowEvent::CursorLeft { .. } => {
                self.pointer = None;
                self.events.push(egui::Event::PointerGone);
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let (Some(position), Some(button)) = (self.pointer, pointer_button(*button)) else {
                    return false;
                };
                let pressed = *state == ElementState::Pressed;
                self.events.push(egui::Event::PointerButton {
                    pos: position,
                    button,
                    pressed,
                    modifiers: self.modifiers,
                });
                if pressed { claims_press(context, position) } else { context.egui_wants_pointer_input() }
            }
            WindowEvent::Touch(touch) => {
                let point = to_points(touch.location.x, touch.location.y);
                self.pointer = Some(point);
                self.events.push(egui::Event::PointerMoved(point));
                let pressed = match touch.phase {
                    TouchPhase::Started => Some(true),
                    TouchPhase::Ended | TouchPhase::Cancelled => Some(false),
                    TouchPhase::Moved => None,
                };
                if let Some(pressed) = pressed {
                    self.events.push(egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: self.modifiers,
                    });
                    if !pressed {
                        self.events.push(egui::Event::PointerGone);
                    }
                }
                if pressed == Some(true) { claims_press(context, point) } else { context.egui_wants_pointer_input() }
            }
            WindowEvent::MouseWheel { delta, phase, .. } => {
                let (unit, delta) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (egui::MouseWheelUnit::Line, egui::vec2(*x, *y)),
                    MouseScrollDelta::PixelDelta(position) => (
                        egui::MouseWheelUnit::Point,
                        egui::vec2(position.x as f32, position.y as f32) / pixels_per_point,
                    ),
                };
                let phase = match phase {
                    TouchPhase::Started => egui::TouchPhase::Start,
                    TouchPhase::Moved => egui::TouchPhase::Move,
                    TouchPhase::Ended => egui::TouchPhase::End,
                    TouchPhase::Cancelled => egui::TouchPhase::Cancel,
                };
                self.events.push(egui::Event::MouseWheel { unit, delta, phase, modifiers: self.modifiers });
                context.egui_wants_pointer_input()
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modifiers = egui::Modifiers {
                    alt: state.alt_key(),
                    ctrl: state.control_key(),
                    shift: state.shift_key(),
                    mac_cmd: false,
                    command: state.control_key() || state.super_key(),
                };
                false
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                let key = match &event.physical_key {
                    winit::keyboard::PhysicalKey::Code(code) => {
                        egui::Key::from_name(&format!("{code:?}").replace("Key", ""))
                    }
                    winit::keyboard::PhysicalKey::Unidentified(_) => None,
                };
                if let Some(key) = key {
                    self.events.push(egui::Event::Key {
                        key,
                        physical_key: Some(key),
                        pressed,
                        repeat: false,
                        modifiers: self.modifiers,
                    });
                }
                if pressed
                    && let Some(text) = &event.text
                    && text.chars().all(|character| !character.is_control())
                {
                    self.events.push(egui::Event::Text(text.to_string()));
                }
                context.egui_wants_keyboard_input()
            }
            WindowEvent::Focused(focused) => {
                self.focused = *focused;
                self.events.push(egui::Event::WindowFocused(*focused));
                false
            }
            _ => false,
        }
    }

    /// Build this frame's raw input from the accumulated events.
    pub fn take_input(&mut self, window: &Window) -> egui::RawInput {
        // One point is one CSS pixel; painting uses the canvas's actual backing ratio (see `shell::display_scale`).
        let pixels_per_point = crate::runtime::shell::display_scale(window);
        let size = window.inner_size();
        let screen = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(size.width as f32 / pixels_per_point, size.height as f32 / pixels_per_point),
        );
        self.events.append(&mut self.clipboard.borrow_mut());
        let mut input = egui::RawInput {
            screen_rect: Some(screen),
            max_texture_side: Some(self.max_texture_side),
            time: Some(self.started.elapsed().as_secs_f64()),
            events: std::mem::take(&mut self.events),
            focused: self.focused,
            ..Default::default()
        };
        if let Some(viewport) = input.viewports.get_mut(&egui::ViewportId::ROOT) {
            viewport.native_pixels_per_point = Some(pixels_per_point);
        } else {
            let info = egui::ViewportInfo { native_pixels_per_point: Some(pixels_per_point), ..Default::default() };
            input.viewports.insert(egui::ViewportId::ROOT, info);
        }
        input
    }
}

impl Drop for WebEguiInput {
    fn drop(&mut self) {
        if let Some(document) = web_sys::window().and_then(|window| window.document()) {
            for (kind, listener) in self.clipboard_listeners.drain(..) {
                let _ = document.remove_event_listener_with_callback(kind, listener.as_ref().unchecked_ref());
            }
        }
    }
}

/// egui's button for a winit mouse button.
fn pointer_button(button: MouseButton) -> Option<egui::PointerButton> {
    match button {
        MouseButton::Left => Some(egui::PointerButton::Primary),
        MouseButton::Right => Some(egui::PointerButton::Secondary),
        MouseButton::Middle => Some(egui::PointerButton::Middle),
        MouseButton::Back => Some(egui::PointerButton::Extra1),
        MouseButton::Forward => Some(egui::PointerButton::Extra2),
        MouseButton::Other(_) => None,
    }
}
