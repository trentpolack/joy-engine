// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The shared script editor on egui's `TextEdit`: catalog-driven
//! syntax highlighting, line numbers with the active line marked, completion while typing and on Ctrl+Space with call
//! insertion, hover documentation for keywords, functions, fields, and inputs, diagnostics underlined and marked in
//! the gutter with go-to-line, Ctrl/Cmd+Enter to run, and a read-only mode with language tools off for JSON.
//!
//! The editor borrows the script text (`&mut String`) each frame and owns only presentation state (completion,
//! diagnostics, pending navigation). Script undo is the text edit's own history, separate from property history;
//! [`ScriptEditor::reset_history`] starts a new history when the owner switches documents, so programmatic refreshes
//! never become undo entries.

use std::sync::Arc;

use egui::text::{
    ByteIndex,
    CCursor,
    CCursorRange,
    CharIndex,
    LayoutJob,
    TextFormat,
};
use egui::{
    Align,
    Align2,
    Color32,
    CornerRadius,
    FontId,
    Galley,
    Id,
    Key,
    Modifiers,
    Order,
    Pos2,
    Rect,
    Sense,
    Shape,
    Stroke,
    TextBuffer,
    TextEdit,
    Ui,
    pos2,
    vec2,
};

use joy_core::scripting::{
    ScriptLanguageCatalog,
    ScriptSymbolKind,
};
use joy_ui::theme::tokens;

use crate::script_language_tools::{
    self,
    CompletionItem,
    DiagnosticSeverity,
    ScriptDiagnostic,
    ScriptSyntax,
    TokenKind,
};

/// Script font size in points.
const SCRIPT_FONT_SIZE: f32 = 13.0;

/// Text inserted for Tab (CodeMirror's `indentWithTab` with a two-space unit).
const INDENT: &str = "  ";

/// Most completion rows shown before scrolling.
const COMPLETION_ROWS: usize = 8;

/// Minimum gutter width (the TypeScript editor's 36 px).
const MINIMUM_GUTTER_WIDTH: f32 = 36.0;

/// What the editor did this frame.
#[derive(Clone, Debug)]
pub struct ScriptEditorResponse {
    /// The text edit's response.
    pub response: egui::Response,
    /// Whether the author changed the text.
    pub changed: bool,
    /// Ctrl/Cmd+Enter was pressed (the TypeScript `onRun`).
    pub run_requested: bool,
    /// One-based cursor line.
    pub line: usize,
    /// One-based cursor column, in characters.
    pub column: usize,
    /// Number of lines.
    pub line_count: usize,
}

impl ScriptEditorResponse {
    /// The TypeScript status text: `Ln 3, Col 7 · 12 lines`.
    pub fn status_text(&self) -> String {
        format!(
            "Ln {}, Col {} · {} {}",
            self.line,
            self.column,
            self.line_count,
            if self.line_count == 1 { "line" } else { "lines" }
        )
    }
}

/// An open completion list.
#[derive(Clone, Debug)]
struct CompletionState {
    /// Character index where the completed word starts.
    start: usize,
    /// Matching items, best first (indices into the editor's items).
    matches: Vec<usize>,
    /// Highlighted row.
    selected: usize,
}

/// A code editor for one script.
#[derive(Clone, Debug)]
pub struct ScriptEditor {
    id: Id,
    syntax: ScriptSyntax,
    catalog: Option<ScriptLanguageCatalog>,
    items: Vec<CompletionItem>,
    read_only: bool,
    background: Color32,
    diagnostics: Vec<ScriptDiagnostic>,
    completion: Option<CompletionState>,
    pending_line: Option<usize>,
    reset_requested: bool,
    focus_requested: bool,
}

impl ScriptEditor {
    /// An editor identified by `id_salt` (unique per editor in the context) highlighting with `syntax`.
    pub fn new(id_salt: impl egui::AsId, syntax: ScriptSyntax) -> Self {
        Self {
            id: Id::new(id_salt),
            syntax,
            catalog: None,
            items: Vec::new(),
            read_only: false,
            background: tokens::SCRIPT,
            diagnostics: Vec::new(),
            completion: None,
            pending_line: None,
            reset_requested: false,
            focus_requested: false,
        }
    }

    /// Enable completion and hover documentation from `catalog` (FORM or JOYFX).
    pub fn with_catalog(mut self, catalog: ScriptLanguageCatalog) -> Self {
        self.set_catalog(Some(catalog));
        self
    }

