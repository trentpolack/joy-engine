// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The workspace driven through its real UI in a headless shell with synthetic input (the TypeScript workspace,
//! content, and usability browser workflows): the Content Browser (open, search, filters, New, rename with F2,
//! delete with confirmation, Cmd/Ctrl+P), saves and conflicts, external reloads, repair views, unsaved-work prompts,
//! session restore, and the Documentation and Project Details panels.

use joy_editor::workspace::main::TabContent;
use joy_editor::workspace::tool_bridge::ToolRegistry;
use joy_engine::egui;
use joy_engine::gameplay::parse_level;
use joy_engine::math::Vec2;
use joy_engine::runtime::App;

use crate::support::{
    Driver,
    TempFolder,
    editor,
    level_text,
    ui_shell,
};

fn sample_workspace() -> TempFolder {
    let workspace = TempFolder::new("workspace-ui");
    workspace.project("demo", "Demo Project", &[]);
    workspace.project("other", "Other Project", &[]);
    workspace.write("projects/demo/assets/levels/track.joylevel", &level_text("Track"));
    workspace.write("projects/demo/assets/levels/yard.joylevel", &level_text("Yard"));
    workspace.write("projects/demo/assets/effects/spark.joyfx", &crate::level_editor::spark_effect());
    workspace.write("projects/demo/assets/form/pillar.form", "box(0, 0.5, 0, 1, 1, 1)\n");
    workspace
}

#[test]
fn the_content_browser_opens_searches_filters_and_creates_assets() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = sample_workspace();
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));
    assert_eq!(driver.editor.workspace().files().len(), 4);
    assert!(driver.has("open-first") && driver.status().contains("Project opened"));
    driver.capture("workspace-empty");

    // Single click opens; the asset count and tab appear.
    driver.click("row:assets/levels/track.joylevel");
    assert_eq!(driver.editor.workspace().active_path(), Some("assets/levels/track.joylevel"));
    assert!(driver.has("tab:assets/levels/track.joylevel"));
    assert_eq!(driver.editor.workspace().tabs().len(), 1);
    driver.click("row:assets/levels/track.joylevel");
    assert_eq!(driver.editor.workspace().tabs().len(), 1, "opening an open asset activates its tab");

    // Search narrows rows; the type filter combines checked kinds.
    driver.type_into("search", "spark");
    assert!(driver.has("row:assets/effects/spark.joyfx") && !driver.has("row:assets/levels/yard.joylevel"));
    driver.type_into("search", "");
    driver.click("filter-toggle");
    driver.click("filter:joylevel");
    assert!(driver.has("row:assets/levels/yard.joylevel") && !driver.has("row:assets/form/pillar.form"));
    driver.click("filter:form");
    assert!(driver.has("row:assets/form/pillar.form") && !driver.has("row:assets/effects/spark.joyfx"));
    driver.capture("workspace-filter");
    driver.click("filter-all");
    assert!(driver.editor.workspace().filters().is_empty());
    driver.press("Escape", egui::Modifiers::NONE);
    assert!(driver.has("row:assets/effects/spark.joyfx"));

    // Collapsing the Content Browser hides it; Cmd/Ctrl+P reveals and focuses search.
    driver.click("assets-toggle");
    assert!(!driver.has("search"));
    driver.press("KeyP", egui::Modifiers::COMMAND);
    assert!(driver.has("search"));
    let focused = driver.ui().memory(|memory| memory.has_focus(egui::Id::new("search")));
    assert!(focused, "Cmd/Ctrl+P focuses search");

    // New creates a starter asset without overwriting and opens it.
    driver.click("new-asset");
    assert!(driver.has("new-asset-type"));
    driver.type_into("new-asset-name", "levels/created");
    driver.click("new-asset-create");
    assert!(
        workspace.exists("projects/demo/assets/levels/created.formlab"),
        "the default New type is a FORM LAB document"
    );
    assert_eq!(driver.editor.workspace().active_path(), Some("assets/levels/created.formlab"));
    assert_eq!(driver.status(), "Created assets/levels/created.formlab");
    driver.click("new-asset");
    driver.type_into("new-asset-name", "levels/created");
    driver.click("new-asset-create");
    assert_eq!(driver.status(), "A file with this name already exists.");
    driver.click("cancel-new-asset");
    driver.fill("search", "");
}

