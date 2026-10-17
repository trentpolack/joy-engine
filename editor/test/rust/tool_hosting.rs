// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Hosting authoring tools as tabs through `AuthoringTool`, against a small test tool: tools start with the shared
//! renderer, open documents, draw in the tab area, receive phases only while active, re-apply settings when activated,
//! save through the workspace, and fall back to a repair view when they reject a document.

use std::cell::RefCell;
use std::rc::Rc;

use joy_editor::workspace::main::TabContent;
use joy_editor::workspace::tool_bridge::ToolRegistry;
use joy_engine::development::{
    AuthoringTool,
    ToolDocument,
    ToolHosting,
};
use joy_engine::egui;
use joy_engine::runtime::{
    Frame,
    Game,
    GameResult,
    RenderTarget,
    StartContext,
};

use crate::support::{
    Driver,
    TempFolder,
    editor,
    level_text,
    ui_shell,
};

/// What one test tool instance observed.
#[derive(Debug, Default)]
pub struct StubState {
    pub starts: u32,
    pub activations: u32,
    pub frames: u32,
    pub renders: u32,
    pub uis: u32,
    pub hosting: Option<egui::Rect>,
    pub button: Option<egui::Rect>,
    pub text: String,
    pub saved: String,
    pub opened: Vec<String>,
}

/// A minimal authoring tool: holds the document text and appends to it from a button in its tab area.
pub struct StubTool {
    pub state: Rc<RefCell<StubState>>,
}

impl Game for StubTool {
    fn start(&mut self, _context: &mut StartContext<'_>) -> GameResult {
        self.state.borrow_mut().starts += 1;
        Ok(())
    }

    fn simulation(&mut self, _frame: &mut Frame<'_>) -> GameResult {
        self.state.borrow_mut().frames += 1;
        Ok(())
    }

    fn render(&mut self, _frame: &mut Frame<'_>, _target: &mut RenderTarget<'_>) -> GameResult {
        self.state.borrow_mut().renders += 1;
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, _frame: &mut Frame<'_>) {
        let mut state = self.state.borrow_mut();
        state.uis += 1;
        let Some(area) = state.hosting else {
            return;
        };
        let text = state.text.clone();
        let button = egui::Area::new(egui::Id::new("stub-tool"))
            .fixed_pos(area.min + egui::vec2(20.0, 20.0))
            .show(context, |ui| {
                ui.label(format!("STUB LAB · {text}"));
                ui.button("Append")
            })
            .inner;
        state.button = Some(button.rect);
        if button.clicked() {
            state.text.push_str(" edited");
        }
    }
}

impl AuthoringTool for StubTool {
    fn tool_name(&self) -> &'static str {
        "STUB LAB"
    }

    fn document_extensions(&self) -> &'static [&'static str] {
        &["form", "formlab", "joyfx"]
    }

    fn open_document(&mut self, document: ToolDocument) -> Result<(), String> {
        if document.contents.contains("INVALID") {
            return Err("Stub cannot read this document.".to_owned());
        }
        let mut state = self.state.borrow_mut();
        state.opened.push(document.name);
        state.text = document.contents.clone();
        state.saved = document.contents;
        Ok(())
    }

    fn save_document(&mut self, _name: &str) -> Result<String, String> {
        Ok(self.state.borrow().text.clone())
    }

    fn has_unsaved_changes(&self) -> bool {
        let state = self.state.borrow();
        state.text != state.saved
    }

    fn mark_saved(&mut self) {
        let mut state = self.state.borrow_mut();
        state.saved = state.text.clone();
    }

    fn set_hosting(&mut self, hosting: ToolHosting) {
        if let ToolHosting::Hosted { area } = hosting {
            self.state.borrow_mut().hosting = Some(area);
        }
    }

    fn activated(&mut self, _context: &mut StartContext<'_>) {
        self.state.borrow_mut().activations += 1;
    }
}

/// Every stub instance's shared state, in creation order.
pub type StubInstances = Rc<RefCell<Vec<Rc<RefCell<StubState>>>>>;

/// A registry with the test tool; `instances` collects every created instance's state (the first is the probe).
pub fn stub_tools() -> (ToolRegistry, StubInstances) {
    let instances: StubInstances = Rc::default();
    let created = instances.clone();
    let mut tools = ToolRegistry::new();
    tools.register(move || {
        let state = Rc::new(RefCell::new(StubState::default()));
        created.borrow_mut().push(state.clone());
        Box::new(StubTool { state })
    });
    (tools, instances)
}

