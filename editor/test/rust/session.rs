// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The restored session: project, open paths, active tab, and panel widths persist through `runtime::Storage`;
//! invalid stored data starts empty.

use joy_editor::workspace::constants::SESSION_STORAGE_KEY;
use joy_editor::workspace::session::{
    LayoutPreferences,
    WorkspaceSession,
};
use joy_engine::runtime::Storage;

use crate::support::TempFolder;

#[test]
fn sessions_round_trip_through_storage_and_ignore_invalid_data() {
    let folder = TempFolder::new("session");
    let storage = Storage::in_directory(&folder.root);
    assert_eq!(WorkspaceSession::load(Some(&storage)), WorkspaceSession::default());

    let mut session = WorkspaceSession { project: "demo".into(), ..Default::default() };
    session.tabs.insert("demo".into(), vec!["assets/a.joylevel".into(), "assets/b.joyfx".into()]);
    session.active.insert("demo".into(), "assets/b.joyfx".into());
    session.layout = LayoutPreferences { content_browser_width: 300.0, content_browser_collapsed: true };
    session.save(Some(&storage)).unwrap();
    assert_eq!(WorkspaceSession::load(Some(&storage)), session);

    storage.save(SESSION_STORAGE_KEY, "{not json").unwrap();
    assert_eq!(WorkspaceSession::load(Some(&storage)), WorkspaceSession::default());
    storage
        .save(
            SESSION_STORAGE_KEY,
            r#"{"project": 3, "tabs": {"demo": ["a", 4]}, "layout": {"contentBrowserWidth": 9000}}"#,
        )
        .unwrap();
    let loaded = WorkspaceSession::load(Some(&storage));
    assert_eq!(loaded.project, "");
    assert_eq!(loaded.tabs["demo"], ["a"]);
    assert_eq!(loaded.layout.content_browser_width, 600.0, "stored widths are clamped");
    assert_eq!(WorkspaceSession::load(None), WorkspaceSession::default());
}
