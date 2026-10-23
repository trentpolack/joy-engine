// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Editor-neutral information about the FORM language for completion, hover text, highlighting, and documentation.
//!
//! [`BUILTINS`] is the single list of built-in functions: the evaluator refuses to let scripts shadow them, the
//! catalog describes them, and tests check that every entry is one the evaluator runs. [`form_document_symbols`]
//! adds the names a script declares (variables, parameters, functions, enums, modules, and attributes) that are in
//! scope at the cursor.

use crate::{
    ast::Token,
    tokenizer::{
        END_TOKEN,
        is_name,
        tokenize,
    },
};
use joy_core::scripting::{
    ScriptLanguageCatalog,
    ScriptLanguageSymbol,
    ScriptSymbolKind,
};

/// FORM language keywords in their stable catalog order.
pub const FORM_KEYWORDS: [&str; 25] = [
    "project", "param", "meta", "let", "const", "if", "else", "match", "case", "for", "foreach", "as", "wrangle", "in",
    "fn", "return", "ref", "enum", "use", "AND", "OR", "NOT", "true", "false", "null",
];

/// Attribute domains (`points`, `faces`, `corners`, `instances`), highlighted as keywords inside wrangles.
pub const FORM_ATTRIBUTE_DOMAINS: [&str; 4] = ["points", "faces", "corners", "instances"];

/// Attribute names realization reads, offered after a wrangle binding.
const STANDARD_ATTRIBUTE_NAMES: [&str; 7] = ["position", "normal", "uv", "color", "material", "rotation", "scale"];

/// One built-in function: its name, display signature, and one-line documentation.
pub struct BuiltinDescription {
    pub name: &'static str,
    pub signature: &'static str,
    pub summary: &'static str,
}

const fn builtin(name: &'static str, signature: &'static str, summary: &'static str) -> BuiltinDescription {
    BuiltinDescription {
        name,
        signature,
        summary,
    }
}

