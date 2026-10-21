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
pub const INTRODUCTION: &str = "Write commands, change a number, see what happens. Distances use your own units, Y points up, and angles are in radians. Comments start with `#` or `//`.";

/// The field guide after the recipes: headings, paragraphs (with `code` spans), and code blocks, in order.
pub const REFERENCE: &[(ReferenceBlock, &str)] = &[
    (ReferenceBlock::Heading, "GEOMETRY YOU CAN KEEP WORKING ON"),
    (
        ReferenceBlock::Paragraph,
        "`let g = grid(columns, rows, width, depth)` creates a reusable surface. `geometry()` starts empty; `geometry(mesh)` makes a mesh editable. Bindings share geometry; `copy(g)` makes an independent copy.",
    ),
    (
        ReferenceBlock::Code,
        "addAttribute(g, \"points\", \"density\", 0)
color(g, rgba(1,1,1,1))
wrangle points in g as point {
  point.position = point.position + vector(0, 1, 0)
  point.density = noise(point.position.x, point.position.z, 7777)
  if point.density > 0 { point.color = rgba(1, 0.4, 0.2, 1) }
}
output(\"Surface\", g)
inspect(\"Intermediate\", g)",
    ),
    (
        ReferenceBlock::Paragraph,
        "Wrangle `points`, `faces`, `corners`, or `instances`. `point.index` is the current element index. Both `in` and a named `as` binding are required: `wrangle points in g as point`. Update existing scalar, XYZ, or RGBA attributes with `point.name = value`; missing reads and writes report an error. Declare fields first with `addAttribute(g, \"points\", \"density\", 0)`, or create/set point colors with `color(g, rgba(1,1,1,1))`. Declarations initialize existing elements and reject duplicate names; later topology additions receive zero defaults. Use `corner.uv = vector(u,v,0)` in `wrangle corners in g as corner` on corners for seams. Position edits clear authored normals; write normals afterward.",
    ),
    (
        ReferenceBlock::Paragraph,
        "`addPoint(g, vector(...))` and `addFace(g, a,b,c)` return indices. `removePoint(g, index)` also removes incident faces. `removeFace(g, index)` removes a face and its corners. Edit topology outside wrangles; removals compact indices.",
    ),
    (
        ReferenceBlock::Paragraph,
        "`attribute(g, \"points\", \"density\", index)` reads another element. `setAttribute(g, \"points\", \"density\", index, value)` updates an existing field outside a wrangle. Use `addAttribute` outside wrangles to create fields on points, faces, corners, or an assembly’s instances. `pointCount(g)` and `faceCount(g)` support custom loops.",
    ),
    (
        ReferenceBlock::Paragraph,
        "Define a named record with `let homeData = {kind: \"home\", settings: {capacity: 4}}`, then group its geometry with `with userData(homeData) { point(0, 0, 0) }`. Nested blocks restore the previous record on exit; local variables stay inside the block. The existing `userData(record)` command still sets data for subsequent vertices; `userData()` clears it. Records support nested objects, arrays, text, numbers, booleans, and null. Use `setUserData(g, pointIndex, record)` and `userData(g, pointIndex)` for editable geometry. The USER DATA column and JSON export retain records; OBJ, PLY, and GLB do not. Records are separate from colors and numeric attributes.",
    ),
    (
        ReferenceBlock::Paragraph,
        "`revolve(profile, segments)` turns ordered `vector(radius,height,0)` points around Y (3–96 segments). Bottom-to-top profiles face outward; radius-zero ends close at a pole. `tube(path, radius, sides)` sweeps ordered XYZ points with 3–64 sides and open ends. Both require 2–512 distinct adjacent points and create new geometry with corner UVs and smooth normals. Sharp tube bends may self-intersect. `smoothNormals(g)` returns an independent copy with area-weighted point normals after deformation.",
    ),
    (
        ReferenceBlock::Paragraph,
        "`scatter(g, count, seed[, \"density\"])` samples triangle area and an optional nonnegative scalar point weight, interpolating point attributes. `instances(source, sites[, material])` retains a source and independent placement data. Point `point.position`, `point.rotation` (XYZ radians), `point.scale` (nonzero XYZ), and `point.color` drive placement. Use `wrangle instances in assembly as point` to edit it.",
    ),
    (
        ReferenceBlock::Paragraph,
        "`output(\"Name\", value)` snapshots a result and includes it in the full build. `inspect(\"Name\", value)` snapshots it for inspection only. Choose a result above the viewport, then open ATTRIBUTES. Click an index or viewport element to select it; scalar/color fields can be visualized as markers. Export always uses the complete build.",
    ),
    (
        ReferenceBlock::Paragraph,
        "Vectors support `+ - * /` with vectors or scalars. `mix` supports vectors/colors. `length`, `normalize`, `dot`, `cross`, `smoothstep(low,high,value)`, and deterministic `noise(x,y,z)` simplify fields and transforms. Comparisons return 0 or 1. Noise is in −1…1.",
    ),
    (ReferenceBlock::Heading, "MAKE SOMETHING"),
    (
        ReferenceBlock::Code,
        "point(x, y, z)
box(x, y, z, width, height, depth)
sphere(x, y, z, radius, segments)
triangle(ax, ay, az, bx, by, bz, cx, cy, cz)
color(red, green, blue, alpha)
color(tint)
material(Steel)
material()
normal(0, 1, 0)
normal()
uv(u, v)",
    ),
    (
        ReferenceBlock::Paragraph,
        "Colors are linear: RGB may exceed one for HDR glow, and alpha (optional, default 1) and UVs are 0–1. Sphere segments are optional (default 20). Spheres generate smooth normals; boxes retain flat shading. Color applies to following commands. Add named materials in the inspector, then select them with material(Name); material() returns to Default.",
    ),
    (ReferenceBlock::Heading, "MAKE IT YOURS"),
    (
        ReferenceBlock::Code,
        "project meta(name=\"Wave study\",
  info=\"SURFACE STUDY\", title=\"MAKE WAVES.\")

param plank = meshBox(1, 0.2, 2)
param finish = materialPbr(rgba(0.9, 0.4, 0.2, 1), 0, 0.6)
material(finish)
instance(plank, vector(0, 1, 0))
let bent = deform(plank, x, y + x*x, z)
instance(bent, vector(3, 0, 0))

param height = 3 meta(min=0, max=10, step=0.1)
param offset = vector(0, 1, 0)
  meta(min=-5, max=5, step=0.1)
param tint = rgba(1, 0.4, 0.2, 1)
color(tint)
point(offset.x, offset.y, offset.z)
let angle = pi / 4
repeat 20 as i {
  point(cos(i) * 4, i * 0.2, sin(i) * 4)
}",
    ),
    (
        ReferenceBlock::Paragraph,
        "Metadata controls the saved name and preview heading. Parameters infer scalar, vector, color, mesh, or material controls from their defaults; use component names such as `.x` or `.r` in expressions. Bounds keep defaults and editor values in range. Parameters live outside loops, whose indices begin at zero.",
    ),
    (ReferenceBlock::Heading, "MAKE A LANDSCAPE"),
    (
        ReferenceBlock::Code,
        "surface(64, 64, 20, 20,
  sin(x * 0.5) * cos(z * 0.5) * 3
)",
    ),
    (
        ReferenceBlock::Paragraph,
        "Columns, rows, width, depth, height expression. Inside the height expression, `x,z` are world coordinates and `u,v` run from 0 to 1.",
    ),
    (ReferenceBlock::Heading, "A LITTLE MATH"),
    (
        ReferenceBlock::Code,
        "+ - * / % ^     pi tau e
sin cos tan abs sqrt floor ceil round
exp log min(a,b) max(a,b) pow(a,b)
atan2(y,x) clamp(x,min,max) mix(a,b,t)
seed(42)
rand() # 0–1, deterministic each run",
    ),
    (
        ReferenceBlock::Paragraph,
        "Use `floor()` for calculated repeat counts. Scripts cannot access files, network, or JavaScript.",
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
        let response = egui::Modal::new(egui::Id::new("form-lab-reference")).show(context, |ui| {
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
