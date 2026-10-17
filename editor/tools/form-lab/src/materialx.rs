// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! MaterialX 1.39 exchange for FORM LAB's constant `gltf_pbr` subset.
//!
//! Export writes one `gltf_pbr` surface shader and one `surfacematerial` per palette entry. Import accepts exactly that
//! subset and rejects everything it would otherwise lose: node graphs, connections, textures, unknown inputs or
//! attributes, unreferenced shaders, significant text, and DOCTYPE or entity declarations. This module carries the
//! small XML reader the subset needs, so parsing works on native and browser targets without another dependency.

use joy_engine::core::format_js_number;
use joy_engine::form::read_materials;
use joy_engine::serde_json::{
    Value as Json,
    json,
};

/// Constant `gltf_pbr` inputs FORM materials map to, with their MaterialX types.
const INPUTS: [(&str, &str); 7] = [
    ("base_color", "color3"),
    ("metallic", "float"),
    ("roughness", "float"),
    ("alpha", "float"),
    ("alpha_mode", "integer"),
    ("alpha_cutoff", "float"),
    ("emissive", "color3"),
];

/// FORM alpha modes in `gltf_pbr` `alpha_mode` order.
const ALPHA_MODES: [&str; 3] = ["OPAQUE", "MASK", "BLEND"];

/// Why MaterialX could not be written or read. Messages match the TypeScript tool.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialXError(pub String);

impl std::fmt::Display for MaterialXError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MaterialXError {}

/// Export constant `gltf_pbr` surface materials as MaterialX 1.39 XML (`exportMaterialX`). Palettes with maps fail
/// instead of silently dropping them.
pub fn export_materialx(materials: &[Json]) -> Result<String, MaterialXError> {
    let validated = read_materials(Some(&Json::Array(materials.to_vec()))).map_err(|error| MaterialXError(error.0))?;
    let textured = validated.iter().any(|material| {
        material["textures"].as_object().is_some_and(|maps| maps.values().any(|source| !source.is_null()))
    });
    if textured {
        return Err(MaterialXError(
            "MaterialX export cannot preserve textures. Save a .formlab project or export GLB instead.".into(),
        ));
    }
    let mut lines = vec!["<?xml version=\"1.0\"?>".to_owned(), "<materialx version=\"1.39\">".to_owned()];
    let mut used_names: std::collections::HashSet<String> =
        validated.iter().filter_map(|material| material["name"].as_str().map(str::to_owned)).collect();
    for (index, material) in validated.iter().enumerate() {
        let mut shader_name = format!("FormLabShader{index}");
        while used_names.contains(&shader_name) {
            shader_name = format!("_{shader_name}");
        }
        used_names.insert(shader_name.clone());
        let numbers = |value: &Json| -> Vec<f64> {
            value.as_array().map_or_else(Vec::new, |values| values.iter().filter_map(Json::as_f64).collect())
        };
        let base_color = numbers(&material["baseColor"]);
        let alpha_mode = ALPHA_MODES.iter().position(|mode| material["alphaMode"] == *mode).unwrap_or(0);
        lines.push(format!("  <gltf_pbr name=\"{shader_name}\" type=\"surfaceshader\">"));
        lines.push(input("base_color", "color3", &base_color[..3.min(base_color.len())]));
        lines.push(input("metallic", "float", &[material["metallic"].as_f64().unwrap_or(0.0)]));
        lines.push(input("roughness", "float", &[material["roughness"].as_f64().unwrap_or(1.0)]));
        lines.push(input("alpha", "float", &[base_color.get(3).copied().unwrap_or(1.0)]));
        lines.push(input("alpha_mode", "integer", &[alpha_mode as f64]));
        lines.push(input("alpha_cutoff", "float", &[material["alphaCutoff"].as_f64().unwrap_or(0.5)]));
        lines.push(input("emissive", "color3", &numbers(&material["emissive"])));
        lines.push("  </gltf_pbr>".to_owned());
        lines.push(format!(
            "  <surfacematerial name=\"{}\" type=\"material\">",
            material["name"].as_str().unwrap_or("")
        ));
        lines.push(format!("    <input name=\"surfaceshader\" type=\"surfaceshader\" nodename=\"{shader_name}\"/>"));
        lines.push("  </surfacematerial>".to_owned());
    }
    lines.push("</materialx>".to_owned());
    Ok(format!("{}\n", lines.join("\n")))
}

