// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Discrete authoring controls for whole-number values: a toggle for zero-or-one flags, a choice list for option
//! indices, and a seed field with a reroll button. They edit the same `&mut f64` values the numeric lab controls do
//! (FORM parameters and component fields store every number as `f64`), and every edit commits immediately, so tools
//! record [`PropertyHistory`](crate::PropertyHistory) entries exactly as they do for an arrow-key step.
//!
//! Rerolling a seed is cosmetic tooling, not gameplay randomness: the next seed is a deterministic scramble of the
//! current one, so repeated rerolls walk a varied sequence without any global random state.

use egui::{
    Response,
    Ui,
};

use crate::lab_controls::{
    ControlResponse,
    EditPhase,
    NumberField,
    NumericDrafts,
};

/// Width of the seed field, wide enough for the full nonnegative 31-bit range.
const SEED_FIELD_WIDTH: f32 = 112.0;

/// Widest a choice list grows; narrower panels shrink it to fit.
const CHOICE_WIDTH: f32 = 180.0;

/// Reroll button text and help.
const REROLL_TEXT: &str = "⟳ Reroll";
const REROLL_HINT: &str = "Pick a new seed; the result is reproducible from the number shown";

/// A zero-or-one value shown as a checkbox; `label` is the visible text beside it.
pub fn toggle_control(ui: &mut Ui, id_salt: impl egui::AsId, value: &mut f64, label: &str) -> ControlResponse {
    ui.push_id(egui::Id::new(id_salt), |ui| {
        let mut checked = *value >= 0.5;
        let response = ui.checkbox(&mut checked, label);
        let changed = response.changed();
        if changed {
            *value = if checked { 1.0 } else { 0.0 };
        }
        discrete_response(response, changed)
    })
    .inner
}

/// An option index shown as a choice list of `options`; values outside the list display as the nearest valid option.
pub fn choice_control(ui: &mut Ui, id_salt: impl egui::AsId, value: &mut f64, options: &[String]) -> ControlResponse {
    let last = options.len().saturating_sub(1);
    let current = (value.round().max(0.0) as usize).min(last);
    let mut selected = current;
    let selected_text = options.get(current).cloned().unwrap_or_default();
    let response = egui::ComboBox::from_id_salt(egui::Id::new(id_salt))
        .selected_text(selected_text)
        .width(ui.available_width().min(CHOICE_WIDTH))
        .show_ui(ui, |ui| {
            for (index, option) in options.iter().enumerate() {
                ui.selectable_value(&mut selected, index, option.as_str());
            }
        });
    let changed = selected != current;
    if changed {
        *value = selected as f64;
    }
    discrete_response(response.response, changed)
}

/// A whole-number seed between `minimum` and `maximum` with a reroll button that picks a new seed in that range.
pub fn seed_control(
    ui: &mut Ui,
    id_salt: impl egui::AsId,
    value: &mut f64,
    minimum: f64,
    maximum: f64,
    drafts: &mut NumericDrafts,
) -> ControlResponse {
    ui.push_id(egui::Id::new(id_salt), |ui| {
        ui.horizontal(|ui| {
            let field_id = NumberField::id_in(ui, "seed");
            let mut combined = NumberField::new("seed", value)
                .range(minimum, maximum)
                .integer()
                .step(1.0)
                .width(SEED_FIELD_WIDTH)
                .label("Seed value")
                .show(ui, drafts);
            let reroll = ui.button(REROLL_TEXT).on_hover_text(REROLL_HINT);
            if reroll.clicked() {
                *value = rerolled_seed(*value, minimum, maximum);
                drafts.discard(field_id);
                combined.changed = true;
                combined.phase = EditPhase::Committed;
            }
            combined.response = combined.response.union(reroll);
            combined
        })
        .inner
    })
    .inner
}

/// The next seed after `seed` within the inclusive whole-number range, never equal to `seed` when the range allows.
pub fn rerolled_seed(seed: f64, minimum: f64, maximum: f64) -> f64 {
    let low = minimum.ceil();
    let high = maximum.floor();
    if !(low.is_finite() && high.is_finite()) || high <= low {
        return low;
    }

    // SplitMix64 scramble of the current seed: well distributed, cheap, and reproducible.
    let mut state = (seed.max(0.0) as u64).wrapping_add(0x9E37_79B9_7F4A_7C15);
    state = (state ^ (state >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    state = (state ^ (state >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    state ^= state >> 31;

    let span = (high - low) as u64 + 1;
    let mut next = low + (state % span) as f64;
    if next == seed {
        next = if next < high { next + 1.0 } else { low };
    }
    next
}

/// A response for an edit that commits the moment it happens.
fn discrete_response(response: Response, changed: bool) -> ControlResponse {
    ControlResponse {
        response,
        changed,
        phase: if changed { EditPhase::Committed } else { EditPhase::Idle },
        component: None,
    }
}

#[cfg(test)]
#[path = "choice_controls.test.rs"]
mod tests;
