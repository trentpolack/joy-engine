// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Project file access (TypeScript `workspace.test.mjs` and the file half of `level.test.mjs`): discovery, the path
//! policy, revision-checked saves with recovery copies, lifecycle operations, and opened folders.

use std::cell::Cell;
use std::rc::Rc;

use joy_editor::server::project_files::{ProjectFiles, RECOVERY_DIRECTORY};
use joy_editor::server::{AssetEntry, ProjectError, ProjectStore};

use crate::support::{TempFolder, level_text, manifest};

#[test]
fn project_saves_preserve_bytes_reject_stale_and_concurrent_writes_and_confine_access() {
    let workspace = TempFolder::new("files");
    workspace.project("demo", "Demo", &[]);
    let asset = workspace.write("projects/demo/assets/form/tree.form", "box(1, 2, 3);\n");
    let mut files = ProjectFiles::new(&workspace.root);
    assert_eq!(files.list_projects().unwrap()[0].id, "demo");
    assert_eq!(files.list_files("demo").unwrap()[0].path, "assets/form/tree.form");

    // A hosting workspace with another layout names its projects folder explicitly.
    workspace.write("games/other/project.joyproject", &manifest("Other", &[]));
    let mut relocated = ProjectFiles::with_projects_directory(&workspace.root, "games");
    let ids: Vec<String> = relocated.list_projects().unwrap().into_iter().map(|project| project.id).collect();
    assert_eq!(ids, ["other"]);
    assert!(relocated.list_files("demo").is_err());

    // Two saves against the same revision: exactly one wins and its bytes are written.
    let original = files.read("demo", "assets/form/tree.form").unwrap();
    let first = files.save("demo", "assets/form/tree.form", "first\n", &original.revision);
    let second = files.save("demo", "assets/form/tree.form", "second\n", &original.revision);
    assert!(first.is_ok() && matches!(second, Err(ProjectError::Conflict { .. })));
    assert_eq!(std::fs::read_to_string(&asset).unwrap(), "first\n");
    let recovery = workspace.root.join("projects/demo/assets/form").join(RECOVERY_DIRECTORY);
    assert_eq!(std::fs::read_dir(&recovery).unwrap().count(), 1, "the replaced file is kept for recovery");
    assert_eq!(files.list_recovery("demo").unwrap().len(), 1);

    // Another editor writing while the save installs keeps its version and reports a conflict.
    let baseline = files.read("demo", "assets/form/tree.form").unwrap();
    let external = asset.clone();
    let fired = Rc::new(Cell::new(false));
    let hook_fired = fired.clone();
    files.on_save_prepared(move || {
        if !hook_fired.replace(true) {
            std::fs::write(&external, "external during save").unwrap();
        }
    });
    let error = files.save("demo", "assets/form/tree.form", "workspace", &baseline.revision).unwrap_err();
    assert!(error.to_string().contains("changed"), "{error}");
    assert!(matches!(error, ProjectError::Conflict { during_save: true, .. }));
    assert_eq!(std::fs::read_to_string(&asset).unwrap(), "external during save");
    let mut files = ProjectFiles::new(&workspace.root);
    assert!(
        files
            .save("demo", "assets/form/tree.form", "old", &original.revision)
            .unwrap_err()
            .to_string()
            .contains("changed")
    );

    // The path policy: traversal, project ids, symbolic links, and size.
    assert!(files.read("demo", "../project.joyproject").is_err());
    assert!(files.read("../demo", "assets/form/tree.form").is_err());
    assert!(files.read("demo", "assets/../project.joyproject").is_err());
    assert!(files.read("demo", "assets\\form\\tree.form").is_err());
    workspace.write("secret.form", "private");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(
            workspace.root.join("secret.form"),
            workspace.root.join("projects/demo/assets/form/link.form"),
        )
        .unwrap();
        assert!(files.read("demo", "assets/form/link.form").unwrap_err().to_string().contains("Symbolic links"));
    }
    let current = files.read("demo", "assets/form/tree.form").unwrap();
    assert!(files.save("demo", "assets/form/tree.form", &"x".repeat(6_000_001), &current.revision).is_err());
    let listed: Vec<String> = files.list_files("demo").unwrap().into_iter().map(|file| file.path).collect();
    assert_eq!(listed, ["assets/form/tree.form"], "links and recovery copies are not listed");
}