#[test]
fn rename_and_delete_confirm_and_leave_closed_files_closed() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = sample_workspace();
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));

    // F2 on a focused row renames; closed documents stay closed.
    driver.click("row:assets/levels/yard.joylevel");
    driver.click("close:assets/levels/yard.joylevel");
    assert!(driver.editor.workspace().tabs().is_empty());
    let row = driver.center("row:assets/levels/yard.joylevel");
    driver.pointer_move(row);
    driver.frames(1);
    driver.secondary_click("row:assets/levels/yard.joylevel");
    assert!(driver.has("rename-asset"), "secondary click opens the actions menu");
    driver.press("Escape", egui::Modifiers::NONE);
    driver.click("more:assets/levels/yard.joylevel");
    driver.click("rename-asset");
    assert!(driver.has("rename-field"));
    driver.fill("rename-field", "levels/renamed.joylevel");
    assert!(workspace.exists("projects/demo/assets/levels/renamed.joylevel"));
    assert!(!workspace.exists("projects/demo/assets/levels/yard.joylevel"));
    assert!(driver.editor.workspace().tabs().is_empty(), "rename leaves closed files closed");
    assert_eq!(driver.status(), "Renamed to assets/levels/renamed.joylevel");

    // An open document reopens at its new name.
    driver.click("row:assets/levels/renamed.joylevel");
    driver.press("F2", egui::Modifiers::NONE);
    assert!(driver.has("rename-field"));
    driver.fill("rename-field", "levels/final.joylevel");
    assert_eq!(driver.editor.workspace().active_path(), Some("assets/levels/final.joylevel"));

    // Delete asks first; cancelling keeps the file.
    driver.pointer_move(driver.center("row:assets/levels/final.joylevel"));
    driver.frames(2);
    driver.click("delete:assets/levels/final.joylevel");
    assert!(driver.has("dialog-confirm"));
    driver.click("dialog-cancel");
    assert!(workspace.exists("projects/demo/assets/levels/final.joylevel"));
    driver.pointer_move(driver.center("row:assets/levels/final.joylevel"));
    driver.frames(2);
    driver.click("delete:assets/levels/final.joylevel");
    driver.click("dialog-confirm");
    assert!(!workspace.exists("projects/demo/assets/levels/final.joylevel"));
    assert!(driver.editor.workspace().tabs().is_empty());
    assert_eq!(driver.status(), "Deleted assets/levels/final.joylevel");
}

#[test]
fn saves_conflicts_external_reloads_and_repair_views() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = sample_workspace();
    let file = "projects/demo/assets/levels/track.joylevel";
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));
    driver.click("row:assets/levels/track.joylevel");
    assert!(driver.has("save"));

    // An edit makes the tab dirty; Ctrl/Cmd+S writes the existing file.
    driver.click("add-box");
    driver.frames(2);
    assert!(driver.editor.workspace().active_tab().unwrap().dirty());
    driver.press("KeyS", egui::Modifiers::COMMAND);
    assert_eq!(parse_level(&workspace.read(file)).unwrap().entities.len(), 2);
    assert!(!driver.editor.workspace().active_tab().unwrap().dirty());
    let recovery = workspace.root.join("projects/demo/assets/levels/.joy-editor-recovery");
    assert_eq!(std::fs::read_dir(recovery).unwrap().count(), 1, "the replaced file is kept for recovery");

    // External edits reload clean tabs.
    let mut external = parse_level(&workspace.read(file)).unwrap();
    external.name = "External track".to_owned();
    workspace.write(file, &joy_engine::gameplay::serialize_level(&external));
    driver.editor.workspace_mut().poll();
    driver.frames(2);
    assert_eq!(driver.status(), "Reloaded external change: assets/levels/track.joylevel");
    assert!(driver.has("entity-count"));
    assert_eq!(level_name(&driver), "External track");

    // A dirty tab keeps its edits and offers the disk version.
    driver.click("add-sphere");
    driver.frames(2);
    external.name = "Second external".to_owned();
    workspace.write(file, &joy_engine::gameplay::serialize_level(&external));
    driver.editor.workspace_mut().poll();
    driver.frames(2);
    assert!(driver.has("reload-disk") && driver.has("keep-local"));
    driver.press("KeyS", egui::Modifiers::COMMAND);
    assert_eq!(driver.status(), "Resolve the disk conflict before saving.");
    driver.capture("workspace-conflict");
    driver.click("reload-disk");
    assert!(driver.has("dialog-confirm"), "using the disk version asks first");
    driver.click("dialog-confirm");
    assert_eq!(level_name(&driver), "Second external");
    assert!(!driver.has("reload-disk"));

    // Keep My Edits acknowledges the disk version; Save then replaces it.
    driver.click("add-sphere");
    driver.frames(2);
    workspace
        .write(file, &joy_engine::gameplay::serialize_level(&external).replace("Second external", "Third external"));
    driver.editor.workspace_mut().poll();
    driver.frames(2);
    driver.click("keep-local");
    assert!(driver.status().contains("Kept your edits"));
    driver.press("KeyS", egui::Modifiers::COMMAND);
    let saved = parse_level(&workspace.read(file)).unwrap();
    assert_eq!(saved.name, "Second external");
    assert_eq!(saved.entities.len(), 3);

    // Malformed disk content opens a repair view; saving valid source restores the editor.
    workspace.write(file, "{broken level");
    driver.editor.workspace_mut().poll();
    driver.frames(2);
    assert!(driver.has("repair:assets/levels/track.joylevel"));
    driver.capture("workspace-repair");
    {
        let tab = driver.editor.workspace_mut().tab_mut("assets/levels/track.joylevel").unwrap();
        let TabContent::Source(view) = &mut tab.content else { panic!("repair view") };
        view.text = level_text("Repaired");
        tab.document.edit(level_text("Repaired"));
    }
    driver.click("save");
    assert!(matches!(driver.editor.workspace().active_tab().unwrap().content, TabContent::Level(_)));
    assert_eq!(level_name(&driver), "Repaired");
    assert_eq!(driver.editor.workspace().active_tab().unwrap().document.text, level_text("Repaired"));
}

