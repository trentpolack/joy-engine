// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Build-only UI asset compiler. Values enter Rust through validated JSON, never hand-maintained copies.

use serde_json::{
    Map,
    Value,
};

/// Font names registered by the runtime's bundled font catalogue. Theme selections must resolve before shipping.
const BUNDLED_FONTS: &[&str] =
    &["Archivo", "Archivo SemiBold", "Archivo Medium", "Archivo Bold", "Archivo Black", "JetBrains Mono"];

/// Generate constants from the asset documents, reporting invalid data with its file and field path. `palettes` are
/// the additional editor palettes as `(file name, document)`, each with `name`, `dark`, and an `editor` object holding
/// exactly the default theme's editor keys.
pub fn generate(theme: &str, layout: &str, palettes: &[(&str, &str)]) -> Result<String, String> {
    let theme: Value = serde_json::from_str(theme).map_err(|error| format!("theme-default.json: {error}"))?;
    let layout: Value = serde_json::from_str(layout).map_err(|error| format!("layout.json: {error}"))?;
    let theme = object(&theme, "theme-default.json", &["game", "editor", "fonts", "opacity"])?;
    let layout = object(&layout, "layout.json", &["type", "space", "radius", "border", "size", "game_panel"])?;
    let mut source =
        String::from("// Generated from assets/ui/theme-default.json and layout.json; edit those assets.\n");

    colors(&mut source, &theme["game"], "theme-default.json.game")?;
    editor_palettes(&mut source, &theme["editor"], palettes)?;
    source.push_str("mod font_faces {\n");
    for (key, value) in
        object(&theme["fonts"], "theme-default.json.fonts", &["ui", "strong", "display", "code", "game_strong"])?
    {
        let name = identifier(key, "theme-default.json.fonts")?;
        let font = value.as_str().filter(|font| BUNDLED_FONTS.contains(font)).ok_or_else(|| {
            format!("theme-default.json.fonts.{key}: expected a bundled font name ({})", BUNDLED_FONTS.join(", "))
        })?;
        source.push_str(&format!("pub const {name}: &str = {font:?};\n"));
    }
    source.push_str("}\nmod opacity {\n");
    for (key, value) in object(&theme["opacity"], "theme-default.json.opacity", &["game_panel"])? {
        number(&mut source, key, value, "theme-default.json.opacity", "", "f32", 1.0)?;
    }
    source.push_str(
        "}\n/// Shared geometry and type sizes from `assets/ui/layout.json`, in logical pixels.\npub mod layout {\n",
    );
    for (group, prefix, kind, maximum) in [
        ("type", "TYPE_", "f32", f32::MAX),
        ("space", "SPACE_", "f32", i8::MAX as f32),
        ("radius", "RADIUS_", "u8", u8::MAX as f32),
        ("border", "BORDER_", "f32", f32::MAX),
        ("size", "", "f32", f32::MAX),
    ] {
        let path = format!("layout.json.{group}");
        for (key, value) in object(&layout[group], &path, &[])? {
            number(&mut source, key, value, &path, prefix, kind, maximum)?;
        }
    }
    source.push_str("/// In-game HUD panel geometry, in logical pixels.\npub mod game_panel {\n");
    let panel = object(&layout["game_panel"], "layout.json.game_panel", &["margin", "radius", "border"])?;
    for (key, kind, maximum) in
        [("margin", "i8", i8::MAX as f32), ("radius", "u8", u8::MAX as f32), ("border", "f32", f32::MAX)]
    {
        number(&mut source, key, &panel[key], "layout.json.game_panel", "", kind, maximum)?;
    }
    source.push_str("}\n}\n");
    Ok(source)
}

fn object<'a>(value: &'a Value, path: &str, fields: &[&str]) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .filter(|object| !object.is_empty())
        .ok_or_else(|| format!("{path}: expected a nonempty object"))?;
    if !fields.is_empty() {
        for field in fields {
            if !object.contains_key(*field) {
                return Err(format!("{path}.{field}: missing required field"));
            }
        }
        for key in object.keys() {
            if !fields.contains(&key.as_str()) {
                return Err(format!("{path}.{key}: unknown field"));
            }
        }
    }
    Ok(object)
}

fn identifier(key: &str, path: &str) -> Result<String, String> {
    if !key.starts_with(|character: char| character.is_ascii_alphabetic())
        || !key.bytes().all(|character| character.is_ascii_alphanumeric() || character == b'_')
    {
        return Err(format!("{path}.{key}: expected a name containing letters, digits, and underscores"));
    }
    Ok(key.to_ascii_uppercase())
}

fn colors(source: &mut String, value: &Value, path: &str) -> Result<(), String> {
    for (key, value) in object(value, path, &[])? {
        let name = identifier(key, path)?;
        let hex = value
            .as_str()
            .and_then(|value| value.strip_prefix('#'))
            .filter(|hex| hex.len() == 6 && hex.bytes().all(|digit| digit.is_ascii_hexdigit()))
            .ok_or_else(|| format!("{path}.{key}: expected an RGB color in #RRGGBB form"))?;
        source.push_str(&format!(
            "/// `{path}.{key}`.\npub const {name}: egui::Color32 = egui::Color32::from_rgb(0x{}, 0x{}, 0x{});\n",
            &hex[..2],
            &hex[2..4],
            &hex[4..]
        ));
    }
    Ok(())
}

