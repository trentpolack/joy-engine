// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The development console: a drop-down command line over the game, toggled with the backquote key, after Unreal
//! Engine's. Typing a variable's name prints its value, what it accepts, and its help; a name followed by a value sets
//! it. Built-in commands list and search variables, reset them, clear the output, and toggle the `stat` overlays. Up
//! and Down walk the command history, Tab completes names, and Escape closes the console.

use std::collections::VecDeque;

use egui::{
    Color32,
    RichText,
};

use crate::console_variables::{
    ConsoleVariables,
    VariableDefinition,
};
use crate::dock;
use crate::render_variables;

/// Built-in commands.
const HELP: &str = "help";
const FIND: &str = "find";
const RESET: &str = "reset";
const CLEAR: &str = "clear";
const STAT: &str = "stat";
const COMMANDS: [&str; 5] = [HELP, FIND, RESET, CLEAR, STAT];

/// `stat` arguments.
const STAT_UNIT: &str = "unit";
const STAT_GPU: &str = "gpu";
const STAT_NONE: &str = "none";

/// Output lines kept, oldest dropped first.
const MAX_OUTPUT_LINES: usize = 256;

/// Commands kept for recall.
const MAX_HISTORY: usize = 64;

/// The key that toggles the console, which never belongs in a typed line.
const TOGGLE_CHARACTER: char = '`';

/// Fraction of the screen height the console covers, within these logical-pixel bounds.
const HEIGHT_FRACTION: f32 = 0.4;
const MIN_HEIGHT: f32 = 160.0;
const MAX_HEIGHT: f32 = 440.0;

/// What produced an output line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    /// An echoed command.
    Command,
    /// A result.
    Info,
    /// A refused command or value.
    Error,
}

/// One line of console output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleLine {
    /// What produced it.
    pub kind: LineKind,
    /// Its text.
    pub text: String,
}

/// The `stat` overlays the console has turned on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatOverlays {
    /// Frame, CPU, and GPU time (`stat unit`).
    pub unit: bool,
    /// Per-phase CPU and GPU time (`stat gpu`).
    pub gpu: bool,
}

/// Console state: the variables, the open drop-down, its line, output, and history.
#[derive(Clone, Debug)]
pub struct Console {
    variables: ConsoleVariables,
    open: bool,
    /// Whether the line should take keyboard focus on the next UI pass.
    focus_pending: bool,
    input: String,
    output: VecDeque<ConsoleLine>,
    history: Vec<String>,
    /// The history entry being recalled, counted back from the newest.
    recalled: Option<usize>,
    stats: StatOverlays,
}

impl Default for Console {
    fn default() -> Self {
        Self::new()
    }
}

impl Console {
    /// A closed console with the engine's rendering variables registered.
    pub fn new() -> Self {
        let mut variables = ConsoleVariables::new();
        render_variables::register(&mut variables);
        Self {
            variables,
            open: false,
            focus_pending: false,
            input: String::new(),
            output: VecDeque::new(),
            history: Vec::new(),
            recalled: None,
            stats: StatOverlays::default(),
        }
    }

    /// The registered variables.
    pub fn variables(&self) -> &ConsoleVariables {
        &self.variables
    }

    /// The registered variables, for registering a game's own and reading changes.
    pub fn variables_mut(&mut self) -> &mut ConsoleVariables {
        &mut self.variables
    }

