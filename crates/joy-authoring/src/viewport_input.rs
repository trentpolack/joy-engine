// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Camera gestures for editor viewports: one captured drag that orbits with the
//! primary button and pans with Shift or any other button, wheel zoom in page pixels, a click without movement, and
//! shortcuts (F frames, 0 resets, arrows orbit) while the viewport has keyboard focus. Pressing the viewport focuses
//! it, so its keys never reach text fields or panels.
//!
//! The reader owns only the gesture in progress; cameras and authored state stay with the caller, which maps the
//! reported actions onto its camera.

use egui::{
    Event,
    EventFilter,
    Id,
    Key,
    Modifiers,
    MouseWheelUnit,
    PointerButton,
    Pos2,
    Rect,
    Response,
    Sense,
    Ui,
    Vec2,
};

/// Wheel line height in pixels (browsers' `deltaMode` 1).
const LINE_HEIGHT_PIXELS: f32 = 16.0;

/// What a drag does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportGesture {
    /// Primary-button drag without Shift: orbit.
    Orbit,
    /// Shift + primary drag, or a secondary or middle drag: pan.
    Pan,
}

/// A viewport keyboard shortcut (pressed without modifiers while the viewport is focused).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportShortcut {
    /// F: frame the content.
    Frame,
    /// 0: reset the camera.
    Reset,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
}

/// One frame's viewport input.
#[derive(Clone, Debug)]
pub struct ViewportActions {
    /// The viewport's response (hover, focus, rect).
    pub response: Response,
    /// The captured drag's mode and this frame's movement in logical pixels.
    pub drag: Option<(ViewportGesture, Vec2)>,
    /// Wheel movement in page pixels; positive (the DOM `deltaY` sign) when scrolling toward the author, zooming out.
    pub zoom: f32,
    /// Shortcuts pressed this frame.
    pub shortcuts: Vec<ViewportShortcut>,
    /// A primary click without modifiers or movement, in logical pixels.
    pub click: Option<Pos2>,
}

/// Read camera gestures for the viewport covering `rect`, identified by `id`.
pub fn viewport_input(ui: &mut Ui, rect: Rect, id: Id) -> ViewportActions {
    let response = ui.interact(rect, id, Sense::click_and_drag());
    let gesture_id = id.with("gesture");
    if response.is_pointer_button_down_on() && !response.has_focus() {
        response.request_focus();
    }

    // The gesture's mode is fixed when the drag begins, like a captured pointer.
    if response.drag_started() {
        let shift = ui.input(|input| input.modifiers.shift);
        let mode = if response.dragged_by(PointerButton::Primary) && !shift {
            ViewportGesture::Orbit
        } else {
            ViewportGesture::Pan
        };
        ui.data_mut(|data| data.insert_temp(gesture_id, mode));
    }
    let mode: Option<ViewportGesture> = ui.data(|data| data.get_temp(gesture_id));
    let drag = match mode {
        Some(mode) if response.dragged() => Some((mode, response.drag_delta())),
        _ => None,
    };
    if response.drag_stopped() || (!response.dragged() && mode.is_some()) {
        ui.data_mut(|data| data.remove::<ViewportGesture>(gesture_id));
    }

    let page_height = ui.ctx().content_rect().height();
    let zoom = if response.hovered() {
        ui.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                        ..
                    } if !modifiers.ctrl && !modifiers.command => {
                        let scale = match unit {
                            MouseWheelUnit::Point => 1.0,
                            MouseWheelUnit::Line => LINE_HEIGHT_PIXELS,
                            MouseWheelUnit::Page => page_height,
                        };
                        Some(-delta.y * scale)
                    }
                    _ => None,
                })
                .sum()
        })
    } else {
        0.0
    };

    let mut shortcuts = Vec::new();
    if response.has_focus() {
        ui.memory_mut(|memory| {
            memory.set_focus_lock_filter(
                id,
                EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                    tab: false,
                },
            )
        });
        ui.input_mut(|input| {
            for (key, shortcut) in [
                (Key::F, ViewportShortcut::Frame),
                (Key::Num0, ViewportShortcut::Reset),
                (Key::ArrowLeft, ViewportShortcut::Left),
                (Key::ArrowRight, ViewportShortcut::Right),
                (Key::ArrowUp, ViewportShortcut::Up),
                (Key::ArrowDown, ViewportShortcut::Down),
            ] {
                // Frame and reset ignore auto-repeat; arrows keep orbiting while held.
                let pressed = input.events.iter().any(|event| {
                    matches!(event, Event::Key { key: pressed_key, pressed: true, repeat, modifiers, .. }
                        if *pressed_key == key && *modifiers == Modifiers::NONE
                            && (!*repeat || matches!(shortcut, ViewportShortcut::Left | ViewportShortcut::Right | ViewportShortcut::Up | ViewportShortcut::Down)))
                });
                if pressed {
                    input.consume_key(Modifiers::NONE, key);
                    shortcuts.push(shortcut);
                }
            }
        });
    }

    let plain = ui.input(|input| input.modifiers.is_none());
    let click =
        (response.clicked_by(PointerButton::Primary) && plain).then(|| response.interact_pointer_pos()).flatten();
    ViewportActions {
        response,
        drag,
        zoom,
        shortcuts,
        click,
    }
}

#[cfg(test)]
#[path = "viewport_input.test.rs"]
mod tests;
