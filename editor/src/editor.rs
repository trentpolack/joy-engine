// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! [`JoyEditor`]: the workspace as a `joy_engine::App`. The shell owns the window (or browser canvas), the renderer,
//! and input; the editor draws its chrome with egui and drives only the active tab: the level editor renders its
//! viewport through the shared renderer, and a hosted tool receives its frame phases with the tab area as its hosting
//! rectangle. Inactive tabs receive nothing, so their previews suspend.

use joy_engine::egui;
use joy_engine::rendering::GpuCapabilities;
use joy_engine::runtime::{
    App,
    AppResult,
    Frame,
    RenderTarget,
    StartContext,
    Storage,
};

use crate::server::ProjectStore;
use crate::workspace::main::{
    TabContent,
    Workspace,
};
use crate::workspace::tool_bridge::ToolRegistry;
use crate::workspace::ui::WorkspaceView;

/// How to build an editor.
pub struct EditorOptions {
    /// Where projects and assets come from.
    pub store: Box<dyn ProjectStore>,
    /// Where the session is remembered; `None` keeps it in memory only.
    pub storage: Option<Storage>,
    /// Tools hosted for `.form`, `.formlab`, `.joyfx`, and any other registered extensions.
    pub tools: ToolRegistry,
    /// The project to open first, overriding the restored session.
    pub project: Option<String>,
    /// A message for the status line at startup (for example, why no hosting workspace was found).
    pub startup_message: Option<String>,
}

/// The Joy Editor application.
pub struct JoyEditor {
    workspace: Workspace,
    view: WorkspaceView,
    requested_project: Option<String>,
    startup_message: Option<String>,
    quit_confirmed: bool,
    capabilities: Option<GpuCapabilities>,
    themed: bool,
}

impl JoyEditor {
    /// An editor over `options.store`; projects are discovered when the shell starts it.
    pub fn new(options: EditorOptions) -> Self {
        Self {
            view: WorkspaceView::new(options.storage.clone()),
            workspace: Workspace::new(options.store, options.storage, options.tools),
            requested_project: options.project,
            startup_message: options.startup_message,
            quit_confirmed: false,
            capabilities: None,
            themed: false,
        }
    }

    /// The workspace model.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// The workspace model, mutably.
    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspace
    }

    /// The workspace UI state.
    pub fn view(&self) -> &WorkspaceView {
        &self.view
    }

    /// The rectangle of a named control from the latest UI pass (logical points), such as `save`, `search`,
    /// `row:assets/level.joylevel`, `add-box`, or `viewport`.
    pub fn control(&self, name: &str) -> Option<egui::Rect> {
        self.view.controls.get(name)
    }

    /// Whether the author confirmed quitting.
    pub fn quit_confirmed(&self) -> bool {
        self.quit_confirmed
    }

    /// The GPU capabilities the shell started with.
    pub fn capabilities(&self) -> Option<&GpuCapabilities> {
        self.capabilities.as_ref()
    }

    /// Report a hosted tool's phase failure without stopping the editor.
    fn report(&mut self, result: AppResult) {
        if let Err(error) = result {
            let path = self.workspace.active_path().unwrap_or_default().to_owned();
            self.workspace.set_status(format!("{path}: {error}"));
        }
    }
}

impl App for JoyEditor {
    fn start(&mut self, context: &mut StartContext<'_>) -> AppResult {
        self.capabilities = Some(context.capabilities.clone());
        self.workspace.start(self.requested_project.take().as_deref());
        if let Some(message) = self.startup_message.take() {
            self.workspace.set_status(message);
        }
        Ok(())
    }

    fn input(&mut self, frame: &mut Frame<'_>) -> AppResult {
        if let Some(TabContent::Tool(tool)) = self.workspace.active_tab_mut().map(|tab| &mut tab.content) {
            let result = tool.tool_mut().input(frame);
            self.report(result);
        }
        Ok(())
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> AppResult {
        self.workspace.tick(frame.time.time_seconds);
        let result = match self.workspace.active_tab_mut().map(|tab| &mut tab.content) {
            Some(TabContent::Level(level)) => {
                level.step(f64::from(frame.time.elapsed_seconds));
                Ok(())
            }
            Some(TabContent::Tool(tool)) => tool.tool_mut().simulation(frame),
            _ => Ok(()),
        };
        self.report(result);
        Ok(())
    }

    fn presentation(&mut self, frame: &mut Frame<'_>) -> AppResult {
        self.workspace.sync_active_level();
        if let Some(TabContent::Tool(tool)) = self.workspace.active_tab_mut().map(|tab| &mut tab.content) {
            let result = tool.tool_mut().presentation(frame);
            self.report(result);
        }
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        let capabilities = target.renderer().gpu().capabilities().clone();
        {
            let mut context =
                StartContext { capabilities: &capabilities, renderer: target.renderer(), viewport: frame.viewport };
            self.workspace.prepare_tabs(&mut context);
        }
        let time = frame.time.time_seconds as f32;
        let logical_size = target.renderer().logical_size();
        let result = match self.workspace.active_tab_mut().map(|tab| &mut tab.content) {
            Some(TabContent::Level(level)) => {
                level.viewport.viewport_size = logical_size;
                level.viewport.render(target, time).map_err(Into::into)
            }
            Some(TabContent::Tool(tool)) => tool.tool_mut().render(frame, target),
            _ => Ok(()),
        };
        self.report(result);
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, frame: &mut Frame<'_>) {
        if !self.themed {
            joy_engine::runtime::theme::apply_authoring(context);
            self.themed = true;
        }
        self.view.show(context, &mut self.workspace);
        if self.view.take_commands().quit {
            self.quit_confirmed = true;
            frame.request_exit();
        }
        let area = self.view.document_rect;
        if let Some(TabContent::Tool(tool)) = self.workspace.active_tab_mut().map(|tab| &mut tab.content)
            && tool.started()
        {
            tool.set_area(area);
            tool.tool_mut().ui(context, frame);
        }
    }

    fn after_frame(&mut self, frame: &mut Frame<'_>) -> AppResult {
        if let Some(TabContent::Tool(tool)) = self.workspace.active_tab_mut().map(|tab| &mut tab.content) {
            let result = tool.tool_mut().after_frame(frame);
            self.report(result);
        }
        Ok(())
    }

    fn visibility_changed(&mut self, visible: bool) {
        if let Some(TabContent::Tool(tool)) = self.workspace.active_tab_mut().map(|tab| &mut tab.content) {
            tool.tool_mut().visibility_changed(visible);
        }
    }

    fn close_requested(&mut self) -> bool {
        if self.quit_confirmed || self.workspace.unsaved_paths().is_empty() {
            return true;
        }
        self.view.confirm_quit();
        false
    }
}
