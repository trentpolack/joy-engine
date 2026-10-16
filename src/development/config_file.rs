// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Native persistence for config panels: a version-1 tuning override document (`tuning-overrides.json`), saved with
//! only the values that differ from field defaults so later default changes still reach untouched fields.

use std::path::PathBuf;

use serde_json::{Map, Value};

use crate::core::format::json_number;
use crate::core::{ConfigField, ConfigValues, create_config_document_values};
use crate::development::files::{WriteError, pretty_json, read_document, revision_of, write_atomically};

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
mod tests {
    use std::fs;

    use super::*;
    use crate::development::config_session::tests::fields;

    #[test]
    fn saves_only_overrides_and_refuse_outside_changes() {
        let directory = std::env::temp_dir().join(format!("joy-config-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("tuning-overrides.json");
        fs::write(&path, "{\n  \"$schema\": \"./tuning-overrides.schema.json\",\n  \"version\": 1,\n  \"values\": {\n    \"speed\": 8\n  }\n}\n").unwrap();

        let (mut file, mut values) = ConfigFile::open(path.clone(), &fields()).unwrap();
        assert_eq!(values["speed"], 8.0);
        assert_eq!(values["count"], 3.0);
        values.insert("speed".to_owned(), 4.0);
        values.insert("count".to_owned(), 5.0);
        file.save(&values).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"$schema\": \"./tuning-overrides.schema.json\",\n  \"version\": 1,\n  \"values\": {\n    \"count\": 5\n  }\n}\n"
        );

        fs::write(&path, "{\"version\": 1, \"values\": {}}").unwrap();
        assert!(file.save(&values).unwrap_err().contains("changed outside"));
        fs::write(&path, "{\"version\": 2, \"values\": {}}").unwrap();
        assert!(ConfigFile::open(path, &fields()).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
