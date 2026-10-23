// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Geometry interchange exports of an accepted build: Wavefront OBJ, ASCII PLY, and generated JSON. Number formatting
//! is part of each format's contract, so exports stay byte-compatible with recorded reference output. None of these
//! formats carries the procedural recipe.

use joy_engine::core::{
    format_number,
    integral_numbers_as_integers,
};
use joy_engine::form::GeometryData;
use joy_engine::serde_json::{
    self,
    Map,
    Value as Json,
};

/// Indexed faces and independent points in Wavefront OBJ, Y-up in script units.
pub fn export_obj(data: &GeometryData) -> String {
    // The header and object name keep their FORM LAB spelling so exports match assets already exported.
    let mut lines = vec!["# FORM LAB — Y-up, script units".to_owned(), "o FormLab".to_owned()];
    for position in data.positions.chunks(3) {
        lines.push(format!("v {}", join_numbers(position)));
    }
    for triangle in data.triangles.chunks(3) {
        let corners: Vec<String> = triangle.iter().map(|index| (index + 1).to_string()).collect();
        lines.push(format!("f {}", corners.join(" ")));
    }
    for index in &data.points {
        lines.push(format!("p {}", index + 1));
    }
    lines.join("\n") + "\n"
}

/// ASCII PLY with RGBA vertices and faces; vertices no face references remain point samples.
pub fn export_ply(data: &GeometryData) -> String {
    let vertex_count = data.positions.len() / 3;
    let mut lines: Vec<String> = [
        "ply",
        "format ascii 1.0",
        "comment FORM LAB — Y-up, script units",
        &format!("element vertex {vertex_count}"),
        "property float x",
        "property float y",
        "property float z",
        "property uchar red",
        "property uchar green",
        "property uchar blue",
        "property uchar alpha",
        &format!("element face {}", data.triangles.len() / 3),
        "property list uchar int vertex_indices",
        "end_header",
    ]
    .iter()
    .map(|line| (*line).to_owned())
    .collect();
    for vertex in 0..vertex_count {
        let mut fields: Vec<String> =
            data.positions[vertex * 3..(vertex * 3) + 3].iter().map(|value| format_number(*value)).collect();
        for channel in 0..3 {
            let color = data.colors.get((vertex * 3) + channel).copied().unwrap_or(1.0);
            fields.push(format_number((color * 255.0).round()));
        }
        let alpha = data.alphas.get(vertex).copied().unwrap_or(1.0);
        fields.push(format_number((alpha * 255.0).round()));
        lines.push(fields.join(" "));
    }
    for triangle in data.triangles.chunks(3) {
        let corners: Vec<String> = triangle.iter().map(u32::to_string).collect();
        lines.push(format!("3 {}", corners.join(" ")));
    }
    lines.join("\n") + "\n"
}

/// Generated geometry and metadata for custom integrations, pretty-printed with two-space indentation. This is not an
/// editable FORM EDITOR document.
pub fn export_geometry_json(data: &GeometryData) -> Result<String, serde_json::Error> {
    let mut object = Map::new();
    object.insert("format".into(), Json::from("form-editor-geometry"));
    object.insert("version".into(), Json::from(1));
    object.insert("upAxis".into(), Json::from("Y"));
    object.insert("units".into(), Json::from("script"));
    if let Json::Object(fields) = serde_json::to_value(data)? {
        object.extend(fields);
    }
    serde_json::to_string_pretty(&integral_numbers_as_integers(Json::Object(object)))
}

/// Numbers in their shortest round-trip form, joined by spaces.
fn join_numbers(values: &[f64]) -> String {
    values.iter().map(|value| format_number(*value)).collect::<Vec<_>>().join(" ")
}