/// Every FORM built-in function, grouped by purpose.
pub const BUILTINS: &[BuiltinDescription] = &[
    // ---- Numbers.
    builtin("sin", "sin(radians)", "Sine of an angle in radians."),
    builtin("cos", "cos(radians)", "Cosine of an angle in radians."),
    builtin("tan", "tan(radians)", "Tangent of an angle in radians."),
    builtin("atan2", "atan2(y, x)", "Angle in radians of the direction (x, y)."),
    builtin("abs", "abs(value)", "Absolute value."),
    builtin("sqrt", "sqrt(value)", "Square root."),
    builtin("floor", "floor(value)", "Largest whole number at or below the value."),
    builtin("ceil", "ceil(value)", "Smallest whole number at or above the value."),
    builtin("round", "round(value)", "Nearest whole number; halves round up."),
    builtin("exp", "exp(value)", "e raised to the value."),
    builtin("log", "log(value)", "Natural logarithm."),
    builtin("pow", "pow(base, exponent)", "Base raised to the exponent (the same as base ^ exponent)."),
    builtin("min", "min(a, b)", "The smaller of two numbers."),
    builtin("max", "max(a, b)", "The larger of two numbers."),
    builtin("clamp", "clamp(value, low, high)", "The value limited to low–high."),
    builtin("mix", "mix(a, b, t)", "Linear blend from a to b by t; works on numbers, vectors, and colors."),
    builtin("smoothstep", "smoothstep(low, high, value)", "Smooth 0–1 ramp as the value moves from low to high."),
    builtin("noise", "noise(x, y, z)", "Deterministic smooth value noise in -1–1."),
    builtin("rand", "rand()", "Next number in 0–1 from the script's random stream."),
    builtin("seed", "seed(value)", "Restart the script's random stream from a whole-number seed."),
    builtin("number", "number(flag)", "1 for true and 0 for false."),
    builtin("itemCount", "itemCount(list)", "How many items a list or record holds."),
    // ---- Vectors and colors.
    builtin("vector", "vector(x, y, z)", "An XYZ vector."),
    builtin("rgba", "rgba(red, green, blue, alpha)", "A linear RGBA color; RGB may exceed 1 for glow."),
    builtin("length", "length(vector)", "Length of a vector."),
    builtin("normalize", "normalize(vector)", "The vector scaled to length 1."),
    builtin("dot", "dot(a, b)", "Dot product of two vectors."),
    builtin("cross", "cross(a, b)", "Cross product of two vectors."),
    // ---- Mesh and material resources.
    builtin("meshBox", "meshBox(width, height, depth)", "A reusable box mesh resource."),
    builtin("meshSphere", "meshSphere(radius[, segments])", "A reusable sphere mesh resource."),
    builtin(
        "transformMesh",
        "transformMesh(mesh, position, rotation, scale)",
        "A transformed copy of a mesh resource.",
    ),
    builtin("deform", "deform(mesh, x, y, z)", "A mesh whose vertices move to per-vertex x, y, and z expressions."),
    builtin("materialPbr", "materialPbr(color, metallic, roughness)", "A physically based material resource."),
    // ---- Geometry.
    builtin("geometry", "geometry([mesh])", "Empty editable geometry, or an editable copy of a mesh or geometry."),
    builtin("box", "box(width, height, depth)", "A centered box: 8 corners, flat shaded."),
    builtin("sphere", "sphere(radius[, segments])", "A centered UV sphere with smooth normals."),
    builtin("grid", "grid(columns, rows, width, depth)", "A flat XZ grid with UVs."),
    builtin("revolve", "revolve(profile, sides)", "Spin a (radius, height, 0) profile around the Y axis."),
    builtin("tube", "tube(path, radius, sides)", "Sweep a circle along a path of points."),
    builtin(
        "place",
        "place(source, position[, rotation, scale])",
        "A scaled, rotated, and moved copy of a geometry or mesh.",
    ),
    builtin("merge", "merge(a, b, …)", "New geometry combining every argument, attributes and materials included."),
    builtin("append", "append(target, source)", "Add a copy of source to target in place; returns target."),
    builtin("realize", "realize(instances)", "Geometry holding a copy of an instance collection's source per site."),
    builtin("copy", "copy(value)", "An independent copy of geometry or an instance collection."),
    builtin("translate", "translate(geometry, offset)", "Move geometry in place; returns it."),
    builtin("rotate", "rotate(geometry, radians)", "Rotate geometry in place by XYZ Euler angles; returns it."),
    builtin("scale", "scale(geometry, factors)", "Scale geometry in place per axis; returns it."),
    builtin("color", "color(geometry, rgba)", "Color every point of geometry in place; returns it."),
    builtin(
        "material",
        "material(geometry, material)",
        "Assign a material (or palette material name) to every face; returns the geometry.",
    ),
    builtin("smoothNormals", "smoothNormals(geometry)", "A copy with averaged point normals."),
    builtin(
        "addPoint",
        "addPoint(geometry, position[, attributes])",
        "Add a point, optionally setting attributes from a record; returns its index.",
    ),
    builtin("addFace", "addFace(geometry, a, b, c)", "Add a triangle between three point indices; returns its index."),
    builtin("removePoint", "removePoint(geometry, index)", "Remove a point and the faces that use it."),
    builtin("removeFace", "removeFace(geometry, index)", "Remove a face."),
    builtin("pointCount", "pointCount(geometry)", "How many points geometry has."),
    builtin("faceCount", "faceCount(geometry)", "How many faces geometry has."),
    // ---- Attributes.
    builtin(
        "addAttribute",
        "addAttribute(geometry, domain, name, default)",
        "Declare an attribute on points, faces, corners, or instances.",
    ),
    builtin("setAttribute", "setAttribute(geometry, domain, name, index, value)", "Set one element's attribute value."),
    builtin("attribute", "attribute(geometry, domain, name, index)", "Read one element's attribute value."),
    builtin("hasAttribute", "hasAttribute(geometry, domain, name)", "Whether geometry declares an attribute."),
    builtin("removeAttribute", "removeAttribute(geometry, domain, name)", "Remove a declared attribute."),
    // ---- Distribution and instances.
    builtin(
        "scatter",
        "scatter(geometry, count, seed[, weight])",
        "Points sampled over a surface, optionally weighted by an attribute.",
    ),
    builtin("instances", "instances(source, sites[, material])", "A collection placing source at every site point."),
    // ---- Results.
    builtin("output", "output(name, geometry)", "Add geometry to the result as a named, visible output."),
    builtin("inspect", "inspect(name, value)", "Record a named snapshot for inspection without drawing it."),
];