fn workspace_with_tools() -> TempFolder {
    let workspace = TempFolder::new("hosting");
    workspace.project("demo", "Demo", &[]);
    workspace.write("projects/demo/assets/tower.formlab", "{\"version\": 1, \"source\": \"box(0, 0, 0, 1, 1, 1)\"}\n");
    workspace.write("projects/demo/assets/spark.joyfx", "{\"name\": \"spark\"}\n");
    workspace.write("projects/demo/assets/broken.formlab", "INVALID\n");
    workspace.write("projects/demo/assets/track.joylevel", &level_text("Track"));
    workspace
}

#[test]
fn hosted_tools_start_open_draw_in_the_tab_and_suspend_while_inactive() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = workspace_with_tools();
    let (tools, instances) = stub_tools();
    let mut driver = Driver::start(shell, editor(&workspace.root, None, tools, Some("demo")));
    assert_eq!(driver.editor.workspace().tool_names(), ["STUB LAB"]);

    driver.click("row:assets/tower.formlab");
    let tower = instances.borrow().last().cloned().unwrap();
    {
        let state = tower.borrow();
        assert_eq!(state.starts, 1, "the tool started once with the shared renderer");
        assert_eq!(state.opened, ["tower.formlab"]);
        assert!(state.activations >= 1, "activation re-applies renderer settings");
        assert!(state.frames > 0 && state.renders > 0 && state.uis > 0);
        let area = state.hosting.expect("hosted area");
        let document = driver.editor.view().document_rect;
        assert_eq!(area, document, "the tool draws in the tab area");
        assert!(area.min.x >= 160.0 && area.min.y > 52.0, "the tab area leaves the editor chrome visible");
    }

    driver.click("row:assets/spark.joyfx");
    let spark = instances.borrow().last().cloned().unwrap();
    let suspended = tower.borrow().frames;
    let activations = tower.borrow().activations;
    driver.frames(5);
    assert_eq!(tower.borrow().frames, suspended, "inactive tabs receive no phases");
    assert!(spark.borrow().frames >= 5);

    driver.click("tab:assets/tower.formlab");
    assert!(tower.borrow().activations > activations, "returning to a tab activates it again");
    assert!(tower.borrow().frames > suspended);
    assert_eq!(tower.borrow().starts, 1, "tabs keep their tool and state");
    driver.capture("hosting-stub-tool");
}

#[test]
fn hosted_edits_save_through_the_workspace_and_rejected_documents_open_for_repair() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = workspace_with_tools();
    let (tools, instances) = stub_tools();
    let mut driver = Driver::start(shell, editor(&workspace.root, None, tools, Some("demo")));
    driver.click("row:assets/tower.formlab");
    let tower = instances.borrow().last().cloned().unwrap();

    // The tool's own button, inside its tab area, receives the click.
    let area = tower.borrow().hosting.unwrap();
    let button = tower.borrow().button.unwrap();
    assert!(area.contains_rect(button), "the tool draws inside its tab area");
    driver.click_at(joy_engine::glam::Vec2::new(button.center().x, button.center().y));
    assert!(tower.borrow().text.ends_with(" edited"), "the hosted tool's button receives synthetic clicks");
    assert!(driver.editor.workspace().active_tab().unwrap().dirty());
    driver.press("KeyS", egui::Modifiers::COMMAND);
    assert!(
        workspace.read("projects/demo/assets/tower.formlab").ends_with(" edited"),
        "Save writes the tool's document"
    );
    assert!(!driver.editor.workspace().active_tab().unwrap().dirty(), "Save marks the tool saved");
    assert_eq!(driver.status(), "Saved assets/tower.formlab");

    // A document the tool rejects opens as editable source; saving a fix reopens the tool.
    driver.click("row:assets/broken.formlab");
    assert!(driver.has("repair:assets/broken.formlab"), "{:?}", driver.editor.view().controls.names());
    assert!(driver.status().contains("Stub cannot read this document."));
    {
        let tab = driver.editor.workspace_mut().tab_mut("assets/broken.formlab").unwrap();
        let TabContent::Source(view) = &mut tab.content else { panic!("repair view") };
        view.text = "FIXED\n".to_owned();
        tab.document.edit("FIXED\n");
    }
    driver.press("KeyS", egui::Modifiers::COMMAND);
    driver.frames(2);
    assert_eq!(workspace.read("projects/demo/assets/broken.formlab"), "FIXED\n");
    let tab = driver.editor.workspace().tab("assets/broken.formlab").unwrap();
    assert!(matches!(tab.content, TabContent::Tool(_)), "the repaired document reopens in its tool");
    assert_eq!(instances.borrow().last().unwrap().borrow().text, "FIXED\n");
}

