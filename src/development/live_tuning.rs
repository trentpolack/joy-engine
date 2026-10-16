// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Live tuning of running objects through one shared history and TUNE panel, matching the TypeScript `LiveTuning`.
//!
//! TypeScript binds object properties by reflection. Rust games register each object's current values under an
//! identifier instead, then apply [`take_change`](LiveTuning::take_change) to their own state every frame; values
//! read back by identifier and field key. Every field applies live.

use std::collections::HashSet;

use egui::Context;
use indexmap::IndexMap;

use crate::core::{ConfigApplyMode, ConfigError, ConfigField, ConfigValues};
use crate::development::config_panel::ConfigPanel;
use crate::development::config_session::{ConfigSaveAdapter, ConfigSession, ConfigSessionError};

/// One live numeric value of a registered object.
#[derive(Clone, Debug, PartialEq)]
pub struct TuningField {
    /// Field key within its object (for example `stiffness`).
    pub key: String,
    /// Display label.
    pub label: String,
    /// One sentence of help.
    pub description: String,
    /// The object's current value.
    pub value: f64,
    /// Reset target; defaults to `value`.
    pub default_value: Option<f64>,
    /// Inclusive minimum.
    pub min: f64,
    /// Inclusive maximum.
    pub max: f64,
    /// Control increment; exactly 1 means integer values.
    pub step: f64,
    /// Display units.
    pub units: String,
    /// Display group; defaults to the object's identifier.
    pub group: Option<String>,
}

/// A registration was refused.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TuningError {
    /// Objects must register before the panel or session is used.
    #[error("Register live objects before opening the tuning session.")]
    SessionOpen,
    /// Identifiers start with a letter and contain only letters, digits, `_`, and `-`.
    #[error("Tuning identifiers must start with a letter and contain no dots.")]
    InvalidIdentifier,
    /// Another object already uses the identifier.
    #[error("Duplicate tuning identifier \"{0}\".")]
    DuplicateIdentifier(String),
    /// A field's metadata or value broke the contract.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A preset was rejected.
    #[error(transparent)]
    Session(#[from] ConfigSessionError),
}

/// Shared history and an optional panel for registered live objects.
pub struct LiveTuning {
    fields: Vec<ConfigField>,
    initial: IndexMap<String, f64>,
    identifiers: HashSet<String>,
    save: Option<ConfigSaveAdapter>,
    session: Option<ConfigSession>,
    panel: ConfigPanel,
}