    /// Read-only presentation: selectable text with highlighting, no language tools, on the surface fill (JSON views).
    pub fn read_only(mut self) -> Self {
        self.read_only = true;
        self.background = tokens::SURFACE;
        self
    }

    /// Replace the language catalog (`None` turns completion and hover off).
    pub fn set_catalog(&mut self, catalog: Option<ScriptLanguageCatalog>) {
        self.items = catalog.as_ref().map(script_language_tools::completion_items).unwrap_or_default();
        self.catalog = catalog;
        self.completion = None;
    }

    /// Replace the highlighting rules, for example when an effect's parameter names change.
    pub fn set_syntax(&mut self, syntax: ScriptSyntax) {
        self.syntax = syntax;
    }

    /// The editor's id; the text edit inside is `id().with("text")`.
    pub fn id(&self) -> Id {
        self.id
    }

    /// Show these diagnostics (replacing earlier ones); an empty list clears them.
    pub fn set_diagnostics(&mut self, diagnostics: Vec<ScriptDiagnostic>) {
        self.diagnostics = diagnostics;
    }

    /// The diagnostics shown.
    pub fn diagnostics(&self) -> &[ScriptDiagnostic] {
        &self.diagnostics
    }

    /// Move the cursor to the start of one-based `line`, focus the editor, and scroll the line into view next frame.
    pub fn go_to_line(&mut self, line: usize) {
        self.pending_line = Some(line.max(1));
    }

    /// Focus the editor next frame.
    pub fn request_focus(&mut self) {
        self.focus_requested = true;
    }

    /// Start a new undo history with the cursor at the start, for a document or emitter switch (the TypeScript
    /// `setState` with fresh state).
    pub fn reset_history(&mut self) {
        self.reset_requested = true;
        self.completion = None;
    }

    /// Whether the completion list is open.
    pub fn completion_open(&self) -> bool {
        self.completion.as_ref().is_some_and(|completion| !completion.matches.is_empty())
    }

    /// Labels of the completion list as shown, best first.
    pub fn completion_labels(&self) -> Vec<&str> {
        self.completion
            .as_ref()
            .map(|completion| completion.matches.iter().map(|index| self.items[*index].label.as_str()).collect())
            .unwrap_or_default()
    }

    /// Draw a read-only view of `text` (no editing, completion, or hover tools).
    pub fn show_read_only(&mut self, ui: &mut Ui, text: &str) -> ScriptEditorResponse {
        let mut view = text;
        self.draw(ui, &mut view, true)
    }

    /// Draw the editor for `text` and apply the author's edits.
    pub fn show(&mut self, ui: &mut Ui, text: &mut String) -> ScriptEditorResponse {
        let read_only = self.read_only;
        self.draw(ui, text, read_only)
    }

