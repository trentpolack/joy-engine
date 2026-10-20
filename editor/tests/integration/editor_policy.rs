// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Locating the hosting workspace (the production availability policy belongs to site builds): the nearest
//! `joy.joyconfig`, `--workspace`, and `JOY_EDITOR_WORKSPACE`.

use joy_editor::desktop::workspace::{
    WorkspaceSource,
    find_workspace_root,
    launch_option,
    locate_workspace,
};

use crate::support::TempFolder;

#[test]
fn the_hosting_workspace_is_found_from_nested_folders() {
    let root = TempFolder::new("policy");
    let nested = root.root.join("joy-engine/tools/form-lab/src");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(find_workspace_root(&nested), None);
    root.write("joy.joyconfig", "{}");
    assert_eq!(find_workspace_root(&nested).as_deref(), Some(root.root.as_path()));

    let located = locate_workspace(&[], None, &nested).unwrap();
    assert_eq!((located.root.as_path(), located.source), (root.root.as_path(), WorkspaceSource::Config));
}

#[test]
fn arguments_and_the_environment_choose_the_workspace_first() {
    let first = TempFolder::new("argument");
    let second = TempFolder::new("environment");
    let arguments = vec![format!("--workspace={}", first.root.display())];
    let located = locate_workspace(&arguments, Some(second.root.to_str().unwrap()), &second.root).unwrap();
    assert_eq!((located.root, located.source), (first.root.clone(), WorkspaceSource::Argument));
    let located = locate_workspace(&[], Some(second.root.to_str().unwrap()), &first.root).unwrap();
    assert_eq!((located.root, located.source), (second.root.clone(), WorkspaceSource::Environment));
    let missing = vec!["--workspace".to_owned(), first.root.join("missing").display().to_string()];
    assert!(locate_workspace(&missing, None, &first.root).unwrap_err().contains("is not a folder"));
    let message = locate_workspace(&[], None, &first.root).unwrap_err();
    assert!(
        message.contains("--workspace")
            && message.contains("JOY_EDITOR_WORKSPACE")
            && message.contains("joy.joyconfig")
    );

    let arguments: Vec<String> = ["--project", "god-game", "--examples"].map(str::to_owned).to_vec();
    assert_eq!(launch_option(&arguments, "project").as_deref(), Some("god-game"));
    assert_eq!(launch_option(&["--project=luna".to_owned()], "project").as_deref(), Some("luna"));
    assert_eq!(launch_option(&arguments, "workspace"), None);
}
