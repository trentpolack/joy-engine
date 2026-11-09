// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The authoring shell's frame and chrome: a header (brand, Open…, Run Project, file state, Save), document tabs with
//! type glyphs, a per-document tool toolbar (tool name, Assets, tool actions, preview state, Focus viewport), the body,
//! and a status bar.
//!
//! [`ShellLayout`] owns a retained Taffy column that places those rows; rows a host hides collapse without moving the
//! others' identities. The widgets below draw the quiet, sentence-case chrome every authoring host shares, so Joy
//! Editor and the standalone tool editors read as one product. All sizes are logical points; colors and type come from
//! the shared UI theme.

use egui::{
    Align,
    Color32,
    CornerRadius,
    FontId,
    Rect,
    RichText,
    Sense,
    Stroke,
    StrokeKind,
    Ui,
    pos2,
    vec2,
};
use joy_storage::Storage;
use joy_ui::theme::{
    colors,
    layout,
    text_styles,
};
use joy_ui::ui_layout::{
    node_rect,
    taffy::prelude::*,
};

use crate::components::spaced;

/// Header height in logical points.
pub const HEADER_HEIGHT: f32 = 56.0;
/// Document tab strip height.
pub const TABS_HEIGHT: f32 = 46.0;
/// Tool toolbar height.
pub const TOOLBAR_HEIGHT: f32 = 50.0;
/// Status bar height.
pub const STATUS_HEIGHT: f32 = 30.0;

/// The brand's letter spacing (tight, like the mockups' wordmark).
const BRAND_SPACING: f32 = -0.4;
/// The brand's size.
const BRAND_SIZE: f32 = 22.0;
/// The tool badge's letter spacing.
const BADGE_SPACING: f32 = 1.6;
/// Horizontal padding inside chrome buttons.
const BUTTON_PADDING: f32 = 12.0;
/// Height of chrome buttons.
const BUTTON_HEIGHT: f32 = 34.0;
/// Width of the accent rule above the active tab.
const TAB_ACCENT_HEIGHT: f32 = 2.0;
/// Space reserved for a tab's close control.
const TAB_CLOSE_WIDTH: f32 = 28.0;

/// Rows of the shell, in logical points; hidden rows are `None`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShellRows {
    /// The header.
    pub header: Rect,
    /// The document tabs.
    pub tabs: Option<Rect>,
    /// The active document's tool toolbar.
    pub toolbar: Option<Rect>,
    /// The area beneath the toolbar, for the content browser and the document.
    pub body: Rect,
    /// The status bar.
    pub status: Rect,
}

/// Owns the shell's retained Taffy column: header, tabs, toolbar, body, and status.
#[derive(Clone, Debug)]
pub struct ShellLayout {
    tree: TaffyTree,
    root: NodeId,
    header: NodeId,
    tabs: NodeId,
    toolbar: NodeId,
    body: NodeId,
    status: NodeId,
}

impl Default for ShellLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellLayout {
    /// A column with every row visible.
    pub fn new() -> Self {
        let mut tree = TaffyTree::with_capacity(6);
        // Logical points; egui rounds to physical pixels at the display scale.
        tree.disable_rounding();
        let header = tree.new_leaf(fixed_row(HEADER_HEIGHT)).expect("header row");
        let tabs = tree.new_leaf(fixed_row(TABS_HEIGHT)).expect("tab row");
        let toolbar = tree.new_leaf(fixed_row(TOOLBAR_HEIGHT)).expect("toolbar row");
        let body = tree
            .new_leaf(Style {
                flex_grow: 1.0,
                min_size: Size {
                    width: length(0.0),
                    height: length(0.0),
                },
                ..Default::default()
            })
            .expect("body row");
        let status = tree.new_leaf(fixed_row(STATUS_HEIGHT)).expect("status row");
        let root = tree
            .new_with_children(
                Style {
                    flex_direction: FlexDirection::Column,
                    ..Default::default()
                },
                &[header, tabs, toolbar, body, status],
            )
            .expect("shell column");
        Self {
            tree,
            root,
            header,
            tabs,
            toolbar,
            body,
            status,
        }
    }

