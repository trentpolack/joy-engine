// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared egui controls for authoring tools: exact number fields with drag scrubbing and Shift/Alt keyboard steps,
//! bounded scalars (slider plus
//! exact field), XYZ and RGBA component groups, a linear-RGBA color control with an sRGB picker, and default/modified
//! indication with per-field reset.
//!
//! Controls borrow authored values (`&mut f64`, `&mut [f64]`) and own no document state. Each returns a
//! [`ControlResponse`] whose [`EditPhase`] tells the tool when to record [`PropertyHistory`] entries: slider drags and
//! number scrubs report `Continuing` while they move and `Committed` once on release; exact entries, arrow steps,
//! resets, and picker choices commit immediately.
//!
//! A draft the author types is validated on Enter or when the field loses focus. A rejected draft never changes the
//! value: it stays visible in the field with its message until repaired (a valid entry, a slider move, a reset, or
//! Escape back to the accepted value), and [`NumericDrafts`] reports it so tools refuse Apply, export, navigation, or a
//! picker choice that would discard it. Displaying a value never rounds it: number text is
//! exact ([`format_number`]), sliders do not clamp values they did not edit, and opening or refreshing the color picker
//! does not write back.
//!
//! The mouse wheel never changes a number; egui scrolls the panel instead.
//!
//! [`PropertyHistory`]: crate::PropertyHistory
//! [`format_number`]: crate::lab_values::format_number

use indexmap::IndexMap;

use egui::{
    Align,
    Align2,
    Color32,
    CornerRadius,
    FontId,
    Id,
    Key,
    Pos2,
    Rect,
    Response,
    Sense,
    Stroke,
    StrokeKind,
    TextEdit,
    Ui,
    Vec2,
    pos2,
    vec2,
};

use joy_ui::theme::tokens;

use crate::lab_values::{
    self,
    NumericRules,
    REPAIR_DRAFTS_MESSAGE,
    StepScale,
    format_number,
    parse_number,
    stepped_value,
    validate_draft,
};

/// Default exact-field width: the lab grid's value column.
pub const NUMBER_FIELD_WIDTH: f32 = 76.0;

/// Width of a bounded scalar's leading label column.
pub const SCALAR_LABEL_WIDTH: f32 = 64.0;

/// Side of the color swatch.
pub const COLOR_SWATCH_SIZE: f32 = 30.0;

/// Height of the sliders under component fields.
const COMPONENT_SLIDER_HEIGHT: f32 = 24.0;

/// Side of the reset button.
const RESET_BUTTON_SIZE: f32 = 20.0;

/// Checkerboard cell size behind swatches.
const CHECKER_CELL: f32 = 8.0;

/// Checkerboard colors behind translucent swatches.
const CHECKER_LIGHT: Color32 = Color32::from_rgb(0x6f, 0x7c, 0x8f);
const CHECKER_DARK: Color32 = Color32::from_rgb(0x26, 0x37, 0x4d);

/// Hover help for exact fields.
const NUMBER_FIELD_HINT: &str = "Type directly · Shift + arrows fine · Alt + arrows coarse · drag to scrub";

/// Hover help for color swatches.
const COLOR_PICKER_HINT: &str = "Choose sRGB color · precise channels are linear · alpha is preserved";

/// Display-space change below which a picker report is floating-point noise rather than a choice.
const PICKER_CHANGE_EPSILON: f64 = 1e-4;

// ---------------------------------------------------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------------------------------------------------

/// Where a control's interaction stands with respect to property history.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum EditPhase {
    /// No edit this frame.
    #[default]
    Idle,
    /// A continuous gesture (slider drag, number scrub, picker drag) moved the value and is still in progress; do not
    /// record history yet.
    Continuing,
    /// A gesture ended or a discrete edit happened; record history now
    /// ([`PropertyHistory::commit`](crate::PropertyHistory::commit)).
    Committed,
}

/// What a lab control did this frame.
#[derive(Clone, Debug)]
pub struct ControlResponse {
    /// The egui response covering the control.
    pub response: Response,
    /// Whether an authored value changed this frame.
    pub changed: bool,
    /// History phase of the interaction.
    pub phase: EditPhase,
    /// The component that changed, for groups (`None` for single values or no change).
    pub component: Option<usize>,
}

impl ControlResponse {
    fn idle(response: Response) -> Self {
        Self { response, changed: false, phase: EditPhase::Idle, component: None }
    }

    /// Whether history should record now.
    pub fn committed(&self) -> bool {
        self.phase == EditPhase::Committed
    }

    /// Combine with another part of the same control.
    fn merge(mut self, other: ControlResponse) -> Self {
        self.response = self.response.union(other.response);
        self.changed |= other.changed;
        self.phase = self.phase.max(other.phase);
        self.component = self.component.or(other.component);
        self
    }

    fn with_component(mut self, component: usize) -> Self {
        if self.changed || self.phase != EditPhase::Idle {
            self.component = Some(component);
        }
        self
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Drafts
// ---------------------------------------------------------------------------------------------------------------------

/// A numeric draft that is not an acceptable value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejectedDraft {
    /// The text as typed.
    pub text: String,
    /// Why it was rejected.
    pub message: String,
    /// Whether the author confirmed it (Enter or leaving the field); unconfirmed drafts are still being typed.
    pub confirmed: bool,
}

/// One tool's invalid numeric drafts, keyed by field. Tools own one per document view, pass it to every numeric
/// control, and check [`require_valid`](Self::require_valid) before actions that would replace or discard drafts
/// (Apply, export, switching emitters, loading examples, picker choices).
#[derive(Clone, Debug, Default)]
pub struct NumericDrafts {
    rejected: IndexMap<Id, RejectedDraft>,
}

impl NumericDrafts {
    /// Whether any field holds an invalid draft (confirmed or still being typed).
    pub fn has_rejected(&self) -> bool {
        !self.rejected.is_empty()
    }