/// Whether `name` is a built-in function.
pub fn is_builtin(name: &str) -> bool {
    BUILTINS.iter().any(|builtin| builtin.name == name)
}

/// Build the FORM language catalog: keywords, then every built-in function.
pub fn form_language_catalog() -> ScriptLanguageCatalog {
    let keywords = FORM_KEYWORDS.iter().map(|name| ScriptLanguageSymbol {
        name: (*name).to_owned(),
        kind: ScriptSymbolKind::Keyword,
        detail: "FORM keyword".to_owned(),
        documentation: keyword_documentation(name).to_owned(),
        signatures: Vec::new(),
        writable: None,
    });
    let functions = BUILTINS.iter().map(|builtin| ScriptLanguageSymbol {
        name: builtin.name.to_owned(),
        kind: ScriptSymbolKind::Function,
        detail: builtin.signature.to_owned(),
        documentation: builtin.summary.to_owned(),
        signatures: vec![builtin.signature.to_owned()],
        writable: None,
    });
    ScriptLanguageCatalog {
        id: "form".to_owned(),
        symbols: keywords.chain(functions).collect(),
    }
}

/// The names `source` declares that are visible at `cursor` (a character offset), for completion and hover.
///
/// Top-level parameters, functions, enums, and modules are always visible. Variables, loop names, wrangle bindings,
/// and function parameters are visible only inside their block and after their declaration. Attribute names declared
/// with `addAttribute` are offered as properties. The scan is tolerant: unfinished source still yields the names
/// declared before the first character the tokenizer rejects.
pub fn form_document_symbols(source: &str, cursor: usize) -> Vec<ScriptLanguageSymbol> {
    let tokens = tolerant_tokens(source);
    let cursor_position = character_position(source, cursor);
    let mut scan = SymbolScan::default();
    let mut index = 0;
    while index < tokens.len() && tokens[index].text != END_TOKEN {
        let token = &tokens[index];
        let before_cursor = (token.line, token.column) < cursor_position;
        match token.text.as_str() {
            "{" if before_cursor => scan.open_block(index),
            "}" if before_cursor => scan.close_block(),
            "let" if before_cursor => scan.local(name_at(&tokens, index + 1), "variable", true),
            "const" if before_cursor => scan.local(name_at(&tokens, index + 1), "constant", false),
            "for" | "foreach" if before_cursor => {
                if let (Some(name), Some(open)) = (name_at(&tokens, index + 1), body_open(&tokens, index + 3)) {
                    let detail = if token.text == "for" { "loop index" } else { "loop item" };
                    scan.pending_block_names.push((open, name, detail, false));
                }
            }
            "wrangle" if before_cursor => {
                if let Some(name) = name_after_as(&tokens, index)
                    && let Some(open) = body_open(&tokens, index + 1)
                {
                    scan.pending_block_names.push((open, name, "wrangle element", false));
                    scan.pending_block_names.push((open, "index".to_owned(), "element index", false));
                }
            }
            "param" => scan.global(name_at(&tokens, index + 1), ScriptSymbolKind::Variable, "parameter".to_owned()),
            "fn" => {
                if let Some(name) = name_at(&tokens, index + 1) {
                    let (parameters, signature) = function_signature(&tokens, index + 2, &name);
                    scan.global(Some(name), ScriptSymbolKind::Function, signature);
                    if let Some(open) = body_open(&tokens, index + 2).filter(|_| before_cursor) {
                        for parameter in parameters {
                            scan.pending_block_names.push((open, parameter, "function parameter", true));
                        }
                    }
                }
            }
            "enum" => {
                let members = enum_members(&tokens, index + 2);
                scan.global(
                    name_at(&tokens, index + 1),
                    ScriptSymbolKind::Variable,
                    format!("enum {{ {} }}", members.join(", ")),
                );
            }
            "use" => {
                if let Some(name) = name_after_as(&tokens, index) {
                    let path = tokens.get(index + 1).map(|token| token.text.clone()).unwrap_or_default();
                    scan.global(Some(name), ScriptSymbolKind::Variable, format!("module {path}"));
                }
            }
            "addAttribute" => {
                if let Some(name) =
                    tokens.get(index + 6).and_then(|token| serde_json::from_str::<String>(&token.text).ok())
                {
                    scan.attribute(name);
                }
            }
            _ => {}
        }
        index += 1;
    }
    for name in STANDARD_ATTRIBUTE_NAMES {
        scan.attribute(name.to_owned());
    }
    scan.symbols()
}