    /// Place the rows inside `area`, collapsing the tab strip and toolbar when they are hidden.
    pub fn rows(&mut self, area: Rect, show_tabs: bool, show_toolbar: bool) -> ShellRows {
        self.set_visible(self.tabs, TABS_HEIGHT, show_tabs);
        self.set_visible(self.toolbar, TOOLBAR_HEIGHT, show_toolbar);
        let root_style = Style {
            flex_direction: FlexDirection::Column,
            size: Size {
                width: length(area.width().max(0.0)),
                height: length(area.height().max(0.0)),
            },
            ..Default::default()
        };
        let computed = self
            .tree
            .set_style(self.root, root_style)
            .and_then(|()| self.tree.compute_layout(self.root, Size::MAX_CONTENT));
        if computed.is_err() {
            return fallback_rows(area);
        }
        let rect = |node| node_rect(&self.tree, self.root, node, area.min).unwrap_or(Rect::NOTHING);
        ShellRows {
            header: rect(self.header),
            tabs: show_tabs.then(|| rect(self.tabs)),
            toolbar: show_toolbar.then(|| rect(self.toolbar)),
            body: rect(self.body),
            status: rect(self.status),
        }
    }

    fn set_visible(&mut self, node: NodeId, height: f32, visible: bool) {
        let mut style = fixed_row(height);
        if !visible {
            style.display = Display::None;
        }
        // A rejected style keeps the previous one; the next frame tries again.
        let _ = self.tree.set_style(node, style);
    }
}

/// The short type glyph shown before a document's name in its tab (`F`, `FX`, `L`, `O`), from its file extension.
pub fn document_glyph(path: &str) -> &'static str {
    let extension = path.rsplit('.').next().unwrap_or_default();
    match extension {
        "form" | "forminst" => "F",
        "joyfx" => "FX",
        "joylevel" => "L",
        "joyobject" => "O",
        "joyproject" => "P",
        _ => "·",
    }
}

/// The tool name a document opens in, shown in the toolbar (`FORM`, `JOYFX`, `LEVEL`, `OBJECT`, `SOURCE`).
pub fn document_tool_name(path: &str) -> &'static str {
    let extension = path.rsplit('.').next().unwrap_or_default();
    match extension {
        "form" | "forminst" => "FORM",
        "joyfx" => "JOYFX",
        "joylevel" => "LEVEL",
        "joyobject" => "OBJECT",
        _ => "SOURCE",
    }
}

/// How a status label reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    /// Everything is current (saved, preview up to date).
    Good,
    /// Something waits on the author (unsaved edits, a stale preview).
    Pending,
    /// Something needs attention (a disk conflict, a failed build).
    Problem,
    /// Informational.
    Quiet,
}

impl StatusTone {
    fn color(self) -> Color32 {
        match self {
            Self::Good => colors::success(),
            Self::Pending => colors::accent_hover(),
            Self::Problem => colors::danger(),
            Self::Quiet => colors::muted(),
        }
    }
}

/// The product wordmark (`JOY EDITOR`) in the accent display face; the version shows on hover.
pub fn brand(ui: &mut Ui, product: &str, version: &str) -> egui::Response {
    let family = text_styles::strong_family();
    let title = spaced(&product.to_uppercase(), FontId::new(BRAND_SIZE, family), colors::accent(), BRAND_SPACING);
    ui.label(title).on_hover_text(format!("{product} {version}"))
}

/// A tool's name in the toolbar (`FORM`, `JOYFX`, `LEVEL`): small, spaced, accent capitals.
pub fn tool_badge(ui: &mut Ui, name: &str) -> egui::Response {
    let family = text_styles::strong_family();
    ui.label(spaced(name, FontId::new(layout::TYPE_SM + 1.0, family), colors::accent(), BADGE_SPACING))
}

/// A thin vertical rule between header or toolbar groups.
pub fn rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 26.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, colors::line());
}

/// A borderless text action (`Open…`, `▷ Run Project`, `Apply ⌘↵`): text only until hovered or focused.
pub fn quiet_button(ui: &mut Ui, label: &str) -> egui::Response {
    chrome_button(ui, label, ChromeStyle::Quiet, false)
}

/// An outlined chip (`Assets`, `Save ⌘S`, `Shaded ▾`, `View settings`): a subtle fill and hairline border; `active`
/// marks a toggled chip such as a visible Assets drawer.
pub fn chip_button(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    chrome_button(ui, label, ChromeStyle::Chip, active)
}

/// A quiet action that opens a menu (`Snap 1 m`), with a painted chevron after its label.
pub fn quiet_menu_button(ui: &mut Ui, label: &str) -> egui::Response {
    chrome_button_with(ui, label, ChromeStyle::Quiet, false, Trailing::Chevron)
}