    /// `Ok` when every draft is valid; otherwise the message to show the author.
    pub fn require_valid(&self) -> Result<(), String> {
        if self.has_rejected() { Err(REPAIR_DRAFTS_MESSAGE.to_owned()) } else { Ok(()) }
    }

    /// The rejected draft of the field with `id`, if any.
    pub fn rejected(&self, id: Id) -> Option<&RejectedDraft> {
        self.rejected.get(&id)
    }

    /// Every rejected draft's message, in field order, for status lines.
    pub fn messages(&self) -> Vec<&str> {
        self.rejected.values().map(|draft| draft.message.as_str()).collect()
    }

    /// Drop every draft, when the document they belong to is replaced.
    pub fn clear(&mut self) {
        self.rejected.clear();
    }

    /// Drop one field's draft, when its value was replaced another way.
    pub fn discard(&mut self, id: Id) {
        self.rejected.shift_remove(&id);
    }

    fn reject(&mut self, id: Id, text: String, message: String, confirmed: bool) {
        self.rejected.insert(id, RejectedDraft { text, message, confirmed });
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Exact number field
// ---------------------------------------------------------------------------------------------------------------------

/// Per-field interaction memory kept in egui's temporary storage.
#[derive(Clone, Debug, Default)]
struct FieldMemory {
    /// Draft text while the field is being typed into.
    editing: Option<String>,
    /// Typing begins next frame: focus is requested then, after the click that started it has been processed (a
    /// click's own frame would otherwise surrender the new focus).
    focus_pending: bool,
    /// Value when a scrub began.
    scrub_start: Option<f64>,
    /// Escape cancelled the scrub; ignore movement until release.
    scrub_cancelled: bool,
}

/// An extra acceptance rule for a field, returning a message for unacceptable values (for example a minimum that would
/// exceed its maximum).
pub type ValueCheck<'a> = Box<dyn Fn(f64) -> Result<(), String> + 'a>;

/// An exact numeric field: shows the authored value exactly, scrubs by dragging, steps with arrow keys (Shift fine,
/// Alt coarse), and keeps rejected drafts visible until repaired.
pub struct NumberField<'a> {
    id_salt: Id,
    value: &'a mut f64,
    rules: NumericRules,
    step: f64,
    default: Option<f64>,
    width: f32,
    align: Align,
    label: Option<String>,
    check: Option<ValueCheck<'a>>,
}

impl<'a> NumberField<'a> {
    /// A field for `value`, identified by `id_salt` within the parent `Ui` (use a stable name such as a parameter
    /// name, so drafts follow the field when rows are added or removed).
    pub fn new(id_salt: impl egui::AsId, value: &'a mut f64) -> Self {
        Self {
            id_salt: Id::new(id_salt),
            value,
            rules: NumericRules::default(),
            step: 0.01,
            default: None,
            width: NUMBER_FIELD_WIDTH,
            align: Align::Max,
            label: None,
            check: None,
        }
    }

    /// Accept only values within `minimum..=maximum`.
    pub fn range(mut self, minimum: f64, maximum: f64) -> Self {
        self.rules.minimum = Some(minimum);
        self.rules.maximum = Some(maximum);
        self
    }

    /// Accept only values of at least `minimum`.
    pub fn minimum(mut self, minimum: f64) -> Self {
        self.rules.minimum = Some(minimum);
        self
    }

    /// Accept only values of at most `maximum`.
    pub fn maximum(mut self, maximum: f64) -> Self {
        self.rules.maximum = Some(maximum);
        self
    }

    /// Use complete rules.
    pub fn rules(mut self, rules: NumericRules) -> Self {
        self.rules = rules;
        self
    }

    /// Accept whole numbers only (Shift steps then move by one).
    pub fn integer(mut self) -> Self {
        self.rules.integer = true;
        if self.step < 1.0 {
            self.step = 1.0;
        }
        self
    }

    /// The keyboard and scrub step (default 0.01).
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// The value's default: differing values draw in the accent color and offer a reset button.
    pub fn default_value(mut self, default: Option<f64>) -> Self {
        self.default = default;
        self
    }

    /// Field width in points (default [`NUMBER_FIELD_WIDTH`]).
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Left-align the text (component fields); fields right-align by default.
    pub fn align_left(mut self) -> Self {
        self.align = Align::Min;
        self
    }

    /// Name for hover help and accessibility, for example `power value`.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// An extra rule beyond bounds and integrality.
    pub fn check(mut self, check: impl Fn(f64) -> Result<(), String> + 'a) -> Self {
        self.check = Some(Box::new(check));
        self
    }

    /// The field's id in `ui`, for [`NumericDrafts::rejected`] and tests.
    pub fn id_in(ui: &Ui, id_salt: impl egui::AsId) -> Id {
        ui.make_persistent_id(Id::new(id_salt))
    }