/// Names collected while scanning, with block-scoped names on a stack of open blocks.
#[derive(Default)]
struct SymbolScan {
    globals: Vec<ScriptLanguageSymbol>,
    blocks: Vec<Vec<ScriptLanguageSymbol>>,
    top_level: Vec<ScriptLanguageSymbol>,
    attributes: Vec<String>,
    // The body token index prevents record literals in loop headers from consuming the bindings.
    pending_block_names: Vec<(usize, String, &'static str, bool)>,
}

impl SymbolScan {
    fn open_block(&mut self, index: usize) {
        let mut block = Vec::new();
        self.pending_block_names.retain(|(open, name, detail, writable)| {
            if *open == index {
                block.push(variable(name.clone(), (*detail).to_owned(), *writable));
                false
            } else {
                true
            }
        });
        self.blocks.push(block);
    }

    fn close_block(&mut self) {
        self.blocks.pop();
    }

    fn local(&mut self, name: Option<String>, detail: &str, writable: bool) {
        let Some(name) = name else {
            return;
        };
        let symbol = variable(name, detail.to_owned(), writable);
        match self.blocks.last_mut() {
            Some(block) => block.push(symbol),
            None => self.top_level.push(symbol),
        }
    }

    fn global(&mut self, name: Option<String>, kind: ScriptSymbolKind, detail: String) {
        if let Some(name) = name {
            self.globals.push(ScriptLanguageSymbol {
                documentation: detail.clone(),
                signatures: if kind == ScriptSymbolKind::Function { vec![detail.clone()] } else { Vec::new() },
                name,
                kind,
                detail,
                writable: None,
            });
        }
    }

    fn attribute(&mut self, name: String) {
        if !self.attributes.contains(&name) {
            self.attributes.push(name);
        }
    }

    /// Visible symbols, innermost first, without duplicate names.
    fn symbols(self) -> Vec<ScriptLanguageSymbol> {
        let mut result: Vec<ScriptLanguageSymbol> = Vec::new();
        let scoped = self.blocks.into_iter().rev().flatten().chain(self.top_level).chain(self.globals);
        for symbol in scoped {
            if !result.iter().any(|existing| existing.name == symbol.name) {
                result.push(symbol);
            }
        }
        for name in self.attributes {
            result.push(ScriptLanguageSymbol {
                documentation: format!("The {name} attribute, read and written as element.{name} in wrangles."),
                name,
                kind: ScriptSymbolKind::Property,
                detail: "attribute".to_owned(),
                signatures: Vec::new(),
                writable: Some(true),
            });
        }
        result
    }
}

fn variable(name: String, detail: String, writable: bool) -> ScriptLanguageSymbol {
    ScriptLanguageSymbol {
        documentation: format!("{name}: {detail}."),
        name,
        kind: ScriptSymbolKind::Variable,
        detail,
        signatures: Vec::new(),
        writable: Some(writable),
    }
}

/// Tokens of `source`, or of its longest prefix the tokenizer accepts while the author is mid-edit.
fn tolerant_tokens(source: &str) -> Vec<Token> {
    match tokenize(source) {
        Ok(tokens) => tokens,
        Err(error) => {
            let mut prefix = String::new();
            for (number, line) in source.lines().enumerate() {
                if number + 1 >= error.line {
                    break;
                }
                prefix.push_str(line);
                prefix.push('\n');
            }
            tokenize(&prefix).unwrap_or_default()
        }
    }
}

/// The one-based line and character column of character offset `cursor`.
fn character_position(source: &str, cursor: usize) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;
    for character in source.chars().take(cursor) {
        if character == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    (line, column)
}

fn name_at(tokens: &[Token], index: usize) -> Option<String> {
    tokens.get(index).filter(|token| is_name(&token.text)).map(|token| token.text.clone())
}

/// Find a statement body, skipping calls, indexing, and record/list literals in its header.
fn body_open(tokens: &[Token], start: usize) -> Option<usize> {
    let mut depth: usize = 0;
    let mut expects_value = true;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text.as_str() {
            "(" | "[" => depth += 1,
            "{" if depth == 0 && !expects_value => return Some(index),
            "{" => depth += 1,
            ";" | "}" if depth == 0 => return None,
            ")" | "]" | "}" => {
                depth = depth.saturating_sub(1);
                expects_value = false;
            }
            ".." | "+" | "-" | "*" | "/" | "%" | "^" | "==" | "!=" | "<" | ">" | "<=" | ">=" | "AND" | "OR" | "NOT"
            | "in" | "as" => expects_value = true,
            _ => expects_value = false,
        }
    }
    None
}