/// An outlined chip that opens a menu (`+ Primitive`, `Shaded`), with a painted chevron after its label.
pub fn chip_menu_button(ui: &mut Ui, label: &str) -> egui::Response {
    chrome_button_with(ui, label, ChromeStyle::Chip, false, Trailing::Chevron)
}

/// The overflow control (three painted dots) for actions that do not need a permanent place.
pub fn overflow_button(ui: &mut Ui) -> egui::Response {
    chrome_button_with(ui, "", ChromeStyle::Quiet, false, Trailing::Dots)
}

/// Run Project: a quiet action led by a painted play triangle.
pub fn run_button(ui: &mut Ui, label: &str) -> egui::Response {
    chrome_button_with(ui, label, ChromeStyle::Quiet, false, Trailing::LeadingPlay)
}

/// A filled call to action for empty states (`+ Add Primitive`).
pub fn call_to_action(ui: &mut Ui, label: &str) -> egui::Response {
    chrome_button(ui, label, ChromeStyle::Accent, false)
}

/// A status label such as `File: Saved` or `Preview: Current`, colored by `tone`.
pub fn status_label(ui: &mut Ui, text: &str, tone: StatusTone) -> egui::Response {
    ui.label(RichText::new(text).size(layout::TYPE_MD).color(tone.color()))
}

/// A document tab's state marker, painted before its name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabMarker {
    /// Saved.
    None,
    /// Unsaved edits.
    Dirty,
    /// The file changed on disk under unsaved edits.
    Conflict,
}

/// What a document tab reported this frame.
#[derive(Debug)]
pub struct TabResponse {
    /// The tab body (click to activate).
    pub tab: egui::Response,
    /// The close control.
    pub close: egui::Response,
}

/// A document tab: the type glyph, the file name (with a dirty or conflict marker), and a close control. The active tab
/// carries an accent rule along its top and the document fill, so it joins the toolbar beneath it.
pub fn document_tab(
    ui: &mut Ui,
    id_salt: &str,
    glyph: &str,
    name: &str,
    active: bool,
    marker: TabMarker,
) -> TabResponse {
    let height = ui.available_height();
    let glyph_font = FontId::new(layout::TYPE_SM, text_styles::strong_family());
    let name_font = FontId::proportional(layout::TYPE_MD + 1.0);
    let glyph_galley = ui.painter().layout_no_wrap(glyph.to_owned(), glyph_font, colors::muted());
    let name_galley = ui.painter().layout_no_wrap(name.to_owned(), name_font, colors::text());
    let marker_width = if marker == TabMarker::None { 0.0 } else { 14.0 };
    let width =
        (BUTTON_PADDING * 2.0) + 26.0 + glyph_galley.size().x + marker_width + name_galley.size().x + TAB_CLOSE_WIDTH;
    let (rect, tab) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let painter = ui.painter();

    let fill = if active {
        colors::panel()
    } else if tab.hovered() {
        colors::surface().gamma_multiply(0.6)
    } else {
        colors::tabs()
    };
    painter.rect_filled(rect, 0.0, fill);
    painter.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(layout::BORDER_THIN, colors::line()));
    if active {
        let accent = Rect::from_min_size(rect.min, vec2(rect.width(), TAB_ACCENT_HEIGHT));
        painter.rect_filled(accent, 0.0, colors::accent());
    }
    let center_y = rect.center().y;
    let glyph_position = pos2(rect.min.x + (BUTTON_PADDING * 1.6), center_y - (glyph_galley.size().y / 2.0));
    painter.galley(glyph_position, glyph_galley.clone(), colors::muted());
    let marker_left = glyph_position.x + glyph_galley.size().x + 10.0;
    match marker {
        TabMarker::None => {}
        TabMarker::Dirty => {
            painter.circle_filled(pos2(marker_left + 4.0, center_y), 3.5, colors::accent_hover());
        }
        TabMarker::Conflict => {
            painter.circle_filled(pos2(marker_left + 4.0, center_y), 3.5, colors::danger());
        }
    }
    let name_position = pos2(marker_left + marker_width, center_y - (name_galley.size().y / 2.0));
    let name_color = if active { colors::text() } else { colors::muted() };
    painter.galley(name_position, name_galley, name_color);

    let close_rect =
        Rect::from_center_size(pos2(rect.right() - (TAB_CLOSE_WIDTH / 2.0) - 6.0, center_y), vec2(20.0, 20.0));
    let close = ui.interact(close_rect, ui.id().with(("close-tab", id_salt)), Sense::click());
    let close_color = if close.hovered() { colors::accent() } else { colors::muted() };
    let painter = ui.painter();
    let cross = close_rect.shrink(6.0);
    let stroke = Stroke::new(1.4, close_color);
    painter.line_segment([cross.left_top(), cross.right_bottom()], stroke);
    painter.line_segment([cross.right_top(), cross.left_bottom()], stroke);
    TabResponse {
        tab,
        close,
    }
}