    /// Draw the field and apply the author's edits.
    pub fn show(self, ui: &mut Ui, drafts: &mut NumericDrafts) -> ControlResponse {
        let NumberField { id_salt, value, rules, step, default, width, align, label, check } = self;
        let spec = FieldSpec { rules, step, width, align };
        let id = ui.make_persistent_id(id_salt);
        let text_id = id.with("text");
        let mut memory: FieldMemory = ui.data(|data| data.get_temp(id)).unwrap_or_default();
        let height = ui.spacing().interact_size.y;
        let size = vec2(width, height);
        let modified = is_modified(*value, default);
        let validate = |text: &str| -> Result<f64, String> {
            let value = validate_draft(text, &rules)?;
            match &check {
                Some(check) => check(value).map(|()| value),
                None => Ok(value),
            }
        };

        let mut result = None;
        if memory.editing.is_none() {
            let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
            let rejected = drafts.rejected(id).cloned();
            let text = rejected.as_ref().map_or_else(|| format_number(*value), |draft| draft.text.clone());
            let color = text_color(rejected.is_some(), modified);
            let focused = response.has_focus() || response.hovered() || response.dragged();
            paint_field(ui, rect, focused, rejected.is_some(), &text, color, align);
            let mut control = ControlResponse::idle(response.clone());

            // Scrubbing: horizontal drags step the value from where the drag began.
            if response.drag_started() {
                memory.scrub_start = Some(*value);
                memory.scrub_cancelled = false;
            }
            if response.dragged()
                && !memory.scrub_cancelled
                && let Some(start) = memory.scrub_start
            {
                if ui.input(|input| input.key_pressed(Key::Escape)) {
                    memory.scrub_cancelled = true;
                    if *value != start {
                        *value = start;
                        control.changed = true;
                    }
                    control.phase = EditPhase::Committed;
                } else {
                    let (origin, current, modifiers) =
                        ui.input(|input| (input.pointer.press_origin(), input.pointer.interact_pos(), input.modifiers));
                    if let (Some(origin), Some(current)) = (origin, current) {
                        let scale = StepScale::from_modifiers(modifiers);
                        let next = lab_values::scrubbed_value(start, current.x - origin.x, step, scale, &rules);
                        if next != *value && validate(&format_number(next)).is_ok() {
                            *value = next;
                            control.changed = true;
                            control.phase = EditPhase::Continuing;
                            drafts.discard(id);
                        }
                    }
                }
            }
            if response.drag_stopped() && memory.scrub_start.take().is_some() {
                control.phase = EditPhase::Committed;
            }

            // A click (or keyboard focus from Tab) starts typing, beginning from any rejected draft.
            let pointer_down = ui.input(|input| input.pointer.any_down());
            if response.clicked() || (response.gained_focus() && !pointer_down) {
                memory.editing = Some(text.clone());
                memory.focus_pending = true;
                ui.ctx().request_repaint();
            }
            let hint = match (&label, &rejected) {
                (_, Some(draft)) => draft.message.clone(),
                (Some(label), None) => format!("{label} · {NUMBER_FIELD_HINT}"),
                (None, None) => NUMBER_FIELD_HINT.to_owned(),
            };
            let name = label.clone().unwrap_or_else(|| "Value".to_owned());
            let shown = parse_number(&text);
            control.response.widget_info(|| {
                let mut info = egui::WidgetInfo::labeled(egui::WidgetType::DragValue, ui.is_enabled(), &name);
                info.value = shown;
                info
            });
            let response = control.response.clone().on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            control.response = response.on_hover_text(hint);
            if let Some(draft) = rejected.filter(|draft| draft.confirmed) {
                show_draft_error(ui, &draft.message, width);
            }
            result = Some(control);
        }

        if memory.editing.is_some() && result.is_none() {
            result = Some(show_editing(ui, drafts, id, text_id, &mut memory, value, spec, modified, &validate));
        }
        ui.data_mut(|data| data.insert_temp(id, memory));
        result.expect("one of the two modes drew the field")
    }
}

/// Layout and stepping settings shared by a field's two modes.
#[derive(Clone, Copy)]
struct FieldSpec {
    rules: NumericRules,
    step: f64,
    width: f32,
    align: Align,
}

/// The typing mode: a single-line text edit with keyboard steps and validation on Enter or focus loss.
#[allow(clippy::too_many_arguments)]
fn show_editing(
    ui: &mut Ui,
    drafts: &mut NumericDrafts,
    id: Id,
    text_id: Id,
    memory: &mut FieldMemory,
    value: &mut f64,
    spec: FieldSpec,
    modified: bool,
    validate: &dyn Fn(&str) -> Result<f64, String>,
) -> ControlResponse {
    let size = vec2(spec.width, ui.spacing().interact_size.y);
    let mut draft = memory.editing.take().unwrap_or_default();
    if std::mem::take(&mut memory.focus_pending) {
        ui.memory_mut(|memory| memory.request_focus(text_id));
        select_all(ui.ctx(), text_id, draft.chars().count());
    }
    let focused = ui.memory(|memory| memory.has_focus(text_id));
    let mut changed = false;
    let mut phase = EditPhase::Idle;

    // Escape returns to the accepted value and drops the draft.
    if focused && ui.input(|input| input.key_pressed(Key::Escape)) {
        drafts.discard(id);
        ui.memory_mut(|memory| memory.surrender_focus(text_id));
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        paint_field(ui, rect, false, false, &format_number(*value), text_color(false, modified), spec.align);
        return ControlResponse::idle(response);
    }

    // Arrow steps commit immediately, before the text edit sees the keys.
    if focused {
        let presses = ui.input_mut(|input| {
            let mut presses = Vec::new();
            input.events.retain(|event| match event {
                egui::Event::Key { key: key @ (Key::ArrowUp | Key::ArrowDown), pressed: true, modifiers, .. }
                    if !modifiers.command && !modifiers.ctrl =>
                {
                    presses
                        .push((if *key == Key::ArrowUp { 1.0 } else { -1.0 }, StepScale::from_modifiers(*modifiers)));
                    false
                }
                _ => true,
            });
            presses
        });
        for (direction, scale) in presses {
            let Some(current) = parse_number(&draft) else {
                continue;
            };
            let next = stepped_value(current, spec.step, scale, direction, &spec.rules);
            draft = format_number(next);
            match validate(&draft) {
                Ok(accepted) => {
                    if accepted != *value {
                        *value = accepted;
                        changed = true;
                    }
                    phase = EditPhase::Committed;
                    drafts.discard(id);
                }
                Err(message) => drafts.reject(id, draft.clone(), message, true),
            }
            select_all(ui.ctx(), text_id, draft.chars().count());
        }
    }

    let previous = drafts.rejected(id).cloned();
    let showing_rejection = previous.as_ref().is_some_and(|rejected| rejected.confirmed && rejected.text == draft);
    let color = text_color(showing_rejection, modified);
    let entered = focused && ui.input(|input| input.key_pressed(Key::Enter));
    let output = TextEdit::singleline(&mut draft)
        .id(text_id)
        .font(FontId::monospace(12.0))
        .text_color(color)
        .horizontal_align(spec.align)
        .margin(vec2(tokens::SPACE_XS, 2.0))
        .min_size(size)
        .desired_width(size.x - (2.0 * tokens::SPACE_XS))
        .show(ui);
    let response = output.response.response.clone();

    // Enter, leaving the field, or a field that never received focus confirms the draft.
    if entered || response.lost_focus() || !focused {
        match validate(&draft) {
            Ok(accepted) => {
                if accepted != *value {
                    *value = accepted;
                    changed = true;
                }
                phase = EditPhase::Committed;
                drafts.discard(id);
            }
            Err(message) => {
                drafts.reject(id, draft.clone(), message, true);
                if entered {
                    // Keep typing after a rejected Enter, as a browser number field keeps focus.
                    ui.memory_mut(|memory| memory.request_focus(text_id));
                    memory.editing = Some(draft);
                }
            }
        }
    } else {
        // While typing, invalid text counts as a pending draft (shown once confirmed) so actions still refuse it.
        match validate(&draft) {
            Ok(_) => drafts.discard(id),
            Err(message) => {
                let confirmed = previous.as_ref().is_some_and(|rejected| rejected.confirmed && rejected.text == draft);
                drafts.reject(id, draft.clone(), message, confirmed);
            }
        }
        memory.editing = Some(draft);
    }
    if let Some(rejected) = drafts.rejected(id).filter(|rejected| rejected.confirmed).cloned() {
        show_draft_error(ui, &rejected.message, size.x);
    }
    ControlResponse { response, changed, phase, component: None }
}

// ---------------------------------------------------------------------------------------------------------------------
// Bounded scalar and component groups
// ---------------------------------------------------------------------------------------------------------------------

/// A bounded scalar: a slider for the authored range paired with an exact field, an optional leading label, and a reset
/// button while the value differs from its default. The slider appears only when both bounds are finite and distinct.
pub struct ScalarControl<'a> {
    id_salt: Id,
    value: &'a mut f64,
    rules: NumericRules,
    step: f64,
    slider_step: Option<f64>,
    default: Option<f64>,
    label: Option<String>,
    name: Option<String>,
    slider: bool,
    field_width: f32,
}

impl<'a> ScalarControl<'a> {
    /// A control for `value`, identified by `id_salt` within the parent `Ui`.
    pub fn new(id_salt: impl egui::AsId, value: &'a mut f64) -> Self {
        Self {
            id_salt: Id::new(id_salt),
            value,
            rules: NumericRules::default(),
            step: 0.01,
            slider_step: None,
            default: None,
            label: None,
            name: None,
            slider: true,
            field_width: NUMBER_FIELD_WIDTH,
        }
    }