#[test]
fn manifests_asset_lifecycle_and_recovery_copies_stay_project_owned() {
    let workspace = TempFolder::new("lifecycle");
    let external = TempFolder::new("external");
    external.write("project.joyproject", &manifest("External Demo", &[".note"]));
    external.write("assets/example.note", "first");
    let mut files = ProjectFiles::new(&workspace.root);
    let project = files.open_folder(external.root.to_str().unwrap()).unwrap();
    assert_eq!(project.name, "External Demo");
    assert!(project.external);
    assert_eq!(
        files.list_files(&project.id).unwrap(),
        [AssetEntry { path: "assets/example.note".into(), kind: "note".into() }]
    );
    assert!(files.list_projects().unwrap().iter().any(|listed| listed.id == project.id));

    let original = files.read(&project.id, "assets/example.note").unwrap();
    files.save(&project.id, "assets/example.note", "second", &original.revision).unwrap();
    assert_eq!(files.list_recovery(&project.id).unwrap().len(), 1);
    files.rename(&project.id, "assets/example.note", "assets/renamed.note").unwrap();
    files.create(&project.id, "assets/created.note", "created").unwrap();
    let listed: Vec<String> = files.list_files(&project.id).unwrap().into_iter().map(|file| file.path).collect();
    assert_eq!(listed, ["assets/created.note", "assets/renamed.note"]);
    assert_eq!(
        files.rename(&project.id, "assets/created.note", "assets/renamed.note"),
        Err(ProjectError::AlreadyExists)
    );
    files.remove(&project.id, "assets/created.note").unwrap();
    assert_eq!(files.remove_recovery(&project.id, None).unwrap(), 1);
    assert!(files.list_recovery(&project.id).unwrap().is_empty());
    assert!(files.remove_recovery(&project.id, Some("assets/.joy-editor-recovery/missing")).is_err());

    // A folder without a manifest is not a project.
    let plain = TempFolder::new("plain");
    assert!(
        files
            .open_folder(plain.root.to_str().unwrap())
            .unwrap_err()
            .to_string()
            .contains("not a compatible Joy project")
    );
}

#[test]
fn new_levels_use_exclusive_creation_and_reject_traversal_links_and_invalid_documents() {
    let workspace = TempFolder::new("levels");
    workspace.project("demo", "Demo", &[]);
    let mut files = ProjectFiles::new(&workspace.root);
    let text = level_text("Track");
    assert!(files.create("demo", "assets/track.joylevel", &text).is_ok());
    assert_eq!(files.create("demo", "assets/track.joylevel", &text), Err(ProjectError::AlreadyExists));
    assert_eq!(workspace.read("projects/demo/assets/track.joylevel"), text);
    let original = files.read("demo", "assets/track.joylevel").unwrap();
    files.save("demo", "assets/track.joylevel", &format!("{text}\n"), &original.revision).unwrap();
    assert!(
        files
            .save("demo", "assets/track.joylevel", &text, &original.revision)
            .unwrap_err()
            .to_string()
            .contains("changed")
    );
    assert!(files.create("demo", "assets/../evil.joylevel", &text).is_err());
    assert!(files.create("demo", "assets/code.js", &text).is_err());
    assert!(files.create("demo", "elsewhere/level.joylevel", &text).is_err());
    assert!(files.create("demo", "assets/bad.joylevel", "{broken").is_err());
    assert!(files.create("demo", "assets/bad.joyobject", "{\"version\": 1}").is_err());
    #[cfg(unix)]
    {
        std::fs::create_dir_all(workspace.root.join("outside")).unwrap();
        std::os::unix::fs::symlink(workspace.root.join("outside"), workspace.root.join("projects/demo/assets/link"))
            .unwrap();
        assert!(files.create("demo", "assets/link/escape.joylevel", &text).is_err());
        std::os::unix::fs::symlink(
            workspace.root.join("outside/missing"),
            workspace.root.join("projects/demo/assets/symlink.joylevel"),
        )
        .unwrap();
        assert!(files.create("demo", "assets/symlink.joylevel", &text).is_err());
        assert!(!workspace.exists("outside/missing") && !workspace.exists("outside/escape.joylevel"));
    }
}

#[test]
fn discovery_requires_valid_manifests_and_sorts_projects_by_name() {
    let workspace = TempFolder::new("discovery");
    workspace.project("zeta", "Alpha Playground", &[]);
    workspace.project("alpha", "Zebra Garden", &[]);
    workspace.write("projects/no-manifest/package.json", "{}");
    workspace.write("projects/broken/project.joyproject", "{\"schemaVersion\": 2}");
    workspace.write("projects/bad name/project.joyproject", &manifest("Spaced", &[]));
    let mut files = ProjectFiles::new(&workspace.root);
    let names: Vec<String> = files.list_projects().unwrap().into_iter().map(|project| project.name).collect();
    assert_eq!(names, ["Alpha Playground", "Zebra Garden"]);
    assert!(files.list_files("bad name").unwrap_err().to_string().contains("Invalid project"));

    // Manifest capabilities add listed formats; hidden folders are skipped.
    workspace.project("notes", "Notes", &[".note"]);
    workspace.write("projects/notes/assets/readme.note", "hello");
    workspace.write("projects/notes/assets/.hidden/secret.note", "hidden");
    workspace.write("projects/notes/assets/code.js", "nope");
    let listed: Vec<String> = files.list_files("notes").unwrap().into_iter().map(|file| file.path).collect();
    assert_eq!(listed, ["assets/readme.note"]);
}
