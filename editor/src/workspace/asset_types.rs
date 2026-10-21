// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Asset formats the workspace routes to an owned editor, and the starter content it creates.

use joy_engine::form::FormInstance;
use joy_engine::gameplay::entity_type;
use joy_engine::serde_json::{
    self,
    Value,
    json,
};

/// Formats the workspace can create, import, and open, without dots.
pub const SUPPORTED_ASSET_KINDS: [&str; 5] = ["joylevel", "joyobject", "form", "forminst", "joyfx"];

/// Formats offered by **New**, in menu order, with their labels.
pub const NEW_ASSET_KINDS: [(&str, &str); 5] = [
    ("joylevel", "Level"),
    ("joyobject", "Object"),
    ("form", "FORM script (.form)"),
    ("forminst", "FORM instance (.forminst)"),
    ("joyfx", "Particle effect"),
];

/// The format **New** selects first: a FORM script, the recipe instances refer to.
pub const DEFAULT_NEW_ASSET_KIND: &str = "form";

/// Which editor owns a document. Unknown extensions never fall through to FORM.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetEditor {
    /// The level editor, for `.joylevel` levels and `.joyobject` reusable objects.
    Level,
    /// A hosted authoring tool registered for the extension (FORM EDITOR for `.form` and `.forminst`, JOYFX EDITOR for
    /// `.joyfx`).
    Tool,
    /// A plain source view, for manifest-declared formats without an editor.
    Source,
}

/// The editor for a project-relative asset path.
pub fn asset_editor(path: &str) -> AssetEditor {
    match asset_kind(path).as_deref() {
        Some("joylevel" | "joyobject") => AssetEditor::Level,
        Some("form" | "forminst" | "joyfx") => AssetEditor::Tool,
        _ => AssetEditor::Source,
    }
}

/// The lowercase extension of a path without the dot, if it has one.
pub fn asset_kind(path: &str) -> Option<String> {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rfind('.').filter(|index| *index > 0).map(|index| name[index + 1..].to_lowercase())
}

/// Whether the workspace can route `path` to one of its own editors.
pub fn is_supported_asset_path(path: &str) -> bool {
    asset_kind(path).is_some_and(|kind| SUPPORTED_ASSET_KINDS.contains(&kind.as_str()))
}

/// The project path **New** writes for an author's `name` and chosen `kind`, or a message explaining the naming rule.
///
/// Names use letters, digits, dashes, underscores, and optional folders; the extension is added when missing.
pub fn new_asset_path(name: &str, kind: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Name the new asset before creating it.".to_owned());
    }
    let suffix = format!(".{kind}");
    let file = if name.to_lowercase().ends_with(&suffix) { name.to_owned() } else { format!("{name}{suffix}") };
    let stem = &file[..file.len() - suffix.len()];
    let valid_stem = !stem.is_empty()
        && stem.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'/' | b'-'))
        && stem.split('/').all(|part| !part.is_empty());
    if !valid_stem || file.contains("..") || !is_supported_asset_path(&file) {
        return Err("Use letters, numbers, dashes, underscores and optional folders for the asset name.".to_owned());
    }
    Ok(format!("assets/{file}"))
}

/// A one-sentence explanation of what a new asset of `kind` stores.
pub fn new_asset_hint(kind: &str) -> &'static str {
    match kind {
        "form" => "Procedural recipe. Stores the script and the parameters it exposes.",
        "forminst" => {
            "Configured FORM. Stores a script (packed here; point it at a .form to share one recipe), parameter overrides, materials and variants."
        }
        "joylevel" => "Scene document. Stores entities and their authored settings.",
        "joyobject" => "Reusable object definition. Stores entity components and defaults.",
        "joyfx" => "Editable effect. Stores emitters, scripts and effect parameters.",
        _ => "",
    }
}

/// Starter content accepted by the authoring and runtime readers, named after the file.
///
/// Returns `None` for formats the workspace cannot create.
pub fn create_asset_text(path: &str) -> Option<String> {
    let kind = asset_kind(path)?;
    if !SUPPORTED_ASSET_KINDS.contains(&kind.as_str()) {
        return None;
    }
    let file = path.rsplit('/').next().unwrap_or(path);
    let name = file.rfind('.').map_or(file, |index| &file[..index]);
    let quoted = serde_json::to_string(name).unwrap_or_else(|_| "\"New asset\"".to_owned());
    let source = format!(
        "project meta(name={quoted})\n\nlet block = translate(box(1, 1, 1), vector(0, 0.5, 0))\noutput({quoted}, color(block, rgba(0.93, 0.42, 0.22, 1)))\n"
    );
    let value: Value = match kind.as_str() {
        "form" => return Some(source),
        "forminst" => FormInstance::packed(name, &source).to_json(),
        "joyfx" => json!({
            "version": 1, "name": name, "maxParticles": 128, "parameters": {}, "emitters": [{
                "id": "sparks", "enabled": true, "root": true, "duration": 2, "rate": 12,
                "bursts": [{"time": 0, "count": 8}], "lifetime": 1, "renderer": "soft",
                "spawn": "velocity.x = rand(-0.5, 0.5); velocity.y = rand(1, 2); velocity.z = rand(-0.5, 0.5); size = 0.15; color.r = 2; color.g = 0.5; color.b = 0.1;",
                "update": "color.a = 1 - t;"
            }]
        }),
        "joyobject" => json!({
            "version": 1, "name": name, "entity": {
                "id": "object", "name": name, "type": entity_type::ENTITY,
                "components": {"render": {"shape": "box", "size": [1, 1, 1], "color": [0.93, 0.42, 0.22]}}
            }
        }),
        _ => json!({"version": 1, "name": name, "entities": []}),
    };
    let mut text = serde_json::to_string_pretty(&value).ok()?;
    text.push('\n');
    Some(text)
}