/// Import FORM LAB's constant `gltf_pbr` MaterialX subset (`importMaterialX`). Returns validated material records;
/// a Default material is included only when the document authors one.
pub fn import_materialx(text: &str) -> Result<Vec<Json>, MaterialXError> {
    let upper = text.to_ascii_uppercase();
    if upper.contains("<!DOCTYPE") || upper.contains("<!ENTITY") {
        return Err(failure("MaterialX DOCTYPE and entity declarations are not supported."));
    }
    let root = xml::parse(text).map_err(|()| failure("MaterialX XML is malformed."))?;
    if root.name != "materialx" {
        return Err(failure("Expected a MaterialX document."));
    }
    assert_no_significant_text(&root)?;
    assert_attributes(&root, &["version", "xmlns"])?;
    if root.attribute("version") != Some("1.39") {
        return Err(MaterialXError(format!(
            "Unsupported MaterialX version '{}'.",
            root.attribute("version").unwrap_or("")
        )));
    }

    let mut shaders: Vec<(String, Shader)> = Vec::new();
    let mut material_elements = Vec::new();
    let mut global_names = std::collections::HashSet::new();
    for element in root.child_elements() {
        let name = require_name(element)?;
        if !global_names.insert(name.to_owned()) {
            return Err(MaterialXError(format!(
                "MaterialX node names must be globally unique; '{name}' is duplicated."
            )));
        }
        match element.name.as_str() {
            "gltf_pbr" => shaders.push((name.to_owned(), read_shader(element)?)),
            "surfacematerial" => material_elements.push(element),
            other => return Err(MaterialXError(format!("Unsupported MaterialX node '{other}'."))),
        }
    }

    let authored_default = material_elements.iter().any(|element| element.attribute("name") == Some("Default"));
    let materials =
        material_elements.iter().map(|element| read_material(element, &shaders)).collect::<Result<Vec<_>, _>>()?;
    let referenced: std::collections::HashSet<&str> = material_elements
        .iter()
        .filter_map(|element| element.child_elements().next().and_then(|connection| connection.attribute("nodename")))
        .collect();
    if let Some((name, _)) = shaders.iter().find(|(name, _)| !referenced.contains(name.as_str())) {
        return Err(MaterialXError(format!("MaterialX shader '{name}' is unreferenced and cannot be imported.")));
    }
    if materials.is_empty() {
        return Err(failure("MaterialX document contains no surface materials."));
    }
    let validated = read_materials(Some(&Json::Array(materials))).map_err(|error| MaterialXError(error.0))?;
    Ok(if authored_default {
        validated
    } else {
        validated.into_iter().filter(|material| material["name"] != "Default").collect()
    })
}

/// Constant shader values read from one `gltf_pbr` node, with the `gltf_pbr` defaults for absent inputs.
#[derive(Clone, Debug)]
struct Shader {
    base_color: Vec<f64>,
    metallic: f64,
    roughness: f64,
    alpha: f64,
    alpha_mode: f64,
    alpha_cutoff: f64,
    emissive: Vec<f64>,
}

/// An error with a fixed message.
fn failure(message: &str) -> MaterialXError {
    MaterialXError(message.to_owned())
}

/// One exported `<input>` line.
fn input(name: &str, kind: &str, values: &[f64]) -> String {
    let serialized: Vec<String> = values.iter().map(|value| format_js_number(*value)).collect();
    format!("    <input name=\"{name}\" type=\"{kind}\" value=\"{}\"/>", serialized.join(", "))
}

fn read_shader(element: &xml::Element) -> Result<Shader, MaterialXError> {
    assert_attributes(element, &["name", "type", "doc", "uiname"])?;
    if element.attribute("type") != Some("surfaceshader") {
        return Err(failure("gltf_pbr nodes must have the surfaceshader type."));
    }
    let mut values: Vec<(&str, Vec<f64>)> = Vec::new();
    for child in element.child_elements() {
        if child.name != "input" {
            return Err(MaterialXError(format!("Unsupported MaterialX shader child '{}'.", child.name)));
        }
        let name = child.attribute("name").unwrap_or("");
        let constant =
            || MaterialXError(format!("MaterialX input '{name}' must be a constant value without a connection."));
        if ["nodename", "nodegraph", "interfacename"].iter().any(|attribute| child.attribute(attribute).is_some()) {
            return Err(constant());
        }
        assert_attributes(child, &["name", "type", "value", "doc", "uiname", "uifolder"])?;
        let Some((input_name, expected)) = INPUTS.iter().find(|(input, _)| *input == name) else {
            return Err(MaterialXError(format!("Unsupported gltf_pbr input '{name}'.")));
        };
        if child.attribute("type") != Some(expected) {
            return Err(MaterialXError(format!("MaterialX input '{name}' must have type '{expected}'.")));
        }
        let value = child.attribute("value").ok_or_else(constant)?;
        if values.iter().any(|(existing, _)| existing == input_name) {
            return Err(MaterialXError(format!("Duplicate MaterialX input '{name}'.")));
        }
        values.push((input_name, parse_value(value, expected, name)?));
    }
    let value = |name: &str| values.iter().find(|(input, _)| *input == name).map(|(_, value)| value.clone());
    let scalar = |name: &str, fallback: f64| value(name).map_or(fallback, |value| value[0]);
    Ok(Shader {
        base_color: value("base_color").unwrap_or_else(|| vec![1.0, 1.0, 1.0]),
        metallic: scalar("metallic", 1.0),
        roughness: scalar("roughness", 1.0),
        alpha: scalar("alpha", 1.0),
        alpha_mode: scalar("alpha_mode", 0.0),
        alpha_cutoff: scalar("alpha_cutoff", 0.5),
        emissive: value("emissive").unwrap_or_else(|| vec![0.0, 0.0, 0.0]),
    })
}

