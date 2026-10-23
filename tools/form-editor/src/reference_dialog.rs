// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Script Reference dialog (UI module): the FORM SCRIPT field guide with searchable, appendable Recipes from
//! `script_guide`. Appending never replaces source or touches parameters.

use joy_engine::development::components;
use joy_engine::egui::{
    self,
    RichText,
    Ui,
};
use joy_engine::runtime::theme::{
    colors,
    layout,
};

use crate::script_guide::{
    RECIPES,
    matching_recipes,
};

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
        "`output(\"Name\", value)` adds a result to the build; `inspect(\"Name\", value)` keeps a snapshot for inspection only. Choose a result above the viewport, then open ATTRIBUTES. Export always uses every output.",
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
        "Comparisons produce `true` or `false`, and `if` requires them; compare numbers explicitly (`count > 0`). `AND`, `OR`, and `NOT` are written in capitals and skip the right side once the answer is known. `number(flag)` turns true and false into 1 and 0. `let` declares a variable in the current block; `name = value` and `+=`, `-=`, `*=`, `/=` update the nearest existing one, so loops can count. Parameters and loop bindings are read-only. `const` declares immutable values: geometry and instance aliases are frozen too. Use `copy(value)` for an editable copy.",
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

/// The dialog's state: whether it is open and the recipe search.
#[derive(Clone, Debug, Default)]
pub struct ReferenceDialog {
    open: bool,
    /// Recipe search text.
    pub search: String,
    scroll_to_recipes: bool,
}

impl ReferenceDialog {
    /// Open the guide (Script Reference, or `?`).
    pub fn open(&mut self) {
        self.open = true;
    }

    /// Open the guide at the recipes (Recipes ↗).
    pub fn open_recipes(&mut self) {
        self.open = true;
        self.scroll_to_recipes = true;
    }

    /// Whether the guide is open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Close the guide.
    pub fn close(&mut self) {
        self.open = false;
    }

    /// Draw the guide when open; returns the index of a recipe to append.
    pub fn show(&mut self, context: &egui::Context) -> Option<usize> {
        if !self.open {
            return None;
        }
        let mut appended = None;
        let screen = context.content_rect();
        let response = egui::Modal::new(egui::Id::new("form-editor-reference")).show(context, |ui| {
            ui.set_width((screen.width() - 80.0).clamp(320.0, 760.0));
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("THE FORM SCRIPT FIELD GUIDE")
                            .color(colors::ACCENT)
                            .size(layout::TYPE_XS)
                            .strong(),
                    );
                    ui.label(RichText::new("A FEW WORDS.\nINFINITE WHAT-IFS.").size(22.0).strong());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    if ui.button("Close").on_hover_text("Close language reference").clicked() {
                        self.open = false;
                    }
                });
            });
            egui::ScrollArea::vertical().max_height((screen.height() - 160.0).max(200.0)).show(ui, |ui| {
                paragraph(ui, INTRODUCTION);
                ui.add_space(layout::SPACE_MD);
                let recipes =
                    ui.label(RichText::new("FIND A STARTING POINT").color(colors::MUTED).size(layout::TYPE_XS));
                if std::mem::take(&mut self.scroll_to_recipes) {
                    recipes.scroll_to_me(Some(egui::Align::TOP));
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search recipes: surface, attributes, topology, assembly…")
                        .desired_width(f32::INFINITY),
                );
                let matches = matching_recipes(&self.search);
                if matches.is_empty() {
                    components::guidance(ui, "No matching recipes. Try geometry, field, or assembly.");
                }
                for index in matches {
                    let recipe = &RECIPES[index];
                    components::panel_frame().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(recipe.title).strong());
                        components::guidance(ui, recipe.description);
                        ui.label(RichText::new(recipe.code_for("study")).monospace().size(layout::TYPE_SM));
                        if ui.button("Append recipe ↗").on_hover_text(format!("Append {}", recipe.title)).clicked() {
                            appended = Some(index);
                        }
                    });
                }
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
            });
        });
        if response.should_close() {
            self.open = false;
        }
        if appended.is_some() {
            self.open = false;
        }
        appended
    }
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