#[test]
fn formats_without_a_tool_open_as_editable_source() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = workspace_with_tools();
    let mut driver = Driver::start(shell, editor(&workspace.root, None, ToolRegistry::new(), Some("demo")));
    driver.click("row:assets/spark.joyfx");
    let tab = driver.editor.workspace().active_tab().unwrap();
    assert!(
        matches!(&tab.content, TabContent::Source(view) if view.message.as_deref().is_some_and(|message| message.contains("No tool for .joyfx")))
    );
    assert!(driver.has("repair:assets/spark.joyfx"));
}

#[test]
fn form_lab_opens_form_documents_as_a_hosted_tab() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = TempFolder::new("form-lab");
    workspace.project("demo", "Demo", &[]);
    let script = "project meta(name=\"Pillar\")\nparam height = 2 meta(min=0.5, max=5, step=0.1)\nbox(0, height*0.5, 0, 1, height, 1)\n";
    workspace.write("projects/demo/assets/pillar.form", script);
    let mut tools = ToolRegistry::new();
    tools.register(|| {
        Box::new(form_lab::FormLab::with_storage(ToolHosting::Hosted { area: egui::Rect::NOTHING }, None))
    });
    let mut driver = Driver::start(shell, editor(&workspace.root, None, tools, Some("demo")));
    assert_eq!(driver.editor.workspace().tool_names(), ["FORM LAB"]);

    driver.click("row:assets/pillar.form");
    driver.frames(10);
    let document = driver.editor.view().document_rect;
    let tab = driver.editor.workspace().active_tab().unwrap();
    let TabContent::Tool(tool) = &tab.content else { panic!("FORM LAB tab, found a repair view: {}", driver.status()) };
    assert_eq!(tool.tool_name(), "FORM LAB");
    assert!(tool.started());
    assert_eq!(tool.area(), Some(document), "FORM LAB draws in the tab area");
    assert!(!tab.dirty(), "opening a document leaves it clean");
    driver.capture("form-lab-tab");

    // Saving writes FORM LAB's own serialization of the document back through the workspace.
    driver.press("KeyS", egui::Modifiers::COMMAND);
    assert!(workspace.read("projects/demo/assets/pillar.form").contains("box(0, height*0.5, 0, 1, height, 1)"));
    driver.click("close:assets/pillar.form");
    assert!(driver.editor.workspace().tabs().is_empty());
}

#[test]
fn particle_lab_opens_effects_as_a_hosted_tab_beside_form_lab() {
    let Some(shell) = ui_shell() else {
        return;
    };
    let workspace = TempFolder::new("particle-lab");
    workspace.project("demo", "Demo", &[]);
    workspace.write("projects/demo/assets/spark.joyfx", &crate::level_editor::spark_effect());
    workspace.write("projects/demo/assets/pillar.form", "box(0, 0.5, 0, 1, 1, 1)\n");
    let mut tools = ToolRegistry::new();
    tools.register(|| {
        Box::new(form_lab::FormLab::with_storage(ToolHosting::Hosted { area: egui::Rect::NOTHING }, None))
    });
    tools.register(|| Box::new(particle_lab::ParticleLabTool::with_storage(None)));
    let mut driver = Driver::start(shell, editor(&workspace.root, None, tools, Some("demo")));
    assert_eq!(driver.editor.workspace().tool_names(), ["FORM LAB", "PARTICLE LAB"]);

    driver.click("row:assets/spark.joyfx");
    driver.frames(10);
    let document = driver.editor.view().document_rect;
    {
        let tab = driver.editor.workspace().active_tab().unwrap();
        let TabContent::Tool(tool) = &tab.content else {
            panic!("PARTICLE LAB tab, found a repair view: {}", driver.status())
        };
        assert_eq!(tool.tool_name(), "PARTICLE LAB");
        assert!(tool.started() && tool.area() == Some(document), "PARTICLE LAB draws in the tab area");
        assert!(!tab.dirty(), "opening an effect leaves it clean");
    }
    driver.capture("particle-lab-tab");

    // Both labs keep their own tabs; switching suspends one and resumes the other.
    driver.click("row:assets/pillar.form");
    driver.frames(4);
    assert_eq!(driver.editor.workspace().tabs().len(), 2);
    driver.click("tab:assets/spark.joyfx");
    driver.frames(2);
    let tab = driver.editor.workspace().active_tab().unwrap();
    assert!(matches!(&tab.content, TabContent::Tool(tool) if tool.tool_name() == "PARTICLE LAB"));
    driver.press("KeyS", egui::Modifiers::COMMAND);
    let saved = workspace.read("projects/demo/assets/spark.joyfx");
    assert!(joy_engine::particles::parse_particle_effect(&saved).is_ok(), "PARTICLE LAB saves a valid effect: {saved}");
}