    /// Inclusive bounds; they validate the exact field and span the slider.
    pub fn range(mut self, minimum: f64, maximum: f64) -> Self {
        self.rules.minimum = Some(minimum);
        self.rules.maximum = Some(maximum);
        self
    }

    /// Complete rules.
    pub fn rules(mut self, rules: NumericRules) -> Self {
        self.rules = rules;
        self
    }

    /// Whole numbers only.
    pub fn integer(mut self) -> Self {
        self.rules.integer = true;
        self.step = self.step.max(1.0);
        self
    }

    /// Keyboard and scrub step for the exact field.
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// Snap slider drags to multiples of `step` from the minimum (continuous by default).
    pub fn slider_step(mut self, step: f64) -> Self {
        self.slider_step = Some(step);
        self
    }

    /// The default value for modified indication and reset.
    pub fn default_value(mut self, default: Option<f64>) -> Self {
        self.default = default;
        self
    }

    /// A leading label in the muted label column.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Accessible name without a visible label: the field becomes `<name> value` and the slider `<name> slider`
    /// (the label, when given, is the default name).
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Hide the slider even when bounded.
    pub fn without_slider(mut self) -> Self {
        self.slider = false;
        self
    }

    /// Width of the exact field.
    pub fn field_width(mut self, width: f32) -> Self {
        self.field_width = width;
        self
    }

