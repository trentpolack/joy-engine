// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One independent editing history over a game's numeric tuning fields and its saved baseline.
//!
//! Edits are validated against the field contract before anything changes. A runtime adapter may reject a complete
//! record, leaving values and history unchanged. Continuous edits (a slider drag) between
//! [`begin_edit`](ConfigSession::begin_edit) and [`end_edit`](ConfigSession::end_edit) undo as one step. Games poll
//! [`take_change`](ConfigSession::take_change) each frame, or install an adapter with
//! [`with_on_change`](ConfigSession::with_on_change).

use indexmap::IndexMap;

use joy_core::{
    ConfigError,
    ConfigField,
    ConfigValues,
    create_config_values,
};

/// Undo steps retained per session.
const MAXIMUM_HISTORY_LENGTH: usize = 100;

/// A runtime adapter that receives every complete record and may reject it.
pub type ConfigChangeAdapter = Box<dyn FnMut(&ConfigValues) -> Result<(), String>>;

/// A persistence adapter that stores a complete snapshot.
pub type ConfigSaveAdapter = Box<dyn FnMut(&ConfigValues) -> Result<(), String>>;

/// An edit or save did not happen.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ConfigSessionError {
    /// The values broke the field contract.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The runtime adapter rejected the record.
    #[error("{0}")]
    Rejected(String),
    /// The session has no save adapter.
    #[error("This config session has no save adapter.")]
    NoSaveAdapter,
    /// The save adapter failed.
    #[error("{0}")]
    Save(String),
}

/// Editing history and saved baseline for one set of tuning fields.
pub struct ConfigSession {
    fields: Vec<ConfigField>,
    defaults: ConfigValues,
    current: ConfigValues,
    saved: ConfigValues,
    on_change: Option<ConfigChangeAdapter>,
    save_values: Option<ConfigSaveAdapter>,
    history: Vec<ConfigValues>,
    future: Vec<ConfigValues>,
    editing: bool,
    edit_remembered: bool,
    changed: bool,
}

impl ConfigSession {
    /// Validate the fields and partial initial overrides; the result is both current and saved.
    pub fn new(fields: &[ConfigField], values: &IndexMap<String, f64>) -> Result<Self, ConfigError> {
        let defaults = create_config_values(fields, &IndexMap::new())?;
        let current = create_config_values(fields, values)?;
        Ok(Self {
            fields: fields.to_vec(),
            defaults,
            saved: current.clone(),
            current,
            on_change: None,
            save_values: None,
            history: Vec::new(),
            future: Vec::new(),
            editing: false,
            edit_remembered: false,
            changed: false,
        })
    }

    /// Install a runtime adapter that receives each complete record before it is committed.
    pub fn with_on_change(mut self, on_change: ConfigChangeAdapter) -> Self {
        self.on_change = Some(on_change);
        self
    }

    /// Install a persistence adapter for [`save`](Self::save).
    pub fn with_save(mut self, save: ConfigSaveAdapter) -> Self {
        self.save_values = Some(save);
        self
    }

    /// The field contract.
    pub fn fields(&self) -> &[ConfigField] {
        &self.fields
    }

    /// Current values in field order.
    pub fn values(&self) -> &ConfigValues {
        &self.current
    }

    /// One current value; unknown keys read as `None`.
    pub fn value(&self, key: &str) -> Option<f64> {
        self.current.get(key).copied()
    }

    /// The last saved (or initial) values.
    pub fn stored_values(&self) -> &ConfigValues {
        &self.saved
    }

    /// Field defaults.
    pub fn default_values(&self) -> &ConfigValues {
        &self.defaults
    }

    /// Whether current values differ from the saved baseline.
    pub fn dirty(&self) -> bool {
        self.current != self.saved
    }

    /// Whether undo has a step.
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Whether redo has a step.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Whether a save adapter is installed.
    pub fn can_save(&self) -> bool {
        self.save_values.is_some()
    }

    /// The current values when they changed since the last call.
    pub fn take_change(&mut self) -> Option<ConfigValues> {
        std::mem::take(&mut self.changed).then(|| self.current.clone())
    }

    /// Report the current values through [`take_change`](Self::take_change) once, for example after loading them.
    pub fn mark_changed(&mut self) {
        self.changed = true;
    }

    /// Start a continuous edit; changes until [`end_edit`](Self::end_edit) undo as one step.
    pub fn begin_edit(&mut self) {
        if !self.editing {
            self.editing = true;
            self.edit_remembered = false;
        }
    }

