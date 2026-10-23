// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Appendable Recipes: runnable starting points that use unique bindings and output names when appended to a study.
//! The recipe cards and search live in the Script Reference dialog's UI module.

/// One recipe: a title, a description, and code whose `$id` placeholder becomes a unique binding name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recipe {
    /// Card title.
    pub title: &'static str,
    /// What the recipe makes.
    pub description: &'static str,
    /// FORM source with `$id` placeholders.
    pub code: &'static str,
}

impl Recipe {
    /// The code with `$id` replaced by `name`.
    pub fn code_for(&self, name: &str) -> String {
        self.code.replace("$id", name)
    }

    /// Lowercase text searched by the recipe filter: title, description, and code.
    pub fn search_text(&self) -> String {
        format!("{} {} {}", self.title, self.description, self.code).to_lowercase()
    }
}

/// The recipes, in card order.
pub const RECIPES: [Recipe; 8] = [
    Recipe {
        title: "Assemble colored parts",
        description: "Name each part, color it from a small palette, and output the merged result. Functions keep \
                      repeated parts readable.",
        code: "const $id_wall = rgba(0.82,0.74,0.6,1)
const $id_roof = rgba(0.36,0.2,0.14,1)
fn $id_post(x, z) {
  return translate(box(0.3, 2, 0.3), vector(x, 1, z))
}
let $id_posts = merge($id_post(-1.5,-1.5), $id_post(1.5,-1.5), $id_post(-1.5,1.5), $id_post(1.5,1.5))
let $id_cap = translate(box(4, 0.4, 4), vector(0, 2.2, 0))
output(\"$id\", merge(color($id_posts, $id_wall), color($id_cap, $id_roof)))",
    },
    Recipe {
        title: "Place objects with attributes",
        description: "Isolated points with attributes are placements: hosts spawn the named object at each point. \
                      Logic picks which object goes where.",
        code: "enum $id_Kind {
  Tree,
  Rock
}
let $id = geometry()
seed(7)
for i in 0..16 {
  const angle = i/16*tau
  let kind = $id_Kind.Tree
  if rand() > 0.7 { kind = $id_Kind.Rock }
  const size = 0.8 + rand()*0.4
  addPoint($id, vector(cos(angle)*8, 0, sin(angle)*8), {kind: kind, scale: vector(size, size, size)})
}
output(\"$id\", $id)",
    },
    Recipe {
        title: "Revolve a profile",
        description: "Make a pot, tower, or turned prop from radius/height points. The profile can be inspected and \
                      edited like any geometry.",
        code: "let $id_profile = geometry()
addPoint($id_profile, vector(0,0,0))
addPoint($id_profile, vector(2,0,0))
addPoint($id_profile, vector(2.5,2,0))
addPoint($id_profile, vector(1,4,0))
const $id = revolve($id_profile, 32)
output(\"$id\", $id)",
    },
    Recipe {
        title: "Sweep a path",
        description: "Generate a cable or pipe around ordered XYZ points. Returns ordinary geometry with smooth normals \
                      and corner UVs.",
        code: "let $id_path = geometry()
for i in 0..41 {
  const t = i/40*tau
  addPoint($id_path, vector(cos(t)*3, i*0.12, sin(t)*3))
}
const $id = tube($id_path, 0.3, 12)
output(\"$id\", $id)",
    },
    Recipe {
        title: "Surface + attribute field",
        description: "A reusable grid, noise displacement, and a custom density attribute. Try density in the \
                      viewport’s attribute view.",
        code: "let $id = grid(24, 24, 12, 12)
addAttribute($id, \"points\", \"density\", 0)
color($id, rgba(1,1,1,1))
wrangle points in $id as element {
  const h = noise(element.position.x*0.3, element.position.z*0.3, 7777)
  element.position = element.position + vector(0, h*3, 0)
  element.density = smoothstep(-0.5, 0.5, h)
  element.color = mix(rgba(0.12,0.28,0.31,1), rgba(0.5,0.7,0.5,1), element.density)
}
output(\"$id\", $id)",
    },
    Recipe {
        title: "Build topology from scratch",
        description: "Create empty geometry, add points, and connect a face. Face winding controls the surface normal.",
        code: "let $id = geometry()
const $id_a = addPoint($id, vector(-2,0,-2))
const $id_b = addPoint($id, vector(2,0,-2))
const $id_c = addPoint($id, vector(0,0,2))
addFace($id, $id_a, $id_c, $id_b)
addAttribute($id, \"faces\", \"region\", 0)
wrangle faces in $id as element { element.region = element.index }
output(\"$id\", $id)",
    },
    Recipe {
        title: "Assembly from generated points",
        description: "A ring of logical instances. Change the source mesh, placement formula, or scale attributes \
                      independently.",
        code: "let $id = geometry()
for i in 0..24 {
  const angle = i/24*tau
  addPoint($id, vector(cos(angle)*6, 0, sin(angle)*6))
}
addAttribute($id, \"points\", \"rotation\", vector(0,0,0))
addAttribute($id, \"points\", \"scale\", vector(1,1,1))
wrangle points in $id as element {
  element.rotation = vector(0, -element.index/24*tau, 0)
  element.scale = vector(1, 1 + sin(element.index*0.7)*0.5, 1)
}
const $id_assembly = instances(meshBox(0.5,2,0.5), $id,
  materialPbr(rgba(0.93,0.42,0.22,1), 0.2, 0.5))
output(\"$id\", $id_assembly)",
    },
    Recipe {
        title: "Shape a primitive",
        description: "Start from a sphere, deform it with a field, and flatten its base. A small building block for \
                      procedural rock and terrain studies.",
        code: "let $id = sphere(2, 16)
scale($id, vector(1.2, 0.8, 1))
rotate($id, vector(0, 0.25, 0))
color($id, rgba(1,1,1,1))
wrangle points in $id as element {
  const n = noise(element.position.x*0.8, element.position.y*0.8, element.position.z*0.8 + 7777)
  element.position = element.position*(1 + n*0.3)
  element.position = vector(element.position.x, max(-1.3, element.position.y), element.position.z)
  element.color = mix(rgba(0.16,0.22,0.28,1), rgba(0.5,0.56,0.62,1), (n + 1)/2)
}
output(\"$id\", $id)",
    },
];

/// The result of appending a recipe: the new source and the byte offset where the cursor goes (the start of the
/// appended block).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppendedRecipe {
    /// The script with the recipe appended.
    pub source: String,
    /// Cursor byte offset at the recipe's heading comment.
    pub cursor: usize,
}

/// Append a complete recipe without replacing source or touching parameters. Bindings use the first unused
/// `recipe_<n>` name, so appending the same recipe twice stays valid.
pub fn append_recipe(source: &str, recipe: &Recipe) -> AppendedRecipe {
    let mut serial = 1;
    while source.contains(&format!("recipe_{serial}")) {
        serial += 1;
    }
    let addition = format!("\n\n# {}\n{}\n", recipe.title.to_uppercase(), recipe.code_for(&format!("recipe_{serial}")));
    AppendedRecipe {
        source: format!("{source}{addition}"),
        cursor: source.len() + 2,
    }
}

/// Indexes of the recipes whose title, description, or code contains `query` (trimmed, ignoring case).
pub fn matching_recipes(query: &str) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    RECIPES
        .iter()
        .enumerate()
        .filter(|(_, recipe)| recipe.search_text().contains(&query))
        .map(|(index, _)| index)
        .collect()
}