    /// Draw the control and apply edits.
    pub fn show(self, ui: &mut Ui, drafts: &mut NumericDrafts) -> ControlResponse {
        let Self { id_salt, value, rules, step, slider_step, default, label, name, slider, field_width } = self;
        let name = name.or_else(|| label.clone());
        ui.push_id(id_salt, |ui| {
            ui.horizontal(|ui| {
                let field_id = NumberField::id_in(ui, "value");
                let modified = is_modified(*value, default);
                let mut combined: Option<ControlResponse> = None;
                if let Some(label) = &label {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(SCALAR_LABEL_WIDTH, ui.spacing().interact_size.y), Sense::hover());
                    paint_label(ui, rect, label, tokens::MUTED, tokens::TYPE_SM);
                }
                let reset_width = if default.is_some() { RESET_BUTTON_SIZE + ui.spacing().item_spacing.x } else { 0.0 };
                if slider && let Some(range) = rules.slider_range() {
                    let spacing = ui.spacing().item_spacing.x;
                    let slider_width = (ui.available_width() - field_width - reset_width - spacing).max(24.0);
                    let response = slider_control(ui, value, range, slider_step, slider_width, None);
                    if let Some(name) = &name {
                        let current = *value;
                        response
                            .response
                            .widget_info(|| egui::WidgetInfo::slider(true, current, format!("{name} slider")));
                    }
                    if response.changed {
                        drafts.discard(field_id);
                    }
                    combined = Some(response);
                }
                let field = NumberField::new("value", value)
                    .rules(rules)
                    .step(step)
                    .default_value(default)
                    .width(field_width)
                    .label(name.as_ref().map_or_else(|| "Value".to_owned(), |name| format!("{name} value")))
                    .show(ui, drafts);
                let mut combined = match combined {
                    Some(slider) => slider.merge(field),
                    None => field,
                };
                if let Some(default) = default
                    && reset_button(ui, modified, default).clicked()
                    && modified
                {
                    *value = default;
                    drafts.discard(field_id);
                    combined.changed = true;
                    combined.phase = EditPhase::Committed;
                }
                combined
            })
            .inner
        })
        .inner
    }
}

/// Grouped components edited together: XYZ vectors and RGBA colors. Each component has a muted label, an exact field,
/// and a short slider when its bounds are finite; per-component defaults add modified indication and reset.
pub struct ComponentGroup<'a> {
    id_salt: Id,
    values: &'a mut [f64],
    labels: &'a [&'a str],
    minimums: Option<&'a [f64]>,
    maximums: Option<&'a [f64]>,
    defaults: Option<&'a [f64]>,
    step: f64,
    slider: bool,
    name: Option<String>,
}

impl<'a> ComponentGroup<'a> {
    /// A group editing `values`, labeled per component (for example `["X", "Y", "Z"]`).
    pub fn new(id_salt: impl egui::AsId, values: &'a mut [f64], labels: &'a [&'a str]) -> Self {
        Self {
            id_salt: Id::new(id_salt),
            values,
            labels,
            minimums: None,
            maximums: None,
            defaults: None,
            step: 0.01,
            slider: true,
            name: None,
        }
    }

    /// An XYZ vector group.
    pub fn xyz(id_salt: impl egui::AsId, values: &'a mut [f64]) -> Self {
        Self::new(id_salt, values, &["X", "Y", "Z"])
    }

    /// An RGBA (or RGB) color group.
    pub fn rgba(id_salt: impl egui::AsId, values: &'a mut [f64]) -> Self {
        Self::new(id_salt, values, &["R", "G", "B", "A"])
    }

    /// Per-component inclusive bounds.
    pub fn bounds(mut self, minimums: &'a [f64], maximums: &'a [f64]) -> Self {
        self.minimums = Some(minimums);
        self.maximums = Some(maximums);
        self
    }

    /// Per-component defaults for modified indication and reset.
    pub fn defaults(mut self, defaults: Option<&'a [f64]>) -> Self {
        self.defaults = defaults;
        self
    }

    /// Keyboard and scrub step.
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// Accessible name prefix: component fields become `<name> X value` and sliders `<name> X slider`.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Hide the component sliders.
    pub fn without_sliders(mut self) -> Self {
        self.slider = false;
        self
    }

    /// Draw the group and apply edits; [`ControlResponse::component`] names the component that changed.
    pub fn show(self, ui: &mut Ui, drafts: &mut NumericDrafts) -> ControlResponse {
        let count = self.values.len().max(1);
        let spacing = ui.spacing().item_spacing.x;
        let column_width = ((ui.available_width() - (spacing * (count as f32 - 1.0))) / count as f32).max(36.0);
        ui.push_id(self.id_salt, |ui| {
            ui.horizontal_top(|ui| {
                let mut combined: Option<ControlResponse> = None;
                for (component, value) in self.values.iter_mut().enumerate() {
                    let rules = NumericRules {
                        minimum: self.minimums.and_then(|bounds| bounds.get(component).copied()),
                        maximum: self.maximums.and_then(|bounds| bounds.get(component).copied()),
                        integer: false,
                    };
                    let default = self.defaults.and_then(|defaults| defaults.get(component).copied());
                    let label = self.labels.get(component).copied().unwrap_or("");
                    let accessible = match &self.name {
                        Some(name) => format!("{name} {label}"),
                        None => label.to_owned(),
                    };
                    let response = ui
                        .vertical(|ui| {
                            ui.set_width(column_width);
                            ui.push_id(component, |ui| {
                                component_column(
                                    ui,
                                    value,
                                    label,
                                    &accessible,
                                    rules,
                                    default,
                                    self.step,
                                    self.slider,
                                    column_width,
                                    drafts,
                                )
                            })
                            .inner
                        })
                        .inner
                        .with_component(component);
                    combined = Some(match combined {
                        Some(previous) => previous.merge(response),
                        None => response,
                    });
                }
                combined.expect("groups have at least one component")
            })
            .inner
        })
        .inner
    }
}

/// One component column: label row (with reset), exact field, and optional slider.
#[allow(clippy::too_many_arguments)]
fn component_column(
    ui: &mut Ui,
    value: &mut f64,
    label: &str,
    accessible: &str,
    rules: NumericRules,
    default: Option<f64>,
    step: f64,
    slider: bool,
    width: f32,
    drafts: &mut NumericDrafts,
) -> ControlResponse {
    ui.spacing_mut().item_spacing.y = 2.0;
    let field_id = NumberField::id_in(ui, "value");
    let modified = is_modified(*value, default);
    let mut reset = None;
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(width - RESET_BUTTON_SIZE, 14.0), Sense::hover());
        paint_label(ui, rect, label, tokens::MUTED, tokens::TYPE_XS);
        if let Some(default) = default {
            reset = Some((reset_button(ui, modified, default), default));
        }
    });
    let mut response = NumberField::new("value", value)
        .rules(rules)
        .step(step)
        .default_value(default)
        .width(width)
        .align_left()
        .label(format!("{accessible} value"))
        .show(ui, drafts);
    if slider && let Some(range) = rules.slider_range() {
        let slider = slider_control(ui, value, range, None, width, Some(COMPONENT_SLIDER_HEIGHT));
        let current = *value;
        slider.response.widget_info(|| egui::WidgetInfo::slider(true, current, format!("{accessible} slider")));
        if slider.changed {
            drafts.discard(field_id);
        }
        response = response.merge(slider);
    }
    if let Some((button, default)) = reset
        && button.clicked()
        && modified
    {
        *value = default;
        drafts.discard(field_id);
        response.changed = true;
        response.phase = EditPhase::Committed;
    }
    response
}

