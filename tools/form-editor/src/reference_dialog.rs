// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM SCRIPT documentation dialog (UI module), with two separate pages: the Script Reference (the language field
//! guide) and Recipes (searchable, appendable starting points from `script_guide`). Each page opens on its own and
//! links to the other. Appending a recipe never replaces source or touches parameters.

use joy_engine::development::components;
use joy_engine::egui::{
    self,
    RichText,
    Ui,
};
use joy_engine::runtime::theme::{
    colors,
    layout,
    text_styles,
};

use crate::script_guide::{
    RECIPES,
    matching_recipes,
};

/// Documentation uses most of the workspace while leaving a visible backdrop around the modal.
const DIALOG_SCREEN_FRACTION: f32 = 0.86;
const DIALOG_MAX_WIDTH: f32 = 1160.0;

/// A block of the field guide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferenceBlock {
    /// A section heading.
    Heading,
    /// Prose with `code` spans.
    Paragraph,
    /// A code sample.
    Code,
}

/// The field guide's introduction.
pub const INTRODUCTION: &str = "Build geometry values, combine them, and output what you want to see. Nothing draws until you `output` it. Distances use your own units, Y points up, and angles are in radians. Comments start with `#` or `//`.";

/// The field guide after the recipes: headings, paragraphs (with `code` spans), and code blocks, in order.
pub const REFERENCE: &[(ReferenceBlock, &str)] = &[
    (ReferenceBlock::Heading, "MAKE SOMETHING"),
    (
        ReferenceBlock::Code,
        "const stone = rgba(0.62, 0.6, 0.55, 1)
const base = color(box(4, 1, 4), stone)
const dome = translate(sphere(1.5, 24), vector(0, 1.5, 0))
const pillar = place(meshBox(0.4, 3, 0.4), vector(1.6, 2, 1.6))
output(\"Shrine\", merge(base, dome, pillar))",
    ),
    (
        ReferenceBlock::Paragraph,
        "Every constructor returns geometry: `box(width, height, depth)`, `sphere(radius[, segments])`, `grid(columns, rows, width, depth)`, `revolve`, `tube`, `geometry()`, and `place(mesh, position[, rotation, scale])`. `translate`, `rotate`, `scale`, `color(g, rgba)`, and `material(g, material)` change geometry in place and return it, so calls nest. `merge(a, b, …)` combines parts into new geometry; `append(target, part)` adds to existing geometry. Each part keeps its colors, materials, and attributes. Geometry without a color shows FORM's orange tint.",
    ),
    (
        ReferenceBlock::Paragraph,
        "`output(\"Name\", value)` adds a result to the build; `inspect(\"Name\", value)` keeps a snapshot for inspection only. Choose one under SHOW above the preview, then open ATTRIBUTES. Export always includes every output and never an inspect snapshot.",
    ),
    (ReferenceBlock::Heading, "DECIDE AND ITERATE"),
    (
        ReferenceBlock::Code,
        "let count = 0
for i in 0..12 {
  if i % 3 == 0 AND NOT (i == 6) {
    count += 1
  } else if i > 9 OR i == 1 {
    count -= 1
  } else {
    count *= 1
  }
}",
    ),
    (
        ReferenceBlock::Paragraph,
        "Comparisons produce `true` or `false`, and `if` requires them; compare numbers explicitly (`count > 0`). `AND`, `OR`, and `NOT` are written in capitals and skip the right side once the answer is known. `number(flag)` turns true and false into 1 and 0. `let` declares a variable in the current block; `name = value` and `+=`, `-=`, `*=`, `/=` update the nearest existing one, so loops can count. Parameters and loop bindings are read-only. `const` declares immutable values: a constant read from geometry or instances holds a frozen snapshot and leaves the source variable editable. Use `copy(value)` for an editable copy of a constant.",
    ),
    (
        ReferenceBlock::Code,
        "const placements = [{x: 0, height: 2}, {x: 3, height: 4}]
let towers = geometry()
foreach placement in placements {
  append(towers, place(box(1, placement.height, 1), vector(placement.x, placement.height/2, 0)))
}
output(\"Towers\", towers)",
    ),
    (
        ReferenceBlock::Paragraph,
        "`for index in start..end` visits ascending integer indices, excluding end. `foreach item in collection` visits list, vector, or color values in order. Both evaluate their input once and allow at most 150,000 iterations. Use `wrangle` to edit geometry or retained instance attributes.",
    ),
    (ReferenceBlock::Heading, "NAME YOUR CHOICES"),
    (
        ReferenceBlock::Code,
        "enum Roof {
  Flat,
  Gable,
  Dome
}
param roof = Roof.Gable
param sign = \"OPEN\"
const heights = {Flat: 0.2, Gable: 1.5, Dome: 2}
match roof {
  case Roof.Flat, Roof.Gable { const pitch = heights[roof] }
  else { const pitch = 0 }
}
const widths = [2, 4, 8]
const widest = widths[itemCount(widths) - 1]",
    ),
    (
        ReferenceBlock::Paragraph,
        "Enums name choices; `param roof = Roof.Gable` becomes a choice control. Text compares with `==`, and an enum member equals its name as text. `param style = \"Brick\" meta(options=[\"Brick\", \"Glass\"])` reads as the chosen text; `param sign = \"OPEN\"` is free text. Records `{key: value}` and lists `[a, b]` hold numbers, text, true/false, and nested records; read them with `record.key`, `record[key]` (text or enum), and `list[index]`.",
    ),
    (ReferenceBlock::Heading, "REUSE IT"),
    (
        ReferenceBlock::Code,
        "use \"parts/kit.form\" as kit

fn column(height) {
  return translate(box(0.4, height, 0.4), vector(0, height / 2, 0))
}

fn add_columns(ref target, count) {
  for i in 0..count {
    append(target, translate(column(3), vector(i * 2, 0, 0)))
  }
}

let colonnade = kit.base(10)
add_columns(ref colonnade, 5)
output(\"Colonnade\", colonnade)",
    ),
    (
        ReferenceBlock::Paragraph,
        "Functions are declared at the top level with `fn`, can appear anywhere in the script, and `return` a value. Arguments are copies, so a function never changes your geometry by accident; mark a parameter `ref` (and pass `ref name`) to edit the caller's variable in place. Functions read script-level variables and parameters but cannot assign them. `use \"path.form\" as kit` loads another script's functions, enums, and constants relative to this script's folder; modules declare only `fn`, `enum`, `const`, `let`, and `use`.",
    ),
    (ReferenceBlock::Heading, "GEOMETRY YOU CAN KEEP WORKING ON"),
    (
        ReferenceBlock::Code,
        "let g = grid(24, 24, 12, 12)
addAttribute(g, \"points\", \"density\", 0)
color(g, rgba(1,1,1,1))
wrangle points in g as point {
  point.position += vector(0, noise(point.position.x, point.position.z, 7777), 0)
  point.density = smoothstep(-0.5, 0.5, point.position.y)
  if point.density > 0.5 { point.color = rgba(1, 0.4, 0.2, 1) }
}
output(\"Surface\", g)",
    ),
    (
        ReferenceBlock::Paragraph,
        "Wrangle `points`, `faces`, `corners`, or `instances`; `point.index` is the current element. Declare fields first with `addAttribute(g, \"points\", \"density\", 0)`; attributes hold numbers, XYZ vectors, RGBA colors, text, enum members, or true/false and keep their type. `face.material = steel` assigns materials per face. Use `corner.uv = vector(u,v,0)` on corners for seams. Position edits clear authored normals; write normals afterward.",
    ),
    (
        ReferenceBlock::Paragraph,
        "`addPoint(g, position[, {kind: \"tree\", seed: 4}])` returns the new index and sets point attributes from the record. `addFace(g, a, b, c)` connects points. `removePoint` and `removeFace` compact indices. `attribute(g, domain, name, index)`, `setAttribute(g, domain, name, index, value)`, `hasAttribute`, and `removeAttribute` work outside wrangles. Isolated points with attributes are placements hosts read (for example an `object` path to spawn). Custom attributes reach the build per vertex and per triangle.",
    ),
    (
        ReferenceBlock::Paragraph,
        "`revolve(profile, segments)` turns ordered `vector(radius,height,0)` points around Y (3–96 segments). `tube(path, radius, sides)` sweeps ordered XYZ points with 3–64 sides. `smoothNormals(g)` returns a copy with averaged point normals. `scatter(g, count, seed[, \"density\"])` samples surfaces. `instances(source, sites[, material])` retains a source and placements driven by `point.position`, `point.rotation`, `point.scale`, and `point.color`; `realize(instances)` turns them into geometry.",
    ),
    (ReferenceBlock::Heading, "MAKE IT YOURS"),
    (
        ReferenceBlock::Code,
        "project meta(name=\"Wave study\",
  info=\"SURFACE STUDY\", title=\"MAKE WAVES.\")

param plank = meshBox(1, 0.2, 2)
param finish = materialPbr(rgba(0.9, 0.4, 0.2, 1), 0, 0.6)
param height = 3 meta(min=0, max=10, step=0.1)
param lit = true
param tint = rgba(1, 0.4, 0.2, 1)

const bent = deform(plank, x, y + x*x, z)
let boards = merge(place(plank, vector(0, 1, 0)), place(bent, vector(3, 0, 0)))
material(boards, finish)
if lit { color(boards, tint) }
output(\"Boards\", boards)",
    ),
    (
        ReferenceBlock::Paragraph,
        "Metadata controls the saved name and preview heading. Parameters infer number, vector, color, true/false, text, choice, mesh, or material controls from their defaults; use component names such as `.x` or `.r` in expressions. Bounds keep defaults and editor values in range. Parameters are declared at the top level. Palette materials from the inspector are assigned by name: `material(g, \"Steel\")`.",
    ),
    (ReferenceBlock::Heading, "A LITTLE MATH"),
    (
        ReferenceBlock::Code,
        "+ - * / % ^     == != < > <= >=     AND OR NOT
pi tau e
sin cos tan atan2 abs sqrt floor ceil round
exp log min(a,b) max(a,b) pow(a,b)
clamp(x,low,high) mix(a,b,t) smoothstep(low,high,x)
noise(x,y,z) # -1–1
vector(x,y,z) rgba(r,g,b,a) length normalize dot cross
seed(42)
rand() # 0–1, deterministic each run",
    ),
    (
        ReferenceBlock::Paragraph,
        "Vectors and colors support `+ - * /` with each other or with numbers. Colors are linear: RGB may exceed one for HDR glow; alpha is 0–1. Use `floor()` for calculated range bounds. Scripts cannot access the network or JavaScript; modules are read through the host.",
    ),
];

/// The budgets line at the end of the guide.
pub const LIMITS: &str = "Limits: 150,000 vertices · 180,000 triangles · 2 million operations · 2 second worker timeout. Point previews sample at most 40,000 markers; exports contain every point.";

/// A page of the documentation dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReferencePage {
    /// The FORM SCRIPT language field guide.
    ScriptReference,
    /// Searchable recipes to append to the script.
    Recipes,
}

impl ReferencePage {
    /// The page's name, as its button and caption read.
    pub fn title(self) -> &'static str {
        match self {
            Self::ScriptReference => "Script Reference",
            Self::Recipes => "Recipes",
        }
    }

    /// The other page, which the header links to.
    fn other(self) -> Self {
        match self {
            Self::ScriptReference => Self::Recipes,
            Self::Recipes => Self::ScriptReference,
        }
    }
}

/// The dialog's state: the open page (if any) and the recipe search.
#[derive(Clone, Debug, Default)]
pub struct ReferenceDialog {
    page: Option<ReferencePage>,
    /// Recipe search text.
    pub search: String,
}

impl ReferenceDialog {
    /// Open the Script Reference page (Script Reference, or `?`).
    pub fn open(&mut self) {
        self.page = Some(ReferencePage::ScriptReference);
    }

    /// Open the Recipes page.
    pub fn open_recipes(&mut self) {
        self.page = Some(ReferencePage::Recipes);
    }

    /// Whether a page is open.
    pub fn is_open(&self) -> bool {
        self.page.is_some()
    }

    /// The open page.
    pub fn page(&self) -> Option<ReferencePage> {
        self.page
    }

    /// Close the dialog.
    pub fn close(&mut self) {
        self.page = None;
    }

    /// Draw the open page; returns the index of a recipe to append.
    pub fn show(&mut self, context: &egui::Context) -> Option<usize> {
        let page = self.page?;
        let mut appended = None;
        let screen = context.content_rect();
        let size = egui::vec2(
            (screen.width() * DIALOG_SCREEN_FRACTION).min(DIALOG_MAX_WIDTH),
            screen.height() * DIALOG_SCREEN_FRACTION,
        );
        let response = egui::Modal::new(egui::Id::new("form-editor-reference")).show(context, |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(page.title().to_uppercase())
                            .color(colors::ACCENT)
                            .size(layout::TYPE_LG)
                            .text_style(egui::TextStyle::Button),
                    );
                    let heading = match page {
                        ReferencePage::ScriptReference => "A FEW WORDS.\nINFINITE WHAT-IFS.",
                        ReferencePage::Recipes => "FIND A\nSTARTING POINT.",
                    };
                    ui.label(RichText::new(heading).text_style(text_styles::display()));
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.add(components::button("Close")).on_hover_text("Close (Escape)").clicked() {
                        self.page = None;
                    }
                    let other = page.other();
                    let link = ui.add(components::button(other.title()));
                    if link.on_hover_text(format!("Open {}", other.title())).clicked() {
                        self.page = Some(other);
                    }
                });
            });
            ui.add_space(layout::SPACE_SM);
            // Each page keeps its own scroll position.
            egui::ScrollArea::vertical()
                .id_salt(page.title())
                .auto_shrink([false, false])
                .max_height(ui.available_height())
                .show(ui, |ui| match page {
                    ReferencePage::ScriptReference => script_reference_page(ui),
                    ReferencePage::Recipes => appended = recipes_page(ui, &mut self.search),
                });
        });
        if response.should_close() || appended.is_some() {
            self.page = None;
        }
        appended
    }
}