    fn draw(&mut self, ui: &mut Ui, text: &mut dyn TextBuffer, read_only: bool) -> ScriptEditorResponse {
        let context = ui.ctx().clone();
        let text_id = self.id.with("text");
        let tools = self.catalog.is_some() && !read_only;
        let mut state = egui::text_edit::TextEditState::load(&context, text_id).unwrap_or_default();
        let mut state_changed = false;
        let mut scroll_to_cursor = false;

        if std::mem::take(&mut self.reset_requested) {
            state.clear_undoer();
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(0))));
            state_changed = true;
        }
        if let Some(line) = self.pending_line.take() {
            let index = script_language_tools::line_start(text.as_str(), line);
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(index))));
            state_changed = true;
            scroll_to_cursor = true;
            self.focus_requested = true;
        }
        if std::mem::take(&mut self.focus_requested) {
            context.memory_mut(|memory| memory.request_focus(text_id));
        }
        let focused = context.memory(|memory| memory.has_focus(text_id));
        let cursor_before = state.cursor.char_range().map_or(0, |range| range.primary.index.0);

        // Keys the editor handles before the text edit sees them.
        let mut run_requested = false;
        let mut explicit_completion = false;
        let mut accept: Option<usize> = None;
        if focused {
            let completion_open = self.completion_open();
            ui.input_mut(|input| {
                if !read_only && input.consume_key(Modifiers::COMMAND, Key::Enter) {
                    run_requested = true;
                }
                if tools && input.consume_key(Modifiers::CTRL, Key::Space) {
                    explicit_completion = true;
                    input.events.retain(|event| !matches!(event, egui::Event::Text(text) if text == " "));
                }
                if completion_open && let Some(completion) = &mut self.completion {
                    let count = completion.matches.len();
                    if input.consume_key(Modifiers::NONE, Key::ArrowDown) {
                        completion.selected = (completion.selected + 1) % count;
                    }
                    if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                        completion.selected = (completion.selected + count - 1) % count;
                    }
                    if input.consume_key(Modifiers::NONE, Key::Enter) || input.consume_key(Modifiers::NONE, Key::Tab) {
                        accept = Some(completion.matches[completion.selected]);
                    }
                    if input.consume_key(Modifiers::NONE, Key::Escape) {
                        self.completion = None;
                    }
                }
                if !read_only && input.consume_key(Modifiers::NONE, Key::Tab) {
                    input.events.push(egui::Event::Text(INDENT.to_owned()));
                }
            });
        }

        // Accept a completion before drawing so the text edit shows the result.
        let mut accepted = false;
        if let (Some(item), Some(completion)) = (accept, self.completion.take())
            && text.is_mutable()
        {
            let cursor = insert_completion(text, completion.start, cursor_before, &self.items[item]);
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(cursor))));
            state_changed = true;
            accepted = true;
        }
        if state_changed {
            state.clone().store(&context, text_id);
        }

        // Layout.
        let line_count = script_language_tools::line_count(text.as_str());
        let font = FontId::monospace(SCRIPT_FONT_SIZE);
        let row_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
        let digit_width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
        let digits = line_count.to_string().len().max(2) as f32;
        let gutter_width = ((digits * digit_width) + 18.0).max(MINIMUM_GUTTER_WIDTH);
        let syntax = &self.syntax;
        let mut layouter = |ui: &Ui, buffer: &dyn TextBuffer, _wrap_width: f32| -> Arc<Galley> {
            ui.painter().layout_job(highlight_job(syntax, buffer.as_str(), &font))
        };

        let frame = egui::Frame::new().fill(self.background).stroke(Stroke::new(tokens::BORDER_THIN, tokens::BORDER));
        let scroll_id = self.id.with("scroll");
        let mut drawn = None;
        frame.show(ui, |ui| {
            let visible_rows = ((ui.available_height() / row_height).floor() as usize).max(4);
            egui::ScrollArea::both().id_salt(scroll_id).auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(gutter_width);
                    let active_line = ui.painter().add(Shape::Noop);
                    let output = TextEdit::multiline(text)
                        .id(text_id)
                        .font(FontId::monospace(SCRIPT_FONT_SIZE))
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .margin(vec2(tokens::SPACE_SM, tokens::SPACE_XS))
                        .desired_width(f32::INFINITY)
                        .desired_rows(visible_rows)
                        .layouter(&mut layouter)
                        .show(ui);
                    if scroll_to_cursor && let Some(range) = output.cursor_range {
                        let rect = output.galley.pos_from_cursor(range.primary).translate(output.galley_pos.to_vec2());
                        ui.scroll_to_rect(rect.expand(row_height * 2.0), Some(Align::Center));
                    }
                    drawn = Some((output, active_line));
                });
            });
        });
        let (output, active_line) = drawn.expect("the scroll area always draws its contents");
        let galley = output.galley.clone();
        let galley_pos = output.galley_pos;
        let response = output.response.response.clone();
        let cursor = output.cursor_range.map_or(cursor_before, |range| range.primary.index.0);
        let (line, column) = script_language_tools::line_and_column(text.as_str(), cursor);
        let has_focus = response.has_focus();

        // Active line, gutter, and diagnostics.
        let clip = response.rect.union(Rect::from_min_size(
            response.rect.min - vec2(gutter_width, 0.0),
            vec2(gutter_width, response.rect.height()),
        ));
        let painter = ui.painter_at(ui.clip_rect().intersect(clip.expand(1.0)));
        let rows = logical_rows(&galley);
        if let Some((_, row_rect)) = rows.get(line.saturating_sub(1)) {
            let rect = Rect::from_min_max(
                pos2(response.rect.left(), galley_pos.y + row_rect.top()),
                pos2(response.rect.right().max(galley_pos.x + galley.size().x), galley_pos.y + row_rect.bottom()),
            );
            painter.set(active_line, Shape::rect_filled(rect, CornerRadius::ZERO, tokens::SURFACE.gamma_multiply(0.4)));
        }
        let gutter_right = galley_pos.x - tokens::SPACE_SM - 4.0;
        for (index, (_, row_rect)) in rows.iter().enumerate() {
            let number = index + 1;
            let top = galley_pos.y + row_rect.top();
            let row = Rect::from_min_max(
                pos2(gutter_right - gutter_width + 8.0, top),
                pos2(gutter_right + 4.0, top + row_rect.height()),
            );
            if number == line {
                painter.rect_filled(row, CornerRadius::ZERO, tokens::SURFACE);
            }
            let color = if number == line { tokens::ACCENT } else { tokens::SCRIPT_GUTTER };
            painter.text(
                pos2(gutter_right, top + (row_rect.height() * 0.5)),
                Align2::RIGHT_CENTER,
                number.to_string(),
                FontId::monospace(SCRIPT_FONT_SIZE - 1.0),
                color,
            );
        }
        let hovered_diagnostic = self.paint_diagnostics(
            ui,
            &painter,
            text.as_str(),
            &galley,
            galley_pos,
            gutter_right - gutter_width + 10.0,
            &rows,
        );

        // Completion: open while typing a word or on request, follow the word, close when it ends.
        let changed = response.changed() || accepted;
        if tools {
            self.update_completion(text.as_str(), cursor, changed && !accepted, explicit_completion, has_focus);
        } else {
            self.completion = None;
        }
        let mut clicked_item = None;
        if has_focus && let Some(completion) = &self.completion {
            let anchor = galley.pos_from_cursor(CCursor::new(completion.start)).translate(galley_pos.to_vec2());
            clicked_item = self.show_completion(ui, completion, pos2(anchor.left(), anchor.bottom() + 2.0));
        }
        let mut changed = changed;
        if let Some(item) = clicked_item
            && let Some(completion) = self.completion.take()
            && text.is_mutable()
        {
            let next = insert_completion(text, completion.start, cursor, &self.items[item]);
            let mut state = egui::text_edit::TextEditState::load(&context, text_id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(next))));
            state.store(&context, text_id);
            context.memory_mut(|memory| memory.request_focus(text_id));
            changed = true;
        }

        // Hover documentation (diagnostics take precedence).
        let mut response = response;
        if let Some(message) = hovered_diagnostic {
            response = response.on_hover_text_at_pointer(message);
        } else if let (Some(catalog), false) = (&self.catalog, read_only)
            && let Some(pointer) = response.hover_pos()
        {
            let index = galley.cursor_from_pos(pointer - galley_pos).index.0;
            let symbol = script_language_tools::word_at(text.as_str(), index)
                .filter(|(range, _)| {
                    // The pointer must be over the word, not past the end of a short line.
                    let end = galley.pos_from_cursor(CCursor::new(range.end)).translate(galley_pos.to_vec2());
                    pointer.x <= end.right() + 1.0
                })
                .and_then(|(_, word)| catalog.symbol(&word).cloned());
            if let Some(symbol) = symbol {
                response = response.on_hover_ui_at_pointer(|ui| {
                    ui.set_max_width(420.0);
                    ui.label(
                        egui::RichText::new(script_language_tools::symbol_heading(&symbol))
                            .monospace()
                            .color(tokens::SCRIPT_FUNCTION),
                    );
                    ui.label(egui::RichText::new(kind_label(symbol.kind)).color(tokens::MUTED).size(tokens::TYPE_XS));
                    ui.label(&symbol.documentation);
                });
            }
        }

        let line_count = script_language_tools::line_count(text.as_str());
        ScriptEditorResponse { response, changed: changed && !read_only, run_requested, line, column, line_count }
    }

    /// Update or close the completion list after this frame's edit.
    fn update_completion(&mut self, text: &str, cursor: usize, typed: bool, explicit: bool, focused: bool) {
        if !focused {
            self.completion = None;
            return;
        }
        let (start, word) = script_language_tools::word_before(text, cursor);
        let typed_word = typed
            && !word.is_empty()
            && text
                .chars()
                .nth(cursor.saturating_sub(1))
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_');
        let open = explicit
            || typed_word
            || self.completion.as_ref().is_some_and(|completion| completion.start == start && !word.is_empty());
        if !open {
            self.completion = None;
            return;
        }
        let matches = script_language_tools::filter_completions(&self.items, &word);
        if matches.is_empty() {
            self.completion = None;
            return;
        }
        let selected = self
            .completion
            .as_ref()
            .filter(|completion| completion.start == start)
            .map_or(0, |completion| completion.selected.min(matches.len() - 1));
        self.completion = Some(CompletionState { start, matches, selected });
    }

    /// Draw the completion list at `position`; returns a clicked item.
    fn show_completion(&self, ui: &Ui, completion: &CompletionState, position: Pos2) -> Option<usize> {
        let mut clicked = None;
        egui::Area::new(self.id.with("completion")).order(Order::Foreground).fixed_pos(position).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(tokens::PANEL)
                .stroke(Stroke::new(tokens::BORDER_THIN, tokens::BORDER))
                .show(ui, |ui| {
                    ui.set_min_width(260.0);
                    let row_height = 20.0;
                    egui::ScrollArea::vertical().max_height(row_height * COMPLETION_ROWS as f32).show(ui, |ui| {
                        for (row, index) in completion.matches.iter().enumerate() {
                            let item = &self.items[*index];
                            let selected = row == completion.selected;
                            let (rect, response) = ui
                                .allocate_exact_size(vec2(ui.available_width().max(260.0), row_height), Sense::click());
                            if selected {
                                ui.painter().rect_filled(rect, CornerRadius::ZERO, tokens::SELECTION);
                                response.scroll_to_me(None);
                            }
                            let painter = ui.painter_at(rect);
                            painter.text(
                                rect.left_center() + vec2(6.0, 0.0),
                                Align2::LEFT_CENTER,
                                kind_badge(item.kind),
                                FontId::monospace(10.0),
                                kind_color(item.kind),
                            );
                            painter.text(
                                rect.left_center() + vec2(24.0, 0.0),
                                Align2::LEFT_CENTER,
                                &item.label,
                                FontId::monospace(12.0),
                                tokens::TEXT,
                            );
                            painter.text(
                                rect.right_center() - vec2(6.0, 0.0),
                                Align2::RIGHT_CENTER,
                                &item.detail,
                                FontId::proportional(tokens::TYPE_XS),
                                tokens::MUTED,
                            );
                            if response.clicked() {
                                clicked = Some(*index);
                            }
                        }
                    });
                    if let Some(item) = completion.matches.get(completion.selected).map(|index| &self.items[*index]) {
                        ui.separator();
                        ui.set_max_width(320.0);
                        ui.label(egui::RichText::new(&item.documentation).color(tokens::MUTED).size(tokens::TYPE_SM));
                    }
                });
        });
        clicked
    }

    /// Underline diagnostics, mark their lines in the gutter, and return the message under the pointer.
    #[allow(clippy::too_many_arguments)]
    fn paint_diagnostics(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        text: &str,
        galley: &Galley,
        galley_pos: Pos2,
        marker_x: f32,
        rows: &[(usize, Rect)],
    ) -> Option<String> {
        let pointer = ui.input(|input| input.pointer.hover_pos());
        let mut hovered = None;
        for diagnostic in &self.diagnostics {
            let color = match diagnostic.severity {
                DiagnosticSeverity::Error => tokens::DANGER,
                DiagnosticSeverity::Warning => tokens::ACCENT_HOVER,
            };
            let range = diagnostic.character_range(text);
            let start = galley.pos_from_cursor(CCursor::new(range.start)).translate(galley_pos.to_vec2());
            let mut end = galley.pos_from_cursor(CCursor::new(range.end)).translate(galley_pos.to_vec2());
            if (end.top() - start.top()).abs() > 1.0 || end.left() <= start.left() {
                end = start.translate(vec2(6.0, 0.0));
            }
            let underline = Rect::from_min_max(
                pos2(start.left(), start.bottom() - 3.0),
                pos2(end.left().max(start.left() + 4.0), start.bottom()),
            );
            paint_squiggle(painter, underline, color);
            if let Some((_, row)) = rows.get(diagnostic.line.saturating_sub(1)) {
                let top = galley_pos.y + row.top();
                let marker =
                    Rect::from_center_size(pos2(marker_x, top + (row.height() * 0.5)), vec2(4.0, row.height() - 4.0));
                painter.rect_filled(marker, CornerRadius::ZERO, color);
                let hover =
                    Rect::from_min_max(pos2(marker.left() - 6.0, top), pos2(marker.right() + 6.0, top + row.height()));
                let target = Rect::from_min_max(start.min, pos2(end.left().max(start.left() + 6.0), start.bottom()));
                if pointer.is_some_and(|pointer| hover.contains(pointer) || target.contains(pointer)) {
                    hovered =
                        Some(format!("Line {}, column {}: {}", diagnostic.line, diagnostic.column, diagnostic.message));
                }
            }
        }
        hovered
    }
}