/// A value-less range slider that never clamps or rounds values it did not edit, as a lab control response.
fn slider_control(
    ui: &mut Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    step: Option<f64>,
    width: f32,
    height: Option<f32>,
) -> ControlResponse {
    let height = height.unwrap_or(ui.spacing().interact_size.y);
    let response = lab_slider(ui, value, range, step, vec2(width, height));
    let changed = response.changed();
    // A press and its drag are one gesture: it continues while the button is held and commits on release.
    let phase = if response.drag_stopped() || response.clicked() {
        EditPhase::Committed
    } else if changed && (response.is_pointer_button_down_on() || response.dragged()) {
        EditPhase::Continuing
    } else if changed {
        EditPhase::Committed
    } else {
        EditPhase::Idle
    };
    ControlResponse { response, changed, phase, component: None }
}

/// The lab range slider: a thin rail with an accent fill up to the value and a round accent thumb. Pressing or dragging
/// sets the value from the pointer, snapped to multiples of `step` from the minimum when given; arrow keys move it by a
/// step (or a hundredth of the range) while focused. A value it did not edit is never clamped or rounded, even outside
/// the range. Returns a response marked changed when the value changed.
pub fn lab_slider(
    ui: &mut Ui,
    value: &mut f64,
    range: std::ops::RangeInclusive<f64>,
    step: Option<f64>,
    size: Vec2,
) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let (minimum, maximum) = (*range.start(), *range.end());
    let thumb_radius = 6.0;
    let left = rect.left() + thumb_radius;
    let right = (rect.right() - thumb_radius).max(left + 1.0);
    let span = maximum - minimum;
    let snap = |candidate: f64| -> f64 {
        let snapped = match step.filter(|step| *step > 0.0) {
            Some(step) => minimum + (((candidate - minimum) / step).round() * step),
            None => candidate,
        };
        snapped.clamp(minimum, maximum)
    };
    let mut next = None;
    if span > 0.0
        && (response.is_pointer_button_down_on() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let fraction = f64::from(((pointer.x - left) / (right - left)).clamp(0.0, 1.0));
        next = Some(snap(minimum + (fraction * span)));
    }
    if response.has_focus() && span > 0.0 {
        ui.memory_mut(|memory| {
            memory
                .set_focus_lock_filter(response.id, egui::EventFilter { horizontal_arrows: true, ..Default::default() })
        });
        let (decrease, increase) =
            ui.input(|input| (input.num_presses(Key::ArrowLeft), input.num_presses(Key::ArrowRight)));
        let presses = increase as f64 - decrease as f64;
        if presses != 0.0 {
            let increment = step.filter(|step| *step > 0.0).unwrap_or(span / 100.0);
            next = Some(snap(value.clamp(minimum, maximum) + (presses * increment)));
        }
    }
    if let Some(next) = next
        && next != *value
    {
        *value = next;
        response.mark_changed();
    }

    // Paint: rail, accent fill to the value, and the thumb.
    let painter = ui.painter_at(rect.expand(2.0));
    let fraction = if span > 0.0 { ((*value - minimum) / span).clamp(0.0, 1.0) as f32 } else { 0.0 };
    let center = pos2(left + ((right - left) * fraction), rect.center().y);
    let rail = Rect::from_min_max(pos2(left, rect.center().y - 2.0), pos2(right, rect.center().y + 2.0));
    painter.rect_filled(rail, CornerRadius::same(2), tokens::BUTTON);
    painter.rect_filled(
        Rect::from_min_max(rail.min, pos2(center.x, rail.max.y)),
        CornerRadius::same(2),
        tokens::ACCENT,
    );
    let active = response.dragged() || response.has_focus();
    let thumb = if response.hovered() || active { tokens::ACCENT_HOVER } else { tokens::ACCENT };
    painter.circle(center, thumb_radius, thumb, Stroke::new(tokens::BORDER_FOCUS, tokens::FIELD));
    if response.has_focus() {
        painter.circle_stroke(center, thumb_radius + 2.5, Stroke::new(tokens::BORDER_THIN, tokens::ACCENT));
    }
    response
}

// ---------------------------------------------------------------------------------------------------------------------
// Color control
// ---------------------------------------------------------------------------------------------------------------------

/// Picker state while the popup is open; created from the authored color when it opens and never written back
/// unless the author changes it.
#[derive(Clone, Debug)]
struct PickerMemory {
    hsva: egui::ecolor::Hsva,
    hex: String,
    hex_error: Option<String>,
    dragging: bool,
}

/// A linear-RGBA color swatch over a checkerboard with an sRGB picker. The swatch shows the linear color in display
/// space (HDR channels clamp for display only); choosing a color converts RGB back to linear within the owner's bounds
/// and preserves alpha. Opening or refreshing the picker never writes, so exact and HDR channels survive until the
/// author picks. A rejected numeric draft blocks picker choices until repaired.
pub struct ColorControl<'a> {
    id_salt: Id,
    values: &'a mut [f64],
    minimums: &'a [f64],
    maximums: &'a [f64],
    label: String,
}