fn read_material(element: &xml::Element, shaders: &[(String, Shader)]) -> Result<Json, MaterialXError> {
    assert_attributes(element, &["name", "type", "doc", "uiname"])?;
    if element.attribute("type") != Some("material") {
        return Err(failure("MaterialX surface materials must have type material."));
    }
    let children: Vec<&xml::Element> = element.child_elements().collect();
    if children.len() != 1 || children[0].name != "input" {
        return Err(failure("Each MaterialX surface material must contain one shader connection."));
    }
    let connection = children[0];
    assert_attributes(connection, &["name", "type", "nodename", "doc"])?;
    if connection.attribute("name") != Some("surfaceshader") || connection.attribute("type") != Some("surfaceshader") {
        return Err(failure("Unsupported MaterialX surface material input."));
    }
    let shader_name = connection.attribute("nodename");
    let Some((_, shader)) = shaders.iter().find(|(name, _)| Some(name.as_str()) == shader_name) else {
        return Err(MaterialXError(format!(
            "MaterialX material references missing shader '{}'.",
            shader_name.unwrap_or("null")
        )));
    };
    let alpha_mode = (shader.alpha_mode.fract() == 0.0 && (0.0..3.0).contains(&shader.alpha_mode))
        .then(|| ALPHA_MODES[shader.alpha_mode as usize])
        .ok_or_else(|| failure("MaterialX alpha_mode must be 0, 1, or 2."))?;
    let mut base_color = shader.base_color.clone();
    base_color.push(shader.alpha);
    Ok(json!({
        "name": element.attribute("name"),
        "baseColor": base_color,
        "metallic": shader.metallic,
        "roughness": shader.roughness,
        "emissive": shader.emissive,
        "alphaMode": alpha_mode,
        "alphaCutoff": shader.alpha_cutoff,
    }))
}

/// Parse a comma-separated constant of `kind` (`color3`, `float`, or `integer`).
fn parse_value(text: &str, kind: &str, name: &str) -> Result<Vec<f64>, MaterialXError> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let expected_length = if kind == "color3" { 3 } else { 1 };
    let invalid = || MaterialXError(format!("MaterialX input '{name}' has an invalid {kind} value."));
    if parts.len() != expected_length || !parts.iter().all(|part| is_number_syntax(part)) {
        return Err(invalid());
    }
    let components: Vec<f64> = parts.iter().map(|part| part.parse::<f64>().unwrap_or(f64::NAN)).collect();
    if components.iter().any(|value| !value.is_finite()) {
        return Err(invalid());
    }
    let integer = parts[0].strip_prefix(['+', '-']).unwrap_or(parts[0]);
    if kind == "integer" && (integer.is_empty() || !integer.bytes().all(|byte| byte.is_ascii_digit())) {
        return Err(MaterialXError(format!("MaterialX input '{name}' has an invalid integer value.")));
    }
    Ok(components)
}

