// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The native Joy Editor:
//!
//!     cargo run -p joy-editor -- [--workspace=<folder>] [--project=<id>] [--examples]
//!
//! The hosting workspace comes from `--workspace`, `JOY_EDITOR_WORKSPACE`, or the nearest `joy.joyconfig` above the
//! current folder; `--examples` opens the bundled read-only examples instead. Browsers start through the library's
//! `web_main!` entry.

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    use joy_editor::desktop::workspace::{WORKSPACE_ENVIRONMENT_VARIABLE, launch_option, locate_workspace};
    use joy_editor::server::BundledProjects;
    use joy_editor::server::ProjectStore;
    use joy_editor::server::project_files::ProjectFiles;
    use joy_editor::workspace::constants::STORAGE_APPLICATION;
    use joy_editor::{EditorOptions, JoyEditor, default_tools, shell_options};

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments.iter().any(|argument| argument == "--help" || argument == "-h") {
        println!("joy-editor [--workspace=<folder>] [--project=<id>] [--examples]");
        return;
    }
    let current = std::env::current_dir().unwrap_or_default();
    let environment = std::env::var(WORKSPACE_ENVIRONMENT_VARIABLE).ok();
    let (store, message): (Box<dyn ProjectStore>, Option<String>) =
        if arguments.iter().any(|argument| argument == "--examples") {
            (Box::new(BundledProjects::new()), None)
        } else {
            match locate_workspace(&arguments, environment.as_deref(), &current) {
                Ok(workspace) => (Box::new(ProjectFiles::new(workspace.root)), None),
                Err(message) => (Box::new(ProjectFiles::new(current)), Some(message)),
            }
        };
    let options = EditorOptions {
        store,
        storage: Some(joy_engine::runtime::Storage::new(STORAGE_APPLICATION)),
        tools: default_tools(),
        project: launch_option(&arguments, "project"),
        startup_message: message,
    };
    if let Err(error) = joy_engine::run(shell_options(), JoyEditor::new(options)) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}