    /// Whether the drop-down is showing.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Show or hide the drop-down; showing it focuses the line.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        self.focus_pending = self.open;
    }

    /// The `stat` overlays turned on.
    pub fn stats(&self) -> StatOverlays {
        self.stats
    }

    /// Logical pixels the open drop-down covers from the top of `screen`; zero while closed.
    pub(crate) fn covered_height(&self, screen: egui::Rect) -> f32 {
        if self.open { drop_down_height(screen) } else { 0.0 }
    }

    /// Output lines, oldest first.
    pub fn output(&self) -> impl Iterator<Item = &ConsoleLine> {
        self.output.iter()
    }

    /// Print a line, as commands and the variables' owners do.
    pub fn print(&mut self, kind: LineKind, text: impl Into<String>) {
        if self.output.len() == MAX_OUTPUT_LINES {
            self.output.pop_front();
        }
        self.output.push_back(ConsoleLine {
            kind,
            text: text.into(),
        });
    }

    /// Run one command line: a built-in command, a variable name (print it), or a name and a value (set it).
    pub fn execute(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.remember(line);
        self.print(LineKind::Command, format!("> {line}"));
        let (command, argument) = match line.split_once(char::is_whitespace) {
            Some((command, argument)) => (command, argument.trim()),
            None => (line, ""),
        };
        match command.to_ascii_lowercase().as_str() {
            HELP => self.help(),
            FIND => self.find(argument),
            RESET => self.reset(argument),
            CLEAR => self.output.clear(),
            STAT => self.stat(argument),
            _ => self.variable(command, argument),
        }
    }

    /// Draw the drop-down when open. Runs commands entered this pass.
    pub(crate) fn ui(&mut self, context: &egui::Context) {
        if !self.open {
            return;
        }

        // Console keys act before the line sees them: Tab completes, Up and Down recall, Escape closes.
        let (tab, up, down, escape) = context.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if escape {
            self.open = false;
            return;
        }
        if tab {
            self.complete();
        }
        if up || down {
            self.recall(up);
        }

        let screen = context.content_rect();
        let height = drop_down_height(screen);
        let fill =
            Color32::from_rgba_unmultiplied(dock::BACKGROUND.r(), dock::BACKGROUND.g(), dock::BACKGROUND.b(), 235);
        egui::Area::new(egui::Id::new("joy-console")).order(egui::Order::Foreground).fixed_pos(screen.min).show(
            context,
            |ui| {
                egui::Frame::new().fill(fill).stroke(egui::Stroke::new(1.0, dock::LINE)).inner_margin(8.0).show(
                    ui,
                    |ui| {
                        ui.set_width(screen.width() - 16.0);
                        egui::ScrollArea::vertical()
                            .max_height(height - 40.0)
                            .stick_to_bottom(true)
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                for line in &self.output {
                                    ui.label(RichText::new(&line.text).monospace().color(line_color(line.kind)));
                                }
                            });
                        self.line_ui(ui);
                    },
                );
            },
        );
    }

    fn line_ui(&mut self, ui: &mut egui::Ui) {
        let response = ui.add(
            egui::TextEdit::singleline(&mut self.input)
                .font(egui::TextStyle::Monospace)
                .desired_width(f32::INFINITY)
                .hint_text("Type help for commands")
                .text_color(dock::TEXT),
        );
        self.input.retain(|character| character != TOGGLE_CHARACTER);
        if std::mem::take(&mut self.focus_pending) {
            response.request_focus();
        }
        if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            let line = std::mem::take(&mut self.input);
            self.execute(&line);
            response.request_focus();
        }
    }

    fn help(&mut self) {
        let lines = [
            "Commands: help, find <text>, reset <variable>, clear, stat unit | gpu | none.",
            "Type a variable's name to see its value and help; follow it with a value to set it.",
            "Tab completes names, Up and Down recall commands, and Escape or ` closes the console.",
        ];
        for line in lines {
            self.print(LineKind::Info, line);
        }
    }

    fn find(&mut self, text: &str) {
        let lines: Vec<String> =
            self.variables.find(text).into_iter().map(|definition| self.describe(definition)).collect();
        if lines.is_empty() {
            self.print(LineKind::Error, format!("No variables match '{text}'."));
        }
        for line in lines {
            self.print(LineKind::Info, line);
        }
    }

    fn reset(&mut self, name: &str) {
        match self.variables.reset(name) {
            Ok(value) => self.print(LineKind::Info, format!("{} = {value}", self.canonical(name))),
            Err(error) => self.print(LineKind::Error, error.to_string()),
        }
    }

    /// Toggle a `stat` overlay; profiling follows whether any overlay shows.
    fn stat(&mut self, which: &str) {
        match which.to_ascii_lowercase().as_str() {
            STAT_UNIT => self.stats.unit = !self.stats.unit,
            STAT_GPU => self.stats.gpu = !self.stats.gpu,
            STAT_NONE => self.stats = StatOverlays::default(),
            _ => {
                self.print(LineKind::Error, "stat accepts unit, gpu, or none.");
                return;
            }
        }
        let profiling = self.stats.unit || self.stats.gpu;
        let _ = self.variables.set(render_variables::PROFILE, if profiling { "1" } else { "0" });
    }

    /// Print or set a variable.
    fn variable(&mut self, name: &str, value: &str) {
        let Some(definition) = self.variables.definition(name) else {
            self.print(LineKind::Error, format!("Unknown command or variable '{name}'. Type help."));
            return;
        };
        if value.is_empty() {
            let line = self.describe(definition);
            self.print(LineKind::Info, line);
            return;
        }
        match self.variables.set(name, value) {
            Ok(value) => self.print(LineKind::Info, format!("{} = {value}", self.canonical(name))),
            Err(error) => self.print(LineKind::Error, error.to_string()),
        }
    }

    /// `name = value  [accepted]  help`.
    fn describe(&self, definition: &VariableDefinition) -> String {
        let value = self.variables.get(&definition.name).map_or_else(String::new, ToString::to_string);
        format!("{} = {value}  [{}]  {}", definition.name, definition.accepted(), definition.help)
    }

    /// A variable's registered spelling.
    fn canonical(&self, name: &str) -> String {
        self.variables.definition(name).map_or_else(|| name.to_owned(), |definition| definition.name.clone())
    }

    /// Complete the line's first word from variable and command names: one match completes it, several extend it to
    /// their shared prefix and print them.
    fn complete(&mut self) {
        if self.input.contains(char::is_whitespace) {
            return;
        }
        let prefix = self.input.to_ascii_lowercase();
        let mut candidates: Vec<String> = COMMANDS
            .iter()
            .filter(|command| command.starts_with(&prefix))
            .map(|command| (*command).to_owned())
            .collect();
        candidates.extend(self.variables.complete(&prefix).into_iter().map(str::to_owned));
        match candidates.as_slice() {
            [] => {}
            [only] => self.input = format!("{only} "),
            _ => {
                self.input = shared_prefix(&candidates);
                let list = candidates.join("  ");
                self.print(LineKind::Info, list);
            }
        }
    }

    /// Step through the history: `older` toward the oldest entry, else back toward an empty line.
    fn recall(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        let newest = self.history.len() - 1;
        self.recalled = match (self.recalled, older) {
            (None, true) => Some(0),
            (Some(back), true) => Some((back + 1).min(newest)),
            (Some(0) | None, false) => None,
            (Some(back), false) => Some(back - 1),
        };
        self.input = self.recalled.map_or_else(String::new, |back| self.history[newest - back].clone());
    }

    fn remember(&mut self, line: &str) {
        self.recalled = None;
        if self.history.last().is_some_and(|last| last == line) {
            return;
        }
        if self.history.len() == MAX_HISTORY {
            self.history.remove(0);
        }
        self.history.push(line.to_owned());
    }
}

/// The drop-down's height on `screen`.
fn drop_down_height(screen: egui::Rect) -> f32 {
    (screen.height() * HEIGHT_FRACTION).clamp(MIN_HEIGHT, MAX_HEIGHT)
}

/// The longest prefix every candidate shares, ignoring case, in the first candidate's spelling.
fn shared_prefix(candidates: &[String]) -> String {
    let first = &candidates[0];
    let mut length = first.len();
    for candidate in &candidates[1..] {
        length = first
            .char_indices()
            .zip(candidate.chars())
            .take_while(|((_, a), b)| a.eq_ignore_ascii_case(b))
            .map(|((index, character), _)| index + character.len_utf8())
            .last()
            .unwrap_or(0)
            .min(length);
    }
    first[..length].to_owned()
}

fn line_color(kind: LineKind) -> Color32 {
    match kind {
        LineKind::Command => dock::ACCENT,
        LineKind::Info => dock::TEXT,
        LineKind::Error => dock::ERROR,
    }
}

#[cfg(test)]
#[path = "console.test.rs"]
mod tests;