/// The name after the first `as` on a `wrangle` or `use` header, before its block opens.
fn name_after_as(tokens: &[Token], start: usize) -> Option<String> {
    let mut index = start + 1;
    while let Some(token) = tokens.get(index) {
        match token.text.as_str() {
            "as" => return name_at(tokens, index + 1),
            "{" | "}" | ";" => return None,
            _ => index += 1,
        }
    }
    None
}

/// A function's parameter names and display signature, reading from its opening parenthesis.
fn function_signature(tokens: &[Token], open: usize, name: &str) -> (Vec<String>, String) {
    let mut parameters = Vec::new();
    let mut spelled = Vec::new();
    if tokens.get(open).is_some_and(|token| token.text == "(") {
        let mut index = open + 1;
        let mut by_reference = false;
        while let Some(token) = tokens.get(index) {
            match token.text.as_str() {
                ")" | "{" => break,
                "ref" => by_reference = true,
                "," => {}
                text if is_name(text) => {
                    parameters.push(text.to_owned());
                    spelled.push(if by_reference { format!("ref {text}") } else { text.to_owned() });
                    by_reference = false;
                }
                _ => {}
            }
            index += 1;
        }
    }
    let signature = format!("{name}({})", spelled.join(", "));
    (parameters, signature)
}

fn enum_members(tokens: &[Token], open: usize) -> Vec<String> {
    let mut members = Vec::new();
    if tokens.get(open).is_some_and(|token| token.text == "{") {
        for token in &tokens[open + 1..] {
            if token.text == "}" {
                break;
            }
            if is_name(&token.text) {
                members.push(token.text.clone());
            }
        }
    }
    members
}

fn keyword_documentation(keyword: &str) -> &'static str {
    match keyword {
        "project" => "project meta(name=\"…\", info=\"…\") names the asset.",
        "param" => "param name = default meta(…) exposes a tunable parameter.",
        "meta" => "Metadata for a parameter or project: bounds, labels, groups, kinds, and options.",
        "let" => "let name = value declares a variable in the current block.",
        "const" => {
            "const name = value declares an immutable value; geometry and instance aliases are frozen too. Use copy() for an editable copy."
        }
        "if" | "else" => "if condition { … } else if condition { … } else { … }; conditions must be true or false.",
        "match" | "case" => "match value { case a, b { … } else { … } } runs the first case equal to the value.",
        "for" => {
            "for index in start..end { … } visits ascending integer indices, excluding end; the index is read-only."
        }
        "foreach" => {
            "foreach item in collection { … } visits list, vector, or color values in order; the item binding is read-only."
        }
        "as" => "Names a wrangle element or module.",
        "in" => "Introduces the range or collection in a loop, or the target of a wrangle.",
        "wrangle" => "wrangle points in geometry as point { … } visits every element of a domain.",
        "fn" | "return" => "fn name(a, ref b) { … return value } declares a reusable function.",
        "ref" => "Marks a function parameter (and its argument) that edits the caller's variable in place.",
        "enum" => {
            "Declares named choices, read as Kind.Tower. Use PascalCase entries on separate lines: enum Kind {\n  Tower,\n  Block\n}"
        }
        "use" => "use \"path.form\" as name loads a module's functions, enums, and constants.",
        "AND" => "True when both sides are true; the right side is skipped when the left is false.",
        "OR" => "True when either side is true; the right side is skipped when the left is true.",
        "NOT" => "Negates true or false.",
        "true" | "false" => "A true/false value.",
        "null" => "An empty record value.",
        _ => "Part of the FORM language grammar.",
    }
}

#[cfg(test)]
#[path = "language_information.test.rs"]
mod tests;