/// The editor `Palette` record, the Joy palette (the default theme's `editor` colors) and every additional palette as
/// constants in `palettes`, and `colors`: one function per color returning the active palette's value.
fn editor_palettes(source: &mut String, default: &Value, extra: &[(&str, &str)]) -> Result<(), String> {
    let keys: Vec<String> = object(default, "theme-default.json.editor", &[])?.keys().cloned().collect();
    let fields: Vec<&str> = keys.iter().map(String::as_str).collect();
    source.push_str(
        "/// An editor color palette: every editor, lab, syntax, and development overlay color. Select one with \
         [`set_palette`].\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct Palette {\n    /// Display name.\n    \
         pub name: &'static str,\n    /// Whether the palette is dark (selects egui's dark or light base visuals).\n    \
         pub dark: bool,\n",
    );
    for key in &keys {
        identifier(key, "theme-default.json.editor")?;
        source.push_str(&format!("    /// `editor.{key}`.\n    pub {key}: egui::Color32,\n"));
    }
    source.push_str("}\n/// The bundled editor palettes.\npub mod palettes {\n    use super::Palette;\n");
    let mut names = Vec::new();
    let mut constant = |source: &mut String, name: &str, dark: bool, colors: &Map<String, Value>, path: &str| {
        let identifier = name.to_ascii_uppercase().replace(' ', "_");
        source.push_str(&format!(
            "    /// The {name} palette.\n    pub const {identifier}: Palette = Palette {{\n        name: {name:?},\n        dark: {dark},\n"
        ));
        for key in &keys {
            let color = hex_color(&colors[key], &format!("{path}.{key}"))?;
            source.push_str(&format!("        {key}: {color},\n"));
        }
        source.push_str("    };\n");
        names.push(identifier);
        Ok::<(), String>(())
    };
    constant(source, "Joy", true, object(default, "theme-default.json.editor", &[])?, "theme-default.json.editor")?;
    for (file, text) in extra {
        let document: Value = serde_json::from_str(text).map_err(|error| format!("{file}: {error}"))?;
        let document = object(&document, file, &["name", "dark", "editor"])?;
        let name = document["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .ok_or_else(|| format!("{file}.name: expected text"))?;
        let dark = document["dark"].as_bool().ok_or_else(|| format!("{file}.dark: expected a boolean"))?;
        let path = format!("{file}.editor");
        let colors = object(&document["editor"], &path, &fields)?;
        constant(source, name, dark, colors, &path)?;
    }
    source.push_str(&format!(
        "    /// Every palette, the default first.\n    pub const ALL: [Palette; {}] = [{}];\n}}\n",
        names.len(),
        names.join(", ")
    ));
    source
        .push_str("/// The active palette's editor, lab, syntax, and development overlay colors.\npub mod colors {\n");
    for key in &keys {
        source.push_str(&format!(
            "    /// The active palette's `editor.{key}`.\n    pub fn {key}() -> egui::Color32 {{\n        super::active_palette().{key}\n    }}\n"
        ));
    }
    source.push_str("}\n");
    Ok(())
}

/// An `egui::Color32::from_rgb` expression for a `#RRGGBB` string.
fn hex_color(value: &Value, path: &str) -> Result<String, String> {
    let hex = value
        .as_str()
        .and_then(|value| value.strip_prefix('#'))
        .filter(|hex| hex.len() == 6 && hex.bytes().all(|digit| digit.is_ascii_hexdigit()))
        .ok_or_else(|| format!("{path}: expected an RGB color in #RRGGBB form"))?;
    Ok(format!("egui::Color32::from_rgb(0x{}, 0x{}, 0x{})", &hex[..2], &hex[2..4], &hex[4..]))
}

fn number(
    source: &mut String,
    key: &str,
    value: &Value,
    path: &str,
    prefix: &str,
    kind: &str,
    maximum: f32,
) -> Result<(), String> {
    let name = format!("{prefix}{}", identifier(key, path)?);
    let value = value
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0 && *value <= f64::from(maximum))
        .ok_or_else(|| format!("{path}.{key}: expected a finite number between 0 and {maximum}"))?;
    let literal = if kind == "f32" {
        format!("{:?}", value as f32)
    } else {
        if value.fract() != 0.0 {
            return Err(format!("{path}.{key}: expected a whole number"));
        }
        // JSON permits -0.0; a bounded integer cast normalizes it before emitting an unsigned Rust literal.
        format!("{}", value as u32)
    };
    source.push_str(&format!("/// `{path}.{key}`.\npub const {name}: {kind} = {literal};\n"));
    Ok(())
}

#[cfg(test)]
#[path = "theme_build.test.rs"]
mod tests;