impl<'a> ColorControl<'a> {
    /// A swatch for linear `values` (RGB or RGBA) with per-channel bounds that picker choices respect.
    pub fn new(id_salt: impl egui::AsId, values: &'a mut [f64], minimums: &'a [f64], maximums: &'a [f64]) -> Self {
        Self { id_salt: Id::new(id_salt), values, minimums, maximums, label: String::new() }
    }

    /// Name for hover help, for example the parameter name.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = label.into();
        self
    }

    /// The popup id for a swatch with `id_salt` in `ui`, for tests and hosts that open it programmatically.
    pub fn popup_id_in(ui: &Ui, id_salt: impl egui::AsId) -> Id {
        ui.make_persistent_id(Id::new(id_salt)).with("picker")
    }

    /// Draw the swatch and, when open, the picker; apply a choice.
    pub fn show(self, ui: &mut Ui, drafts: &NumericDrafts) -> ControlResponse {
        let id = ui.make_persistent_id(self.id_salt);
        let popup_id = id.with("picker");
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(COLOR_SWATCH_SIZE), Sense::click());
        paint_swatch(ui, rect, lab_values::display_color(self.values), response.hovered() || response.has_focus());
        let channels: Vec<String> = self.values.iter().map(|value| format_number(*value)).collect();
        let name = if self.label.is_empty() { String::new() } else { format!("{} · ", self.label) };
        let swatch_name =
            if self.label.is_empty() { "Color picker".to_owned() } else { format!("{} color picker", self.label) };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, &swatch_name));
        let response =
            response.on_hover_text(format!("{name}Linear RGBA ({})\n{COLOR_PICKER_HINT}", channels.join(", ")));
        let mut control = ControlResponse::idle(response.clone());

        let open_before = egui::Popup::is_id_open(ui.ctx(), popup_id);
        let values = self.values;
        let (minimums, maximums) = (self.minimums, self.maximums);
        let shown = egui::Popup::from_toggle_button_response(&response)
            .id(popup_id)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(240.0);
                let mut memory: PickerMemory =
                    ui.data(|data| data.get_temp(popup_id)).unwrap_or_else(|| PickerMemory {
                        hsva: picker_color(values),
                        hex: lab_values::color_hex(values),
                        hex_error: None,
                        dragging: false,
                    });
                let result = picker_contents(ui, values, minimums, maximums, drafts, &mut memory);
                ui.data_mut(|data| data.insert_temp(popup_id, memory));
                result
            });
        match shown {
            Some(inner) => {
                let (changed, phase) = inner.inner;
                control.changed |= changed;
                control.phase = control.phase.max(phase);
            }
            None if open_before => ui.data_mut(|data| data.remove::<PickerMemory>(popup_id)),
            None => {}
        }
        control
    }
}

/// The picker popup's contents; returns whether the color changed and the history phase.
fn picker_contents(
    ui: &mut Ui,
    values: &mut [f64],
    minimums: &[f64],
    maximums: &[f64],
    drafts: &NumericDrafts,
    memory: &mut PickerMemory,
) -> (bool, EditPhase) {
    if let Err(message) = drafts.require_valid() {
        ui.colored_label(tokens::DANGER, message);
        return (false, EditPhase::Idle);
    }
    let mut changed = false;
    let mut phase = EditPhase::Idle;
    let pointer_down = ui.input(|input| input.pointer.any_down());

    ui.spacing_mut().slider_width = 220.0;
    if egui::color_picker::color_picker_hsva_2d(ui, &mut memory.hsva, egui::color_picker::Alpha::Opaque) {
        let picked = memory.hsva.to_rgb().map(|channel| f64::from(egui::ecolor::gamma_from_linear(channel)));
        let current: Vec<f64> =
            (0..3).map(|index| lab_values::linear_to_display(values.get(index).copied().unwrap_or(0.0))).collect();
        let moved =
            picked.iter().zip(&current).any(|(picked, current)| (picked - current).abs() > PICKER_CHANGE_EPSILON);
        if moved {
            let next = lab_values::picked_color(picked, values, minimums, maximums);
            values.copy_from_slice(&next[..values.len()]);
            memory.hex = lab_values::color_hex(values);
            memory.hex_error = None;
            changed = true;
            phase = if pointer_down { EditPhase::Continuing } else { EditPhase::Committed };
            memory.dragging = pointer_down;
        }
    }
    if memory.dragging && !pointer_down {
        memory.dragging = false;
        phase = EditPhase::Committed;
    }

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("sRGB").color(tokens::MUTED).size(tokens::TYPE_SM));
        let response = ui.add(TextEdit::singleline(&mut memory.hex).font(FontId::monospace(12.0)).desired_width(90.0));
        if response.lost_focus() {
            match lab_values::picked_color_hex(&memory.hex, values, minimums, maximums) {
                Ok(next) => {
                    let next = &next[..values.len()];
                    if next != values {
                        values.copy_from_slice(next);
                        memory.hsva = picker_color(values);
                        changed = true;
                        phase = EditPhase::Committed;
                    }
                    memory.hex = lab_values::color_hex(values);
                    memory.hex_error = None;
                }
                Err(message) => memory.hex_error = Some(message),
            }
        }
    });
    if let Some(message) = &memory.hex_error {
        ui.colored_label(tokens::DANGER, message);
    }
    ui.label(egui::RichText::new(COLOR_PICKER_HINT).color(tokens::MUTED).size(tokens::TYPE_XS));
    (changed, phase)
}

