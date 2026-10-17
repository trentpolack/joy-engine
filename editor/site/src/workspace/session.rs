// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The restored workspace session: the selected project, each project's open paths and active tab, and the Content
//! Browser's width. (The level editor's panels persist through the authoring kit's `PanelLayout`.)
//! Unsaved source buffers are never stored; disk content stays authoritative.
//!
//! Sessions persist through `runtime::Storage` (a per-user file natively, `localStorage` in browsers) as JSON with
//! `project`, `tabs`, and `active` fields, alongside panel widths. Unreadable or
//! invalid sessions start empty; storage failures never block authoring.

use std::collections::BTreeMap;

use joy_engine::runtime::Storage;
use joy_engine::serde_json::{
    self,
    Map,
    Value,
};

use super::constants::{
    CONTENT_BROWSER_DEFAULT_WIDTH,
    CONTENT_BROWSER_MAXIMUM_WIDTH,
    CONTENT_BROWSER_MINIMUM_WIDTH,
    SESSION_STORAGE_KEY,
};

/// Panel layout preferences, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutPreferences {
    /// Content Browser width.
    pub content_browser_width: f32,
    /// Whether the Content Browser is collapsed.
    pub content_browser_collapsed: bool,
}

impl Default for LayoutPreferences {
    fn default() -> Self {
        Self { content_browser_width: CONTENT_BROWSER_DEFAULT_WIDTH, content_browser_collapsed: false }
    }
}

/// Navigation and layout restored at the next launch.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceSession {
    /// The selected project id.
    pub project: String,
    /// Open document paths by project, in tab order.
    pub tabs: BTreeMap<String, Vec<String>>,
    /// The active document path by project.
    pub active: BTreeMap<String, String>,
    /// Panel widths and visibility.
    pub layout: LayoutPreferences,
}

impl WorkspaceSession {
    /// Read the stored session; missing, unreadable, or invalid data gives an empty session.
    pub fn load(storage: Option<&Storage>) -> Self {
        let text = storage.and_then(|storage| storage.load(SESSION_STORAGE_KEY).ok().flatten());
        text.and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map(|value| Self::from_json(&value))
            .unwrap_or_default()
    }

    /// Store the session; failures are returned for the status line and otherwise ignored.
    pub fn save(&self, storage: Option<&Storage>) -> Result<(), String> {
        let Some(storage) = storage else {
            return Ok(());
        };
        storage.save(SESSION_STORAGE_KEY, &self.to_json().to_string()).map_err(|error| error.to_string())
    }

    /// Read a session record, ignoring malformed fields.
    pub fn from_json(value: &Value) -> Self {
        let mut session = Self::default();
        let Value::Object(record) = value else {
            return session;
        };
        if let Some(Value::String(project)) = record.get("project") {
            session.project = project.clone();
        }
        if let Some(Value::Object(tabs)) = record.get("tabs") {
            for (project, paths) in tabs {
                let paths: Vec<String> = paths
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|path| path.as_str().map(str::to_owned))
                    .collect();
                session.tabs.insert(project.clone(), paths);
            }
        }
        if let Some(Value::Object(active)) = record.get("active") {
            for (project, path) in active {
                if let Some(path) = path.as_str() {
                    session.active.insert(project.clone(), path.to_owned());
                }
            }
        }
        if let Some(Value::Object(layout)) = record.get("layout") {
            let number = |name: &str| layout.get(name).and_then(Value::as_f64).filter(|value| value.is_finite());
            let defaults = LayoutPreferences::default();
            session.layout = LayoutPreferences {
                content_browser_width: number("contentBrowserWidth").map_or(defaults.content_browser_width, |width| {
                    (width as f32).clamp(CONTENT_BROWSER_MINIMUM_WIDTH, CONTENT_BROWSER_MAXIMUM_WIDTH)
                }),
                content_browser_collapsed: layout.get("contentBrowserCollapsed") == Some(&Value::Bool(true)),
            };
        }
        session
    }

    /// The stored record.
    pub fn to_json(&self) -> Value {
        let mut record = Map::new();
        record.insert("project".to_owned(), Value::from(self.project.clone()));
        let tabs = self.tabs.iter().map(|(project, paths)| (project.clone(), Value::from(paths.clone())));
        record.insert("tabs".to_owned(), Value::Object(tabs.collect()));
        let active = self.active.iter().map(|(project, path)| (project.clone(), Value::from(path.clone())));
        record.insert("active".to_owned(), Value::Object(active.collect()));
        let mut layout = Map::new();
        layout.insert("contentBrowserWidth".to_owned(), Value::from(f64::from(self.layout.content_browser_width)));
        layout.insert("contentBrowserCollapsed".to_owned(), Value::Bool(self.layout.content_browser_collapsed));
        record.insert("layout".to_owned(), Value::Object(layout));
        Value::Object(record)
    }
}