#[test]
fn unsaved_work_prompts_before_closing_switching_projects_or_quitting() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = sample_workspace();
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));
    driver.click("row:assets/levels/track.joylevel");
    driver.click("add-box");
    driver.frames(2);

    driver.click("close:assets/levels/track.joylevel");
    assert!(driver.has("dialog-confirm"));
    driver.click("dialog-cancel");
    assert_eq!(driver.editor.workspace().tabs().len(), 1);

    let outcome = driver.editor.workspace_mut().select_project("other", false);
    assert!(matches!(outcome, joy_editor::workspace::main::Outcome::Confirm(_)));
    assert_eq!(driver.editor.workspace().project(), "demo");

    assert!(!driver.editor.close_requested(), "closing the window with unsaved work asks first");
    driver.frames(2);
    assert!(driver.has("dialog-confirm"));
    driver.click("dialog-confirm");
    assert!(driver.editor.quit_confirmed());

    driver.click("close:assets/levels/track.joylevel");
    driver.click("dialog-confirm");
    assert!(driver.editor.workspace().tabs().is_empty());
    assert_eq!(parse_level(&workspace.read("projects/demo/assets/levels/track.joylevel")).unwrap().entities.len(), 1);
}

#[test]
fn sessions_restore_project_tabs_active_tab_and_panel_widths() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = sample_workspace();
    let storage = TempFolder::new("workspace-storage");
    {
        let mut driver =
            Driver::start(shell, editor(&workspace.root, Some(&storage.root), ToolRegistry::new(), Some("demo")));
        driver.click("row:assets/levels/track.joylevel");
        driver.click("row:assets/form/pillar.form");
        driver.click("tab:assets/levels/track.joylevel");
        let divider = driver.center("assets-divider");
        driver.drag(divider, divider + Vec2::new(60.0, 0.0), 0);
        let width = driver.editor.workspace().layout().content_browser_width;
        assert!(width > 260.0, "dragging the divider widens the Content Browser ({width})");
    }
    let Some(shell) = ui_shell() else {
        return;
    };
    let driver = Driver::start(shell, editor(&workspace.root, Some(&storage.root), ToolRegistry::new(), None));
    let workspace_state = driver.editor.workspace();
    assert_eq!(workspace_state.project(), "demo");
    let paths: Vec<&str> = workspace_state.tabs().iter().map(|tab| tab.document.path.as_str()).collect();
    assert_eq!(paths, ["assets/levels/track.joylevel", "assets/form/pillar.form"]);
    assert_eq!(workspace_state.active_path(), Some("assets/levels/track.joylevel"));
    assert!(workspace_state.layout().content_browser_width > 260.0);
}

#[test]
fn documentation_and_project_details_panels_open_beside_the_content_browser() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = sample_workspace();
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));
    driver.click("workspace-help-open");
    assert!(driver.has("workspace-help-close"));
    driver.capture("workspace-documentation");
    driver.click("workspace-help-close");
    assert!(!driver.has("workspace-help-close"));
    driver.click("workspace-help-open");
    driver.press("Escape", egui::Modifiers::NONE);
    assert!(!driver.has("workspace-help-close"), "Escape closes the documentation");
    driver.click("project-details");
    driver.frames(3);
    assert!(driver.editor.workspace().project_summary().unwrap().manifest.capabilities.len() == 1);
}

fn level_name(driver: &Driver) -> String {
    match &driver.editor.workspace().active_tab().unwrap().content {
        TabContent::Level(level) => level.document().level().name.clone(),
        _ => panic!("level tab"),
    }
}