/// The picker's starting color: the authored linear RGB clamped for display.
fn picker_color(values: &[f64]) -> egui::ecolor::Hsva {
    let channel = |index: usize| values.get(index).copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
    egui::ecolor::Hsva::from_rgb([channel(0), channel(1), channel(2)])
}

// ---------------------------------------------------------------------------------------------------------------------
// Default and modified indication
// ---------------------------------------------------------------------------------------------------------------------

/// Whether `value` differs from its default (an absent default is never modified).
pub fn is_modified(value: f64, default: Option<f64>) -> bool {
    default.is_some_and(|default| value.to_bits() != default.to_bits() && value != default)
}

/// A small reset button, enabled while `modified`, with the default in its hover text. Returns its response; the
/// caller applies the reset when it is clicked.
pub fn reset_button(ui: &mut Ui, modified: bool, default: f64) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::splat(RESET_BUTTON_SIZE), if modified { Sense::click() } else { Sense::hover() });
    let color = if !modified {
        tokens::BORDER
    } else if response.hovered() {
        tokens::ACCENT_HOVER
    } else {
        tokens::ACCENT
    };
    paint_reset_icon(ui, rect, color);
    if modified { response.on_hover_text(format!("Reset to default {}", format_number(default))) } else { response }
}

/// The accent dot after a modified property's heading.
pub fn modified_marker(ui: &mut Ui, modified: bool) {
    if modified {
        ui.label(egui::RichText::new("•").color(tokens::ACCENT)).on_hover_text("Modified from its default");
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------------------------------------------------

fn text_color(rejected: bool, modified: bool) -> Color32 {
    if rejected {
        tokens::DANGER
    } else if modified {
        tokens::ACCENT
    } else {
        tokens::TEXT
    }
}

fn paint_field(ui: &Ui, rect: Rect, focused: bool, rejected: bool, text: &str, color: Color32, align: Align) {
    let stroke_color = if rejected {
        tokens::DANGER
    } else if focused {
        tokens::ACCENT
    } else {
        tokens::BORDER
    };
    let painter = ui.painter_at(rect.expand(1.0));
    painter.rect(
        rect,
        CornerRadius::ZERO,
        tokens::FIELD,
        Stroke::new(tokens::BORDER_THIN, stroke_color),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(tokens::SPACE_XS, 0.0));
    let (anchor, position) = match align {
        Align::Min => (Align2::LEFT_CENTER, inner.left_center()),
        Align::Center => (Align2::CENTER_CENTER, inner.center()),
        Align::Max => (Align2::RIGHT_CENTER, inner.right_center()),
    };
    ui.painter_at(inner).text(position, anchor, text, FontId::monospace(12.0), color);
}

fn paint_label(ui: &Ui, rect: Rect, text: &str, color: Color32, size: f32) {
    ui.painter_at(rect).text(rect.left_center(), Align2::LEFT_CENTER, text, FontId::proportional(size), color);
}

fn show_draft_error(ui: &mut Ui, message: &str, width: f32) {
    ui.scope(|ui| {
        ui.set_max_width(width.max(120.0));
        ui.label(egui::RichText::new(message).color(tokens::DANGER).size(tokens::TYPE_XS));
    });
}

/// Select a text edit's whole contents so typing replaces the shown value.
fn select_all(context: &egui::Context, text_id: Id, characters: usize) {
    let mut state = egui::text_edit::TextEditState::load(context, text_id).unwrap_or_default();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(characters),
    )));
    state.store(context, text_id);
}

/// A swatch: checkerboard, then the (possibly translucent) display color, then a border.
fn paint_swatch(ui: &Ui, rect: Rect, color: Color32, focused: bool) {
    paint_checkerboard(ui, rect);
    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, CornerRadius::ZERO, color);
    let stroke = if focused {
        Stroke::new(tokens::BORDER_FOCUS, tokens::ACCENT)
    } else {
        Stroke::new(tokens::BORDER_THIN, tokens::BORDER)
    };
    painter.rect_stroke(rect, CornerRadius::ZERO, stroke, StrokeKind::Inside);
}

/// The checkerboard behind translucent colors.
pub fn paint_checkerboard(ui: &Ui, rect: Rect) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::ZERO, CHECKER_DARK);
    let columns = (rect.width() / CHECKER_CELL).ceil() as usize;
    let rows = (rect.height() / CHECKER_CELL).ceil() as usize;
    for row in 0..rows {
        for column in 0..columns {
            if (row + column) % 2 == 0 {
                let min = pos2(rect.left() + (column as f32 * CHECKER_CELL), rect.top() + (row as f32 * CHECKER_CELL));
                painter.rect_filled(
                    Rect::from_min_size(min, Vec2::splat(CHECKER_CELL)),
                    CornerRadius::ZERO,
                    CHECKER_LIGHT,
                );
            }
        }
    }
}

/// A counter-clockwise circular arrow (vector, since the bundled fonts lack ↺).
fn paint_reset_icon(ui: &Ui, rect: Rect, color: Color32) {
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let radius = rect.width() * 0.28;
    let stroke = Stroke::new(1.5, color);
    let start = -0.9_f32;
    let end = std::f32::consts::PI * 1.45;
    let points: Vec<Pos2> = (0..=16)
        .map(|step| {
            let angle = start + ((end - start) * (step as f32 / 16.0));
            center + (vec2(angle.cos(), angle.sin()) * radius)
        })
        .collect();
    let tip = points[0];
    painter.add(egui::Shape::line(points, stroke));
    let head = radius * 0.7;
    painter.line_segment([tip, tip + vec2(-head * 0.2, -head)], stroke);
    painter.line_segment([tip, tip + vec2(head, -head * 0.15)], stroke);
}

#[cfg(test)]
#[path = "lab_controls.test.rs"]
mod tests;