/// Paint the shell's row backgrounds and hairlines for `rows`.
pub fn paint_chrome(ui: &Ui, rows: &ShellRows) {
    let painter = ui.painter();
    let line = Stroke::new(layout::BORDER_THIN, colors::line());
    painter.rect_filled(rows.header, 0.0, colors::header());
    painter.line_segment([rows.header.left_bottom(), rows.header.right_bottom()], line);
    if let Some(tabs) = rows.tabs {
        painter.rect_filled(tabs, 0.0, colors::tabs());
        painter.line_segment([tabs.left_bottom(), tabs.right_bottom()], line);
    }
    if let Some(toolbar) = rows.toolbar {
        painter.rect_filled(toolbar, 0.0, colors::panel());
        painter.line_segment([toolbar.left_bottom(), toolbar.right_bottom()], line);
    }
    painter.rect_filled(rows.status, 0.0, colors::header());
    painter.line_segment([rows.status.left_top(), rows.status.right_top()], line);
}

/// A status bar's text: `left` (usually `file · message`) and `right` (context), muted, in `rect`.
pub fn status_text(ui: &mut Ui, rect: Rect, left: &str, right: &str) -> egui::Response {
    let inner = rect.shrink2(vec2(layout::SPACE_LG + 4.0, 0.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(Align::Center)), |ui| {
        let response =
            ui.add(egui::Label::new(RichText::new(left).size(layout::TYPE_MD).color(colors::muted())).truncate());
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(right).size(layout::TYPE_MD).color(colors::muted()));
        });
        response
    })
    .inner
}

// ---------------------------------------------------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------------------------------------------------

/// Storage key of the editors' chosen palette (its name), shared by Joy Editor and the standalone tool editors.
pub const THEME_STORAGE_KEY: &str = "joy.editor.theme";

/// Apply the saved palette from `storage` (Joy when none is saved or storage is unavailable) and the editor look.
/// Call once, during an editor's first UI pass.
pub fn restore_theme(context: &egui::Context, storage: Option<&Storage>) {
    let saved = storage.and_then(|storage| storage.load(THEME_STORAGE_KEY).ok().flatten());
    let palette = saved.as_deref().and_then(joy_ui::theme::palette_named).unwrap_or(joy_ui::theme::palettes::JOY);
    joy_ui::theme::set_palette(context, palette);
}

/// A `Theme` menu offering every bundled palette; choosing one applies it to the whole context and saves it to
/// `storage`. Returns the menu button's response.
pub fn theme_menu(ui: &mut Ui, storage: Option<&Storage>) -> egui::Response {
    let button = quiet_menu_button(ui, "Theme").on_hover_text("Editor colors");
    egui::Popup::menu(&button).show(|ui| {
        ui.set_min_width(170.0);
        let active = joy_ui::theme::active_palette();
        for palette in joy_ui::theme::palettes::ALL {
            if ui.selectable_label(palette == active, palette.name).clicked() && palette != active {
                joy_ui::theme::set_palette(ui.ctx(), palette);
                if let Some(storage) = storage {
                    // A preference that cannot be stored still applies for this session.
                    let _ = storage.save(THEME_STORAGE_KEY, palette.name);
                }
            }
        }
    });
    button
}

// ---------------------------------------------------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------------------------------------------------

/// The look of a chrome button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChromeStyle {
    Quiet,
    Chip,
    Accent,
}

/// Painted decoration around a chrome button's label; the UI fonts carry no chevron, ellipsis, or play glyphs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trailing {
    None,
    Chevron,
    Dots,
    LeadingPlay,
}

/// Width a painted decoration takes beside the label.
const DECORATION_WIDTH: f32 = 14.0;

fn chrome_button(ui: &mut Ui, label: &str, style: ChromeStyle, active: bool) -> egui::Response {
    chrome_button_with(ui, label, style, active, Trailing::None)
}

