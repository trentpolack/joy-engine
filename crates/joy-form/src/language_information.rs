// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Editor-neutral information about the FORM language's built-in symbols for completion, hover text, highlighting,
//! and documentation.

use joy_core::scripting::{
    ScriptLanguageCatalog,
    ScriptLanguageSymbol,
    ScriptSymbolKind,
};

/// FORM language keywords in TypeScript `FORM_KEYWORD` order (`userData` is also a command and is listed there).
pub const FORM_KEYWORDS: [&str; 11] =
    ["project", "userData", "wrangle", "in", "as", "if", "let", "repeat", "with", "param", "meta"];

/// FORM built-in commands in TypeScript `FORM_COMMAND` order.
pub const FORM_COMMANDS: [&str; 49] = [
    "grid",
    "noise",
    "smoothstep",
    "point",
    "box",
    "sphere",
    "triangle",
    "surface",
    "color",
    "userData",
    "setUserData",
    "seed",
    "vector",
    "rgba",
    "uv",
    "material",
    "normal",
    "meshBox",
    "meshSphere",
    "materialPbr",
    "transformMesh",
    "deform",
    "revolve",
    "tube",
    "smoothNormals",
    "instance",
    "instances",
    "geometry",
    "copy",
    "translate",
    "rotate",
    "scale",
    "addPoint",
    "addFace",
    "removePoint",
    "removeFace",
    "length",
    "normalize",
    "dot",
    "cross",
    "mix",
    "pointCount",
    "faceCount",
    "attribute",
    "addAttribute",
    "setAttribute",
    "scatter",
    "output",
    "inspect",
];

/// Attribute domains (`points`, `faces`, `corners`, `instances`), highlighted as keywords inside wrangles.
pub const FORM_ATTRIBUTE_DOMAINS: [&str; 4] = ["points", "faces", "corners", "instances"];

/// Display signatures for commands whose arguments the catalog spells out; others show `name(…)`.
const FORM_FUNCTION_SIGNATURES: [(&str, &str); 12] = [
    ("vector", "vector(x, y, z)"),
    ("rgba", "rgba(r, g, b, alpha)"),
    ("noise", "noise(value[, seed])"),
    ("smoothstep", "smoothstep(edge0, edge1, value)"),
    ("meshBox", "meshBox(width, height, depth)"),
    ("meshSphere", "meshSphere(radius[, segments])"),
    ("materialPbr", "materialPbr(color, metallic, roughness)"),
    ("grid", "grid(columns, rows, width, depth)"),
    ("box", "box(width, height, depth)"),
    ("sphere", "sphere(radius[, segments])"),
    ("output", "output(name, geometry)"),
    ("inspect", "inspect(name, value)"),
];

/// Build the FORM language catalog: keywords that are not also commands, then every command as a function; the
/// TypeScript `FORM_LANGUAGE_CATALOG`.
pub fn form_language_catalog() -> ScriptLanguageCatalog {
    let keywords = FORM_KEYWORDS.iter().filter(|name| !FORM_COMMANDS.contains(name)).map(|name| ScriptLanguageSymbol {
        name: (*name).to_owned(),
        kind: ScriptSymbolKind::Keyword,
        detail: "FORM keyword".to_owned(),
        documentation: format!("The {name} keyword is part of the FORM language grammar."),
        signatures: Vec::new(),
        writable: None,
    });
    let commands = FORM_COMMANDS.iter().map(|name| {
        let signatures = match FORM_FUNCTION_SIGNATURES.iter().find(|(command, _)| command == name) {
            Some((_, signature)) => vec![(*signature).to_owned()],
            None => vec![format!("{name}(…)")],
        };
        ScriptLanguageSymbol {
            name: (*name).to_owned(),
            kind: ScriptSymbolKind::Function,
            detail: signatures.join(" | "),
            documentation: format!("The {name} built-in is evaluated by the bounded FORM runtime."),
            signatures,
            writable: None,
        }
    });
    ScriptLanguageCatalog { id: "form".to_owned(), symbols: keywords.chain(commands).collect() }
}

#[cfg(test)]
#[path = "language_information.test.rs"]
mod tests;