/// `^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?$`
fn is_number_syntax(text: &str) -> bool {
    let bytes = text.strip_prefix(['+', '-']).unwrap_or(text).as_bytes();
    let digits = |start: usize| bytes[start..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    let mut index = digits(0);
    let integer = index;
    let mut fraction = 0;
    if bytes.get(index) == Some(&b'.') {
        fraction = digits(index + 1);
        index += 1 + fraction;
    }
    if integer == 0 && fraction == 0 {
        return false;
    }
    if matches!(bytes.get(index), Some(b'e' | b'E')) {
        index += 1;
        if matches!(bytes.get(index), Some(b'+' | b'-')) {
            index += 1;
        }
        let exponent = digits(index);
        if exponent == 0 {
            return false;
        }
        index += exponent;
    }
    index == bytes.len()
}

fn assert_attributes(element: &xml::Element, allowed: &[&str]) -> Result<(), MaterialXError> {
    match element.attributes.iter().find(|(name, _)| !allowed.contains(&name.as_str())) {
        Some((name, _)) => {
            Err(MaterialXError(format!("Unsupported attribute '{name}' on MaterialX <{}>.", element.name)))
        }
        None => Ok(()),
    }
}

fn require_name(element: &xml::Element) -> Result<&str, MaterialXError> {
    element
        .attribute("name")
        .filter(|name| !name.is_empty())
        .ok_or_else(|| MaterialXError(format!("MaterialX <{}> requires a name.", element.name)))
}

fn assert_no_significant_text(element: &xml::Element) -> Result<(), MaterialXError> {
    for node in &element.children {
        match node {
            xml::Node::Text(text) if !text.trim().is_empty() => {
                return Err(MaterialXError(format!("Unsupported text content in MaterialX <{}>.", element.name)));
            }
            xml::Node::Element(child) => assert_no_significant_text(child)?,
            xml::Node::Instruction => {
                return Err(MaterialXError(format!("Unsupported XML node in MaterialX <{}>.", element.name)));
            }
            xml::Node::Text(_) | xml::Node::Comment => {}
        }
    }
    Ok(())
}

/// The XML subset MaterialX documents use: a declaration, comments, processing instructions, elements with attributes,
/// character data, CDATA sections, and the predefined and numeric character references. Anything else (including
/// mismatched tags, duplicate attributes, and stray markup) is malformed.
mod xml {
    /// A parsed node.
    #[derive(Clone, Debug, PartialEq)]
    pub enum Node {
        /// A child element.
        Element(Element),
        /// Character data, with references resolved (CDATA sections included).
        Text(String),
        /// A comment.
        Comment,
        /// A processing instruction inside an element.
        Instruction,
    }

    /// A parsed element.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Element {
        /// The tag name, prefix included.
        pub name: String,
        /// Attributes in document order with resolved values.
        pub attributes: Vec<(String, String)>,
        /// Child nodes in document order.
        pub children: Vec<Node>,
    }

    impl Element {
        /// An attribute's value.
        pub fn attribute(&self, name: &str) -> Option<&str> {
            self.attributes.iter().find(|(attribute, _)| attribute == name).map(|(_, value)| value.as_str())
        }

        /// Child elements in document order.
        pub fn child_elements(&self) -> impl Iterator<Item = &Element> {
            self.children.iter().filter_map(|node| match node {
                Node::Element(element) => Some(element),
                _ => None,
            })
        }
    }

    /// Parse a document and return its root element.
    pub fn parse(text: &str) -> Result<Element, ()> {
        let mut reader = Reader { text: text.strip_prefix('\u{feff}').unwrap_or(text), position: 0 };
        if reader.rest().starts_with("<?xml") {
            reader.instruction()?;
        }
        reader.miscellany()?;
        if !reader.rest().starts_with('<') {
            return Err(());
        }
        let root = reader.element(0)?;
        reader.miscellany()?;
        if reader.position != reader.text.len() { Err(()) } else { Ok(root) }
    }

    /// Deepest accepted nesting; MaterialX documents are three levels deep.
    const MAXIMUM_DEPTH: usize = 64;

    struct Reader<'a> {
        text: &'a str,
        position: usize,
    }

    impl Reader<'_> {
        fn rest(&self) -> &str {
            &self.text[self.position..]
        }

        fn skip_whitespace(&mut self) -> bool {
            let rest = self.rest();
            let trimmed = rest.trim_start_matches([' ', '\t', '\r', '\n']);
            let skipped = rest.len() - trimmed.len();
            self.position += skipped;
            skipped > 0
        }

        fn expect(&mut self, token: &str) -> Result<(), ()> {
            if self.rest().starts_with(token) {
                self.position += token.len();
                Ok(())
            } else {
                Err(())
            }
        }

        /// Comments, processing instructions, and whitespace outside the root element.
        fn miscellany(&mut self) -> Result<(), ()> {
            loop {
                self.skip_whitespace();
                if self.rest().starts_with("<!--") {
                    self.comment()?;
                } else if self.rest().starts_with("<?") {
                    self.instruction()?;
                } else {
                    return Ok(());
                }
            }
        }

        fn comment(&mut self) -> Result<(), ()> {
            self.expect("<!--")?;
            let end = self.rest().find("-->").ok_or(())?;
            if self.rest()[..end].contains("--") || self.rest()[..end].ends_with('-') {
                return Err(());
            }
            self.position += end + 3;
            Ok(())
        }

        fn instruction(&mut self) -> Result<(), ()> {
            self.expect("<?")?;
            let target = self.name()?;
            if target.eq_ignore_ascii_case("xml") && self.position != 5 {
                return Err(());
            }
            let end = self.rest().find("?>").ok_or(())?;
            self.position += end + 2;
            Ok(())
        }

        fn name(&mut self) -> Result<String, ()> {
            let rest = self.rest();
            let length = rest
                .char_indices()
                .find(|(index, character)| {
                    let valid = character.is_alphanumeric() || matches!(character, '_' | ':' | '-' | '.');
                    let start = *index == 0 && (character.is_ascii_digit() || matches!(character, '-' | '.'));
                    !valid || start
                })
                .map_or(rest.len(), |(index, _)| index);
            if length == 0 {
                return Err(());
            }
            let name = rest[..length].to_owned();
            self.position += length;
            Ok(name)
        }

        fn element(&mut self, depth: usize) -> Result<Element, ()> {
            if depth > MAXIMUM_DEPTH {
                return Err(());
            }
            self.expect("<")?;
            let name = self.name()?;
            let mut attributes: Vec<(String, String)> = Vec::new();
            loop {
                let separated = self.skip_whitespace();
                if self.rest().starts_with("/>") {
                    self.position += 2;
                    return Ok(Element { name, attributes, children: Vec::new() });
                }
                if self.rest().starts_with('>') {
                    self.position += 1;
                    break;
                }
                if !separated {
                    return Err(());
                }
                let attribute = self.name()?;
                self.skip_whitespace();
                self.expect("=")?;
                self.skip_whitespace();
                let quote = self.rest().chars().next().filter(|quote| matches!(quote, '"' | '\'')).ok_or(())?;
                self.position += 1;
                let end = self.rest().find(quote).ok_or(())?;
                let raw = &self.rest()[..end];
                if raw.contains('<') {
                    return Err(());
                }
                let value = resolve_references(raw)?.replace(['\t', '\n', '\r'], " ");
                self.position += end + 1;
                if attributes.iter().any(|(existing, _)| *existing == attribute) {
                    return Err(());
                }
                attributes.push((attribute, value));
            }
            let mut children = Vec::new();
            loop {
                let rest = self.rest();
                if rest.is_empty() {
                    return Err(());
                }
                if rest.starts_with("</") {
                    self.position += 2;
                    let closing = self.name()?;
                    self.skip_whitespace();
                    self.expect(">")?;
                    if closing != name {
                        return Err(());
                    }
                    return Ok(Element { name, attributes, children });
                }
                if rest.starts_with("<!--") {
                    self.comment()?;
                    children.push(Node::Comment);
                } else if rest.starts_with("<![CDATA[") {
                    self.position += 9;
                    let end = self.rest().find("]]>").ok_or(())?;
                    children.push(Node::Text(self.rest()[..end].to_owned()));
                    self.position += end + 3;
                } else if rest.starts_with("<?") {
                    self.instruction()?;
                    children.push(Node::Instruction);
                } else if rest.starts_with('<') {
                    children.push(Node::Element(self.element(depth + 1)?));
                } else {
                    let end = rest.find('<').unwrap_or(rest.len());
                    let raw = &rest[..end];
                    if raw.contains("]]>") {
                        return Err(());
                    }
                    children.push(Node::Text(resolve_references(raw)?));
                    self.position += end;
                }
            }
        }
    }

    /// Resolve `&lt;`, `&gt;`, `&amp;`, `&quot;`, `&apos;`, and numeric character references; any other `&` fails.
    fn resolve_references(raw: &str) -> Result<String, ()> {
        let mut output = String::with_capacity(raw.len());
        let mut rest = raw;
        while let Some(start) = rest.find('&') {
            output.push_str(&rest[..start]);
            let end = rest[start..].find(';').ok_or(())? + start;
            let reference = &rest[start + 1..end];
            let character = match reference {
                "lt" => '<',
                "gt" => '>',
                "amp" => '&',
                "quot" => '"',
                "apos" => '\'',
                _ => {
                    let code = if let Some(hex) = reference.strip_prefix("#x") {
                        u32::from_str_radix(hex, 16).map_err(|_| ())?
                    } else if let Some(decimal) = reference.strip_prefix('#') {
                        decimal.parse::<u32>().map_err(|_| ())?
                    } else {
                        return Err(());
                    };
                    char::from_u32(code).filter(|character| *character != '\0').ok_or(())?
                }
            };
            output.push(character);
            rest = &rest[end + 1..];
        }
        output.push_str(rest);
        Ok(output)
    }
}