fn chrome_button_with(
    ui: &mut Ui,
    label: &str,
    style: ChromeStyle,
    active: bool,
    trailing: Trailing,
) -> egui::Response {
    let font = match style {
        ChromeStyle::Accent => FontId::new(layout::TYPE_MD + 1.0, text_styles::strong_family()),
        _ => FontId::proportional(layout::TYPE_MD + 1.0),
    };
    let text_color = match style {
        ChromeStyle::Accent => colors::on_accent(),
        _ if !ui.is_enabled() => colors::muted().gamma_multiply(0.6),
        _ => colors::text(),
    };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, text_color);
    let decoration = match trailing {
        Trailing::None => 0.0,
        Trailing::Dots => DECORATION_WIDTH + 4.0,
        Trailing::Chevron | Trailing::LeadingPlay if label.is_empty() => DECORATION_WIDTH,
        Trailing::Chevron | Trailing::LeadingPlay => DECORATION_WIDTH + 6.0,
    };
    let size = vec2(galley.size().x + decoration + (BUTTON_PADDING * 2.0), BUTTON_HEIGHT);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hovered = ui.is_enabled() && (response.hovered() || response.has_focus());
    let painter = ui.painter();
    let radius = CornerRadius::same(layout::RADIUS_SM);
    match style {
        ChromeStyle::Quiet => {
            if hovered {
                painter.rect_filled(rect, radius, colors::surface().gamma_multiply(0.7));
            }
        }
        ChromeStyle::Chip => {
            let fill = if active || hovered { colors::surface() } else { colors::surface().gamma_multiply(0.55) };
            let border = if active { colors::border() } else { colors::line() };
            painter.rect(rect, radius, fill, Stroke::new(layout::BORDER_THIN, border), StrokeKind::Inside);
        }
        ChromeStyle::Accent => {
            let fill = if hovered { colors::accent_hover() } else { colors::accent() };
            painter.rect_filled(rect, radius, fill);
        }
    }
    if response.has_focus() {
        painter.rect_stroke(rect, radius, Stroke::new(layout::BORDER_FOCUS, colors::accent()), StrokeKind::Inside);
    }
    let content_width = galley.size().x + decoration;
    let left = rect.center().x - (content_width / 2.0);
    let label_left = if trailing == Trailing::LeadingPlay { left + decoration } else { left };
    painter.galley(pos2(label_left, rect.center().y - (galley.size().y / 2.0)), galley.clone(), text_color);
    let decoration_color = if style == ChromeStyle::Accent { text_color } else { colors::muted() };
    match trailing {
        Trailing::None => {}
        Trailing::Chevron => {
            let center = pos2(left + galley.size().x + 6.0 + (DECORATION_WIDTH / 2.0), rect.center().y + 1.0);
            let points = vec![center + vec2(-4.0, -2.0), center + vec2(4.0, -2.0), center + vec2(0.0, 3.0)];
            painter.add(egui::Shape::convex_polygon(points, decoration_color, Stroke::NONE));
        }
        Trailing::Dots => {
            let center = rect.center();
            for offset in [-5.0, 0.0, 5.0] {
                painter.circle_filled(center + vec2(offset, 0.0), 1.6, colors::text());
            }
        }
        Trailing::LeadingPlay => {
            let center = pos2(left + (DECORATION_WIDTH / 2.0) - 2.0, rect.center().y);
            let points = vec![center + vec2(-4.0, -6.0), center + vec2(6.0, 0.0), center + vec2(-4.0, 6.0)];
            painter.add(egui::Shape::closed_line(points, Stroke::new(1.4, text_color)));
        }
    }
    response
}

fn fixed_row(height: f32) -> Style {
    Style {
        flex_shrink: 0.0,
        size: Size {
            width: percent(1.0),
            height: length(height),
        },
        ..Default::default()
    }
}

/// Rows stacked by hand when Taffy rejects a computation (never expected; keeps the shell drawable).
fn fallback_rows(area: Rect) -> ShellRows {
    let header = Rect::from_min_size(area.min, vec2(area.width(), HEADER_HEIGHT));
    let status = Rect::from_min_max(pos2(area.min.x, area.max.y - STATUS_HEIGHT), area.max);
    ShellRows {
        header,
        tabs: None,
        toolbar: None,
        body: Rect::from_min_max(pos2(area.min.x, header.max.y), pos2(area.max.x, status.min.y)),
        status,
    }
}

#[cfg(test)]
#[path = "editor_shell.test.rs"]
mod tests;