/// The Script Reference page: the introduction, the field guide, and the limits.
fn script_reference_page(ui: &mut Ui) {
    paragraph(ui, INTRODUCTION);
    ui.add_space(layout::SPACE_MD);
    for (kind, text) in REFERENCE {
        match kind {
            ReferenceBlock::Heading => {
                ui.add_space(layout::SPACE_SM);
                ui.label(RichText::new(*text).color(colors::ACCENT).strong());
            }
            ReferenceBlock::Paragraph => paragraph(ui, text),
            ReferenceBlock::Code => {
                egui::Frame::new().fill(colors::SCRIPT).inner_margin(6).show(ui, |ui| {
                    ui.label(RichText::new(*text).monospace().size(layout::TYPE_SM));
                });
            }
        }
    }
    ui.add_space(layout::SPACE_SM);
    components::guidance(ui, LIMITS);
}

/// The Recipes page: a search field and a card per matching recipe. Returns a recipe to append.
fn recipes_page(ui: &mut Ui, search: &mut String) -> Option<usize> {
    let mut appended = None;
    components::guidance(ui, "Append a recipe to the end of your script; nothing you wrote is replaced.");
    ui.add(
        egui::TextEdit::singleline(search)
            .hint_text("Search recipes: surface, attributes, topology, assembly…")
            .desired_width(f32::INFINITY)
            .margin(egui::vec2(layout::SPACE_SM, layout::SPACE_SM)),
    );
    let matches = matching_recipes(search);
    if matches.is_empty() {
        components::guidance(ui, "No matching recipes. Try geometry, field, or assembly.");
    }
    for index in matches {
        let recipe = &RECIPES[index];
        components::panel_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(recipe.title).text_style(egui::TextStyle::Button).size(layout::TYPE_LG));
            components::guidance(ui, recipe.description);
            ui.label(RichText::new(recipe.code_for("study")).monospace().size(layout::TYPE_SM));
            if ui
                .add(components::primary_button("Append recipe"))
                .on_hover_text(format!("Append {}", recipe.title))
                .clicked()
            {
                appended = Some(index);
            }
        });
    }
    appended
}

/// Prose with `code` spans in the script font.
fn paragraph(ui: &mut Ui, text: &str) {
    let mut job = egui::text::LayoutJob::default();
    let body = egui::TextFormat {
        color: colors::TEXT,
        font_id: egui::FontId::proportional(layout::TYPE_MD),
        ..Default::default()
    };
    let code = egui::TextFormat {
        color: colors::SCRIPT_FUNCTION,
        font_id: egui::FontId::monospace(layout::TYPE_SM),
        background: colors::SCRIPT,
        ..Default::default()
    };
    for (index, part) in text.split('`').enumerate() {
        job.append(part, 0.0, if index % 2 == 1 { code.clone() } else { body.clone() });
    }
    job.wrap.max_width = ui.available_width();
    ui.label(job);
}