/// Replace characters `start..end` of `text` with `item`'s insertion (through the buffer's own edit operations),
/// returning the cursor's new character index.
fn insert_completion(text: &mut dyn TextBuffer, start: usize, end: usize, item: &CompletionItem) -> usize {
    text.delete_char_range(CharIndex(start)..CharIndex(end.max(start)));
    text.insert_text(&item.insert, CharIndex(start));
    start + item.cursor
}

/// Each logical line's first character index and its row rectangle (relative to the galley).
fn logical_rows(galley: &Galley) -> Vec<(usize, Rect)> {
    let mut rows = Vec::new();
    let mut index = 0;
    let mut starts_line = true;
    for placed in &galley.rows {
        if starts_line {
            rows.push((index, placed.rect()));
        }
        index += placed.char_count_including_newline().0;
        starts_line = placed.ends_with_newline;
    }
    if rows.is_empty() {
        rows.push((0, Rect::from_min_size(Pos2::ZERO, vec2(0.0, galley.size().y))));
    }
    rows
}

/// A zigzag underline across `rect`.
fn paint_squiggle(painter: &egui::Painter, rect: Rect, color: Color32) {
    let step = 3.0;
    let mut points = Vec::new();
    let mut x = rect.left();
    let mut up = false;
    while x <= rect.right() {
        points.push(pos2(x, if up { rect.top() } else { rect.bottom() }));
        x += step;
        up = !up;
    }
    if points.len() < 2 {
        points.push(pos2(rect.right(), rect.bottom()));
    }
    painter.add(Shape::line(points, Stroke::new(1.2, color)));
}