    /// Finish a continuous edit.
    pub fn end_edit(&mut self) {
        self.editing = false;
        self.edit_remembered = false;
    }

    /// Set one value.
    pub fn set_value(&mut self, key: &str, value: f64) -> Result<(), ConfigSessionError> {
        let mut next = self.current.clone();
        next.insert(key.to_owned(), value);
        let validated = create_config_values(&self.fields, &next)?;
        self.apply(validated, true)
    }

    /// Set several values as one step (for example a preset).
    pub fn set_values(&mut self, values: &IndexMap<String, f64>) -> Result<(), ConfigSessionError> {
        let mut next = self.current.clone();
        next.extend(values.iter().map(|(key, value)| (key.clone(), *value)));
        let validated = create_config_values(&self.fields, &next)?;
        self.end_edit();
        self.apply(validated, true)
    }

    /// Step back one edit.
    pub fn undo(&mut self) {
        self.end_edit();
        let Some(previous) = self.history.last().cloned() else {
            return;
        };
        let current = self.current.clone();
        if self.apply(previous, false).is_ok() {
            self.future.push(current);
            self.history.pop();
        }
    }

    /// Step forward one undone edit.
    pub fn redo(&mut self) {
        self.end_edit();
        let Some(next) = self.future.last().cloned() else {
            return;
        };
        let current = self.current.clone();
        if self.apply(next, false).is_ok() {
            self.history.push(current);
            self.future.pop();
        }
    }

    /// Return to the saved values as one undoable step.
    pub fn revert(&mut self) -> Result<(), ConfigSessionError> {
        self.end_edit();
        self.apply(self.saved.clone(), true)
    }

    /// Return one field to its default.
    pub fn reset_field(&mut self, key: &str) -> Result<(), ConfigSessionError> {
        let Some(default) = self.defaults.get(key).copied() else {
            return Err(ConfigError::UnknownKey(key.to_owned()).into());
        };
        self.end_edit();
        self.set_value(key, default)
    }

    /// Return every field to its default as one step.
    pub fn reset_all(&mut self) -> Result<(), ConfigSessionError> {
        self.end_edit();
        self.apply(self.defaults.clone(), true)
    }

    /// Store a snapshot of the current values through the save adapter; success makes it the saved baseline.
    pub fn save(&mut self) -> Result<(), ConfigSessionError> {
        let snapshot = self.current.clone();
        let Some(save) = self.save_values.as_mut() else {
            return Err(ConfigSessionError::NoSaveAdapter);
        };
        save(&snapshot).map_err(ConfigSessionError::Save)?;
        self.saved = snapshot;
        Ok(())
    }

    /// Commit a complete record once the runtime accepts it.
    fn apply(&mut self, values: ConfigValues, remember: bool) -> Result<(), ConfigSessionError> {
        if self.current == values {
            return Ok(());
        }
        if let Some(on_change) = self.on_change.as_mut() {
            on_change(&values).map_err(ConfigSessionError::Rejected)?;
        }
        if remember {
            self.future.clear();
            if !self.editing || !self.edit_remembered {
                self.edit_remembered = self.editing;
                self.history.push(self.current.clone());
                if self.history.len() > MAXIMUM_HISTORY_LENGTH {
                    self.history.remove(0);
                }
            }
        }
        self.current = values;
        self.changed = true;
        Ok(())
    }
}

impl std::fmt::Debug for ConfigSession {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ConfigSession")
            .field("values", &self.current)
            .field("saved", &self.saved)
            .field("history", &self.history.len())
            .field("future", &self.future.len())
            .finish()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;
    use joy_core::ConfigApplyMode;

    pub(crate) fn fields() -> Vec<ConfigField> {
        let field = |key: &str, group: &str, default_value, max, step, apply_mode| ConfigField {
            key: key.to_owned(),
            label: key.to_owned(),
            group: group.to_owned(),
            description: format!("{key} description."),
            default_value,
            min: if key == "speed" { 1.0 } else { 0.0 },
            max,
            step,
            units: String::new(),
            apply_mode,
        };
        vec![
            field("speed", "Movement", 4.0, 10.0, 0.5, ConfigApplyMode::Live),
            field("count", "Spawning", 3.0, 8.0, 1.0, ConfigApplyMode::NextSpawn),
        ]
    }

    fn values(pairs: &[(&str, f64)]) -> IndexMap<String, f64> {
        pairs.iter().map(|(key, value)| ((*key).to_owned(), *value)).collect()
    }

