// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Native persistence for config panels: a version-1 tuning override document (`tuning-overrides.json`), saved with
//! only the values that differ from field defaults so later default changes still reach untouched fields.

use std::path::PathBuf;

use serde_json::{
    Map,
    Value,
};

use crate::{
    files::WriteError,
    files::pretty_json,
    files::read_document,
    files::revision_of,
    files::write_atomically,
};
use joy_core::format::json_number;
use joy_core::{
    ConfigField,
    ConfigValues,
    create_config_document_values,
};

/// Schema reference written into saved documents, beside the document in each game's `config` folder.
const SCHEMA_REFERENCE: &str = "./tuning-overrides.schema.json";

/// One override document and the revision this editor last read or wrote.
pub(crate) struct ConfigFile {
    path: PathBuf,
    fields: Vec<ConfigField>,
    revision: String,
}

impl ConfigFile {
    /// Read the document and return complete values.
    pub(crate) fn open(path: PathBuf, fields: &[ConfigField]) -> Result<(Self, ConfigValues), String> {
        let text = read_document(&path)?;
        let document: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        let values = create_config_document_values(fields, &document).map_err(|error| error.to_string())?;
        Ok((Self { path, fields: fields.to_vec(), revision: revision_of(&text) }, values))
    }

    /// Write the values that differ from defaults; refuses when the file changed outside this editor.
    pub(crate) fn save(&mut self, values: &ConfigValues) -> Result<(), String> {
        let overrides: Map<String, Value> = self
            .fields
            .iter()
            .filter_map(|field| {
                let value = values.get(&field.key).copied()?;
                (value != field.default_value).then(|| (field.key.clone(), json_number(value)))
            })
            .collect();
        let document = serde_json::json!({ "$schema": SCHEMA_REFERENCE, "version": 1, "values": overrides });
        let text = pretty_json(&document)?;
        match write_atomically(&self.path, &text, &self.revision) {
            Ok(revision) => {
                self.revision = revision;
                Ok(())
            }
            Err(WriteError::Changed) => Err(
                "The config file changed outside this editor. Your edits are retained; restart to reconcile before saving."
                    .to_owned(),
            ),
            Err(WriteError::Failed(message)) => Err(message),
        }
    }
}

#[cfg(test)]
#[path = "config_file.test.rs"]
mod tests;