/// Build the highlighted layout job for `source`.
fn highlight_job(syntax: &ScriptSyntax, source: &str, font: &FontId) -> LayoutJob {
    let mut job = LayoutJob { text: source.to_owned(), ..Default::default() };
    job.wrap.max_width = f32::INFINITY;
    for token in syntax.tokenize(source) {
        job.sections.push(egui::text::LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(token.range.start)..ByteIndex(token.range.end),
            format: TextFormat { font_id: font.clone(), color: token_color(token.kind), ..Default::default() },
        });
    }
    if job.sections.is_empty() {
        job.sections.push(egui::text::LayoutSection {
            leading_space: 0.0,
            byte_range: ByteIndex(0)..ByteIndex(0),
            format: TextFormat { font_id: font.clone(), color: tokens::TEXT, ..Default::default() },
        });
    }
    job
}

/// The TypeScript editor's highlight colors.
pub fn token_color(kind: TokenKind) -> Color32 {
    match kind {
        TokenKind::Comment => tokens::SCRIPT_COMMENT,
        TokenKind::String | TokenKind::Function => tokens::SCRIPT_FUNCTION,
        TokenKind::Number => tokens::SCRIPT_NUMBER,
        TokenKind::Keyword | TokenKind::Boolean => tokens::SCRIPT_KEYWORD,
        TokenKind::Parameter => tokens::SCRIPT_PARAMETER,
        TokenKind::Operator => tokens::SCRIPT_OPERATOR,
        TokenKind::Invalid => tokens::DANGER,
        TokenKind::Whitespace | TokenKind::Property | TokenKind::Variable => tokens::TEXT,
    }
}

fn kind_badge(kind: ScriptSymbolKind) -> &'static str {
    match kind {
        ScriptSymbolKind::Keyword => "K",
        ScriptSymbolKind::Function => "ƒ",
        ScriptSymbolKind::Variable => "V",
        ScriptSymbolKind::Property => "P",
    }
}

fn kind_label(kind: ScriptSymbolKind) -> &'static str {
    match kind {
        ScriptSymbolKind::Keyword => "keyword",
        ScriptSymbolKind::Function => "function",
        ScriptSymbolKind::Variable => "read-only input",
        ScriptSymbolKind::Property => "writable field",
    }
}

fn kind_color(kind: ScriptSymbolKind) -> Color32 {
    match kind {
        ScriptSymbolKind::Keyword => tokens::SCRIPT_KEYWORD,
        ScriptSymbolKind::Function => tokens::SCRIPT_FUNCTION,
        ScriptSymbolKind::Variable => tokens::SCRIPT_PARAMETER,
        ScriptSymbolKind::Property => tokens::SCRIPT_NUMBER,
    }
}

#[cfg(test)]
#[path = "script_editor.test.rs"]
mod tests;