    #[test]
    fn invalid_edits_are_atomic() {
        let mut session = ConfigSession::new(&fields(), &values(&[("speed", 5.0)])).unwrap();
        session.set_value("speed", 7.0).unwrap();
        assert_eq!(session.values(), &values(&[("speed", 7.0), ("count", 3.0)]));
        assert_eq!(session.take_change(), Some(values(&[("speed", 7.0), ("count", 3.0)])));
        assert_eq!(session.take_change(), None);
        assert!(matches!(
            session.set_value("count", 2.5),
            Err(ConfigSessionError::Config(ConfigError::NotAnInteger(_)))
        ));
        assert!(session.set_value("unknown", 2.0).is_err());
        assert_eq!(session.values(), &values(&[("speed", 7.0), ("count", 3.0)]));
    }

    #[test]
    fn undo_revert_and_resets_preserve_saved_and_default_semantics() {
        let mut session = ConfigSession::new(&fields(), &values(&[("speed", 5.0), ("count", 4.0)])).unwrap();
        session.set_value("speed", 6.0).unwrap();
        session.set_value("count", 7.0).unwrap();
        assert!(session.can_undo());
        session.undo();
        assert_eq!(session.values(), &values(&[("speed", 6.0), ("count", 4.0)]));
        session.reset_field("speed").unwrap();
        assert_eq!(session.value("speed"), Some(4.0));
        session.undo();
        assert_eq!(session.value("speed"), Some(6.0));
        session.reset_all().unwrap();
        assert_eq!(session.values(), &values(&[("speed", 4.0), ("count", 3.0)]));
        session.revert().unwrap();
        assert_eq!(session.values(), &values(&[("speed", 5.0), ("count", 4.0)]));
        assert!(!session.dirty());
    }

    #[test]
    fn continuous_edits_undo_as_one_step() {
        let mut session = ConfigSession::new(&fields(), &IndexMap::new()).unwrap();
        session.begin_edit();
        for speed in [4.5, 5.0, 5.5] {
            session.set_value("speed", speed).unwrap();
        }
        session.end_edit();
        session.set_value("speed", 6.0).unwrap();
        session.undo();
        assert_eq!(session.value("speed"), Some(5.5));
        session.undo();
        assert_eq!(session.value("speed"), Some(4.0));
        assert!(!session.can_undo());
        session.redo();
        assert_eq!(session.value("speed"), Some(5.5));
    }

    #[test]
    fn saves_snapshot_the_current_values_and_later_edits_stay_dirty() {
        let saved = Rc::new(RefCell::new(Vec::new()));
        let sink = Rc::clone(&saved);
        let mut session =
            ConfigSession::new(&fields(), &values(&[("speed", 5.0)])).unwrap().with_save(Box::new(move |values| {
                sink.borrow_mut().push(values.clone());
                Ok(())
            }));
        session.set_value("speed", 6.0).unwrap();
        session.save().unwrap();
        session.set_value("speed", 7.0).unwrap();
        assert_eq!(*saved.borrow(), [values(&[("speed", 6.0), ("count", 3.0)])]);
        assert!(session.dirty());
        session.revert().unwrap();
        assert_eq!(session.value("speed"), Some(6.0));

        let mut unsaved = ConfigSession::new(&fields(), &IndexMap::new()).unwrap();
        assert!(!unsaved.can_save());
        assert_eq!(unsaved.save(), Err(ConfigSessionError::NoSaveAdapter));
        let mut failing = ConfigSession::new(&fields(), &IndexMap::new())
            .unwrap()
            .with_save(Box::new(|_| Err("disk full".to_owned())));
        failing.set_value("speed", 6.0).unwrap();
        assert_eq!(failing.save(), Err(ConfigSessionError::Save("disk full".to_owned())));
        assert!(failing.dirty());
    }

    #[test]
    fn runtime_rejection_leaves_values_and_history_unchanged() {
        let mut session =
            ConfigSession::new(&fields(), &values(&[("speed", 5.0)])).unwrap().with_on_change(Box::new(|values| {
                if values["speed"] == 7.0 { Err("runtime rejected value".to_owned()) } else { Ok(()) }
            }));
        assert_eq!(session.set_value("speed", 7.0), Err(ConfigSessionError::Rejected("runtime rejected value".into())));
        assert_eq!(session.value("speed"), Some(5.0));
        assert!(!session.can_undo());
        session.set_value("speed", 6.0).unwrap();
        session.undo();
        assert_eq!(session.value("speed"), Some(5.0));
        assert!(!session.can_undo());
    }
}