impl LiveTuning {
    /// An empty tuning owner whose panel shows `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            fields: Vec::new(),
            initial: IndexMap::new(),
            identifiers: HashSet::new(),
            save: None,
            session: None,
            panel: ConfigPanel::new(title, None),
        }
    }

    /// Persist snapshots through an adapter; without one, tuning lasts for the session only.
    pub fn with_save(mut self, save: ConfigSaveAdapter) -> Self {
        self.save = Some(save);
        self
    }

    /// Register one object's live values under `id`. Keys become `id.key`.
    pub fn register(&mut self, id: &str, fields: &[TuningField]) -> Result<(), TuningError> {
        if self.session.is_some() {
            return Err(TuningError::SessionOpen);
        }
        let mut characters = id.chars();
        let valid = characters.next().is_some_and(|first| first.is_ascii_alphabetic())
            && characters.all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'));
        if !valid {
            return Err(TuningError::InvalidIdentifier);
        }
        if self.identifiers.contains(id) {
            return Err(TuningError::DuplicateIdentifier(id.to_owned()));
        }
        let additions: Vec<ConfigField> = fields
            .iter()
            .map(|field| ConfigField {
                key: format!("{id}.{}", field.key),
                label: field.label.clone(),
                group: field.group.clone().unwrap_or_else(|| id.to_owned()),
                description: field.description.clone(),
                default_value: field.default_value.unwrap_or(field.value),
                min: field.min,
                max: field.max,
                step: field.step,
                units: field.units.clone(),
                apply_mode: ConfigApplyMode::Live,
            })
            .collect();
        let values: IndexMap<String, f64> =
            additions.iter().zip(fields).map(|(added, field)| (added.key.clone(), field.value)).collect();
        // Validate the object's fields and current values before keeping any of them.
        crate::core::create_config_values(&additions, &values)?;
        self.identifiers.insert(id.to_owned());
        self.fields.extend(additions);
        self.initial.extend(values);
        Ok(())
    }

    /// The shared session, opened on first use; registration closes once it opens.
    pub fn session(&mut self) -> &mut ConfigSession {
        let (fields, initial, save) = (&self.fields, &self.initial, &mut self.save);
        self.session.get_or_insert_with(|| {
            // Every field and value was validated at registration, so the combined record is valid.
            let session = ConfigSession::new(fields, initial).unwrap_or_else(|_| {
                ConfigSession::new(&[], &IndexMap::new()).expect("an empty field list is always valid")
            });
            match save.take() {
                Some(save) => session.with_save(save),
                None => session,
            }
        })
    }

    /// Draw the TUNE launcher and panel.
    pub fn ui(&mut self, context: &Context) {
        self.session();
        if let Some(session) = self.session.as_mut() {
            self.panel.ui(context, session);
        }
    }

    /// Every value keyed `id.key` when any changed since the last call.
    pub fn take_change(&mut self) -> Option<ConfigValues> {
        self.session.as_mut().and_then(ConfigSession::take_change)
    }

    /// One object's current value; registered values before the session opens.
    pub fn value(&self, id: &str, key: &str) -> Option<f64> {
        let key = format!("{id}.{key}");
        match &self.session {
            Some(session) => session.value(&key),
            None => self.initial.get(&key).copied(),
        }
    }

    /// Apply a captured preset as one undoable step.
    pub fn apply_preset(&mut self, values: &IndexMap<String, f64>) -> Result<(), TuningError> {
        Ok(self.session().set_values(values)?)
    }

    /// Every current value keyed `id.key`, for a preset.
    pub fn capture_preset(&mut self) -> ConfigValues {
        self.session().values().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(key: &str, value: f64) -> TuningField {
        TuningField {
            key: key.to_owned(),
            label: key.to_owned(),
            description: format!("{key} description."),
            value,
            default_value: None,
            min: 0.0,
            max: 100.0,
            step: 0.5,
            units: String::new(),
            group: None,
        }
    }

    #[test]
    fn registered_objects_share_one_history_keyed_by_identifier() {
        let mut tuning = LiveTuning::new("PLAYGROUND TUNING");
        tuning.register("kart1", &[field("stiffness", 30.0), field("damping", 4.0)]).unwrap();
        tuning.register("camera", &[field("distance", 12.0)]).unwrap();
        assert_eq!(tuning.value("kart1", "damping"), Some(4.0));

        tuning.session().set_value("kart1.stiffness", 35.0).unwrap();
        tuning.session().set_value("camera.distance", 14.0).unwrap();
        let change = tuning.take_change().unwrap();
        assert_eq!((change["kart1.stiffness"], change["camera.distance"]), (35.0, 14.0));
        tuning.session().undo();
        assert_eq!(tuning.value("camera", "distance"), Some(12.0));
        // Resetting returns to the registered value.
        tuning.session().reset_field("kart1.stiffness").unwrap();
        assert_eq!(tuning.value("kart1", "stiffness"), Some(30.0));
        assert_eq!(tuning.session().fields()[2].group, "camera");

        let preset = tuning.capture_preset();
        tuning.session().set_value("kart1.damping", 9.0).unwrap();
        tuning.apply_preset(&preset).unwrap();
        assert_eq!(tuning.value("kart1", "damping"), Some(4.0));
    }

    #[test]
    fn registration_is_validated_and_closes_once_the_session_opens() {
        let mut tuning = LiveTuning::new("T");
        assert_eq!(tuning.register("1kart", &[field("a", 1.0)]), Err(TuningError::InvalidIdentifier));
        assert_eq!(tuning.register("kart.one", &[field("a", 1.0)]), Err(TuningError::InvalidIdentifier));
        assert!(matches!(tuning.register("kart", &[field("a", 500.0)]), Err(TuningError::Config(_))));
        tuning.register("kart", &[field("a", 1.0)]).unwrap();
        assert_eq!(tuning.register("kart", &[field("b", 1.0)]), Err(TuningError::DuplicateIdentifier("kart".into())));
        tuning.session();
        assert_eq!(tuning.register("camera", &[field("b", 1.0)]), Err(TuningError::SessionOpen));
    }
}
