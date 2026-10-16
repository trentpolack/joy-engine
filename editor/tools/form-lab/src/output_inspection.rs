// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Result selection and the bounded, read-only attribute table of the Output inspector: which named output the preview
//! shows, its attribute domains and columns, element locations for picking, paging, the selection, and attribute
//! visualization colors. The UI-free half of `output-inspector.ts`; the script owns every edit.

use joy_engine::core::format_js_number;
use joy_engine::form::GeometryData;
use joy_engine::serde_json::Value as Json;

use crate::rendering::viewport::Viewport;

/// Rows per attribute page.
pub const PAGE_SIZE: usize = 48;

/// An attribute domain.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Domain {
    /// Vertices.
    #[default]
    Points,
    /// Faces (located at their centroids).
    Faces,
    /// Face corners.
    Corners,
    /// Retained instance placements (point attributes of the sites).
    Instances,
}

impl Domain {
    /// The serialized and displayed name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::Faces => "faces",
            Self::Corners => "corners",
            Self::Instances => "instances",
        }
    }

    /// The attribute set the domain reads (instances read point attributes of their sites).
    fn attribute_key(self) -> &'static str {
        match self {
            Self::Instances | Self::Points => "points",
            Self::Faces => "faces",
            Self::Corners => "corners",
        }
    }
}

/// One attribute column: name, components per element, and flat values.
#[derive(Clone, Debug, PartialEq)]
pub struct AttributeColumn {
    /// Attribute name.
    pub name: String,
    /// Components per element (1, 3, or 4).
    pub size: usize,
    /// `size` values per element.
    pub values: Vec<f64>,
}

impl AttributeColumn {
    /// One element's components.
    pub fn element(&self, index: usize) -> &[f64] {
        self.values.get(index * self.size..(index + 1) * self.size).unwrap_or(&[])
    }
}

/// The inspected geometry: faces, attribute sets by domain, and user data records.
#[derive(Clone, Debug, Default, PartialEq)]
struct InspectedGeometry {
    faces: Vec<Vec<usize>>,
    points: Vec<AttributeColumn>,
    faces_attributes: Vec<AttributeColumn>,
    corners: Vec<AttributeColumn>,
    user_data: Option<Vec<Json>>,
}

impl InspectedGeometry {
    fn attributes(&self, domain: Domain) -> &[AttributeColumn] {
        match domain.attribute_key() {
            "points" => &self.points,
            "faces" => &self.faces_attributes,
            _ => &self.corners,
        }
    }

    fn positions(&self) -> &[f64] {
        self.points.iter().find(|column| column.name == "position").map_or(&[], |column| column.values.as_slice())
    }
}

/// Output inspector state for one view.
#[derive(Clone, Debug, Default)]
pub struct OutputInspection {
    data: Option<GeometryData>,
    accepted: Option<(GeometryData, String)>,
    output: String,
    domain: Domain,
    view: String,
    geometry: Option<InspectedGeometry>,
    locations: Vec<[f64; 3]>,
    offset: usize,
    selected: Option<usize>,
    pending: bool,
    kind: &'static str,
    range: String,
}

impl OutputInspection {
    /// An empty inspector awaiting a build.
    pub fn new() -> Self {
        Self { pending: true, kind: "Awaiting valid build", ..Self::default() }
    }

    /// Forget every build (a new document); the preview keeps drawing its last accepted mesh until the next build.
    pub fn reset(&mut self, viewport: &mut Viewport) {
        *self = Self::new();
        viewport.inspection.set(Vec::new(), None);
    }

    /// Show a new build, keeping the chosen output when it still exists. Returns the preview's error when it rejected
    /// the result; the previous result then stays selected.
    pub fn update(&mut self, data: GeometryData, frame: bool, viewport: &mut Viewport) -> Result<(), String> {
        if !self.output_names(&data).contains(&self.output) {
            self.output = String::new();
        }
        self.data = Some(data);
        self.show_result(frame, viewport)
    }

    /// Choose the output the preview shows (`""` for all outputs, the full build).
    pub fn choose_output(&mut self, name: &str, viewport: &mut Viewport) -> Result<(), String> {
        self.output = name.to_owned();
        self.show_result(true, viewport)
    }

    /// Pair the preview with the chosen output; on rejection, return to the last accepted result.
    fn show_result(&mut self, frame: bool, viewport: &mut Viewport) -> Result<(), String> {
        let Some(data) = &self.data else {
            return Err("No build to show.".into());
        };
        let output = output_record(data, &self.output).cloned();
        let shown = match output.as_ref().and_then(|output| output.get("geometry")) {
            Some(geometry) => joy_engine::serde_json::from_value::<GeometryData>(geometry.clone())
                .map_err(|error| format!("Output geometry is malformed: {error}"))?,
            None => data.clone(),
        };
        if let Err(error) = viewport.set_data(&shown, frame) {
            match self.accepted.clone() {
                Some((previous, name)) => {
                    self.data = Some(previous);
                    self.output = name;
                }
                None => self.data = None,
            }
            return Err(error);
        }
        self.pending = false;
        self.accepted = Some((data.clone(), self.output.clone()));
        let instances = output.as_ref().is_some_and(|output| output["value"]["kind"] == "instances");
        self.geometry = Some(match &output {
            Some(output) if instances => inspected_value(&output["value"]["sites"]),
            Some(output) => inspected_value(&output["value"]),
            None => generated_geometry(data),
        });
        self.domain = if instances {
            Domain::Instances
        } else if self.domain == Domain::Instances {
            Domain::Points
        } else {
            self.domain
        };
        self.kind = match &output {
            Some(_) if instances => "Retained instances",
            Some(output) if output["visible"] == true => "Output geometry",
            Some(_) => "Inspection snapshot",
            None => "Full build · export source",
        };
        self.show_domain(viewport);
        Ok(())
    }

    /// Choose the attribute domain.
    pub fn choose_domain(&mut self, domain: Domain, viewport: &mut Viewport) {
        self.domain = domain;
        self.show_domain(viewport);
    }

    fn show_domain(&mut self, viewport: &mut Viewport) {
        self.offset = 0;
        self.selected = None;
        if !self.view_names().contains(&self.view) {
            self.view = String::new();
        }
        self.locations = self.compute_locations();
        self.visualize(viewport);
    }

    /// Choose the visualized attribute (`""` for the material view).
    pub fn choose_view(&mut self, view: &str, viewport: &mut Viewport) {
        self.view = view.to_owned();
        self.visualize(viewport);
    }

    /// Show the previous page of rows.
    pub fn previous_page(&mut self) {
        self.offset = self.offset.saturating_sub(PAGE_SIZE);
    }

    /// Show the next page of rows.
    pub fn next_page(&mut self) {
        self.offset += PAGE_SIZE;
        self.clamp_offset();
    }

    /// Select a row (or clear the selection) and highlight it in the preview.
    pub fn select(&mut self, index: Option<usize>, viewport: &mut Viewport) {
        self.selected = index.filter(|index| *index < self.locations.len());
        viewport.inspection.selected = self.selected;
    }

    /// A viewport pick: select the element and page to it.
    pub fn pick(&mut self, index: Option<usize>, viewport: &mut Viewport) {
        if self.pending {
            return;
        }
        if let Some(index) = index {
            self.offset = (index / PAGE_SIZE) * PAGE_SIZE;
        }
        self.select(index, viewport);
    }

    fn visualize(&mut self, viewport: &mut Viewport) {
        let attribute = self.attributes().iter().find(|column| column.name == self.view).cloned();
        let mut colors = None;
        self.range.clear();
        if let Some(attribute) = attribute.filter(|attribute| !attribute.values.is_empty()) {
            if attribute.size == 1 {
                let (minimum, maximum) = attribute
                    .values
                    .iter()
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| (low.min(*value), high.max(*value)));
                colors = Some(
                    attribute
                        .values
                        .iter()
                        .map(|value| {
                            let t = if maximum == minimum { 0.5 } else { (value - minimum) / (maximum - minimum) };
                            [(0.16 + (t * 0.77)) as f32, (0.45 - (t * 0.03)) as f32, (0.7 - (t * 0.48)) as f32]
                        })
                        .collect(),
                );
                self.range = format!("{}: {} → {}", attribute.name, format_cell(minimum), format_cell(maximum));
            } else if attribute.size == 4 {
                colors = Some(
                    attribute
                        .values
                        .chunks(4)
                        .map(|color| [color[0] as f32, color[1] as f32, color[2] as f32])
                        .collect(),
                );
                self.range = attribute.name.clone();
            }
        }
        viewport.inspection.set(self.locations.clone(), colors);
        viewport.inspection.selected = self.selected;
    }

    fn compute_locations(&self) -> Vec<[f64; 3]> {
        let Some(geometry) = &self.geometry else {
            return Vec::new();
        };
        let positions = geometry.positions();
        let point = |index: usize| -> [f64; 3] {
            positions.get(index * 3..(index * 3) + 3).map_or([0.0; 3], |slice| [slice[0], slice[1], slice[2]])
        };
        match self.domain {
            Domain::Points | Domain::Instances => (0..positions.len() / 3).map(point).collect(),
            Domain::Corners => geometry.faces.iter().flat_map(|face| face.iter().map(|index| point(*index))).collect(),
            Domain::Faces => geometry
                .faces
                .iter()
                .map(|face| {
                    let mut sum = [0.0; 3];
                    for index in face {
                        let position = point(*index);
                        for axis in 0..3 {
                            sum[axis] += position[axis];
                        }
                    }
                    // TypeScript divides by three, the corner count of every FORM face.
                    sum.map(|value| value / 3.0)
                })
                .collect(),
        }
    }

    fn clamp_offset(&mut self) {
        let total = self.locations.len();
        let last_page = (total.saturating_sub(1) / PAGE_SIZE) * PAGE_SIZE;
        self.offset = self.offset.min(last_page);
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Readouts for the UI
    // -----------------------------------------------------------------------------------------------------------------

    /// Output choices: `("", "All outputs")` then each named output, marking inspection-only snapshots.
    pub fn output_choices(&self) -> Vec<(String, String)> {
        let Some(data) = &self.data else {
            return vec![(String::new(), "Previous build".to_owned())];
        };
        let mut choices = vec![(String::new(), "All outputs".to_owned())];
        for output in data.outputs.iter().flatten() {
            let name = output["name"].as_str().unwrap_or_default().to_owned();
            let label = if output["visible"] == true { name.clone() } else { format!("{name} · inspect") };
            choices.push((name, label));
        }
        choices
    }

    fn output_names(&self, data: &GeometryData) -> Vec<String> {
        let mut names = vec![String::new()];
        names.extend(data.outputs.iter().flatten().filter_map(|output| output["name"].as_str().map(str::to_owned)));
        names
    }

    /// The chosen output (`""` for all outputs).
    pub fn output(&self) -> &str {
        &self.output
    }

    /// The domains this result offers.
    pub fn domains(&self) -> Vec<Domain> {
        if self.domain == Domain::Instances {
            vec![Domain::Instances]
        } else {
            vec![Domain::Points, Domain::Faces, Domain::Corners]
        }
    }

    /// The chosen domain.
    pub fn domain(&self) -> Domain {
        self.domain
    }

    /// Attribute columns of the chosen domain.
    pub fn attributes(&self) -> &[AttributeColumn] {
        self.geometry.as_ref().map_or(&[], |geometry| geometry.attributes(self.domain))
    }

    /// Visualizable attributes: scalars and the color attribute.
    pub fn view_names(&self) -> Vec<String> {
        let mut names = vec![String::new()];
        names.extend(
            self.attributes()
                .iter()
                .filter(|column| column.size == 1 || column.name == "color")
                .map(|column| column.name.clone()),
        );
        names
    }

    /// The visualized attribute (`""` for materials).
    pub fn view(&self) -> &str {
        &self.view
    }

    /// The visualization range text.
    pub fn range(&self) -> &str {
        &self.range
    }

    /// The result kind text above the preview.
    pub fn kind(&self) -> &'static str {
        self.kind
    }

    /// Whether a result is being prepared (controls disabled).
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// Mark the inspector pending while a build is outstanding.
    pub fn set_pending(&mut self, pending: bool) {
        self.pending = pending;
    }

    /// Element count of the domain.
    pub fn total(&self) -> usize {
        self.locations.len()
    }

    /// The first row shown.
    pub fn offset(&self) -> usize {
        self.offset.min((self.total().saturating_sub(1) / PAGE_SIZE) * PAGE_SIZE)
    }

    /// The selected element.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Column headings: INDEX, each attribute, and USER DATA where records exist.
    pub fn headings(&self) -> Vec<String> {
        let mut headings = vec!["INDEX".to_owned()];
        headings.extend(self.attributes().iter().map(|column| column.name.clone()));
        if self.records().is_some() {
            headings.push("USER DATA".to_owned());
        }
        headings
    }

    /// The rows of the current page: element index and formatted cells.
    pub fn rows(&self) -> Vec<(usize, Vec<String>)> {
        let start = self.offset();
        let end = (start + PAGE_SIZE).min(self.total());
        let records = self.records();
        (start..end)
            .map(|index| {
                let mut cells: Vec<String> = self
                    .attributes()
                    .iter()
                    .map(|column| {
                        column.element(index).iter().map(|value| format_cell(*value)).collect::<Vec<_>>().join(", ")
                    })
                    .collect();
                if let Some(records) = records {
                    cells.push(records.get(index).map_or_else(
                        || "undefined".to_owned(),
                        |record| crate::export::javascript_json(record.clone()).to_string(),
                    ));
                }
                (index, cells)
            })
            .collect()
    }

    /// The summary in the ATTRIBUTES heading.
    pub fn summary(&self) -> String {
        if self.geometry.is_none() {
            return "Build geometry to inspect".to_owned();
        }
        format!(
            "{} {} · {} attributes",
            crate::session::group_thousands(self.total()),
            self.domain.name(),
            self.attributes().len()
        )
    }

    /// The selection or paging readout.
    pub fn selection_info(&self) -> String {
        match self.selected.and_then(|index| self.locations.get(index).map(|location| (index, location))) {
            Some((index, location)) => format!(
                "{} {index} · {}",
                self.domain.name(),
                location.iter().map(|value| format_cell(*value)).collect::<Vec<_>>().join(", ")
            ),
            None if self.total() > 0 => {
                let start = self.offset();
                format!(
                    "Rows {}–{} · Select a row or click the viewport",
                    start + 1,
                    (start + PAGE_SIZE).min(self.total())
                )
            }
            None if self.geometry.is_some() => "Empty result · no elements to inspect".to_owned(),
            None => "Select a row or click a point in the viewport".to_owned(),
        }
    }

    /// Whether a previous page exists.
    pub fn has_previous_page(&self) -> bool {
        self.offset() > 0
    }

    /// Whether a next page exists.
    pub fn has_next_page(&self) -> bool {
        self.offset() + PAGE_SIZE < self.total()
    }

    fn records(&self) -> Option<&Vec<Json>> {
        if matches!(self.domain, Domain::Points | Domain::Instances) {
            self.geometry.as_ref().and_then(|geometry| geometry.user_data.as_ref())
        } else {
            None
        }
    }
}

/// A table cell: four decimals, printed as JavaScript prints the rounded number.
pub fn format_cell(value: f64) -> String {
    let rounded: f64 = format!("{value:.4}").parse().unwrap_or(value);
    format_js_number(rounded)
}

/// A named output record of a build.
fn output_record<'a>(data: &'a GeometryData, name: &str) -> Option<&'a Json> {
    if name.is_empty() {
        return None;
    }
    data.outputs.iter().flatten().find(|output| output["name"] == name)
}

/// The full build as inspected geometry (`generatedGeometry`): point positions, normals, RGBA colors, and UVs.
fn generated_geometry(data: &GeometryData) -> InspectedGeometry {
    let vertex_count = data.positions.len() / 3;
    let colors: Vec<f64> = (0..vertex_count)
        .flat_map(|index| {
            let rgb = data.colors.get(index * 3..(index * 3) + 3).unwrap_or(&[1.0, 1.0, 1.0]);
            [rgb[0], rgb[1], rgb[2], data.alphas.get(index).copied().unwrap_or(1.0)]
        })
        .collect();
    let uvs: Vec<f64> =
        data.uvs.chunks(2).flat_map(|uv| [uv[0], uv.get(1).copied().unwrap_or_default(), 0.0]).collect();
    InspectedGeometry {
        faces: data.triangles.chunks(3).map(|face| face.iter().map(|index| *index as usize).collect()).collect(),
        points: vec![
            AttributeColumn { name: "position".into(), size: 3, values: data.positions.clone() },
            AttributeColumn { name: "normal".into(), size: 3, values: data.normals.clone() },
            AttributeColumn { name: "color".into(), size: 4, values: colors },
            AttributeColumn { name: "uv".into(), size: 3, values: uvs },
        ],
        faces_attributes: Vec::new(),
        corners: Vec::new(),
        user_data: data.user_data.clone(),
    }
}

/// A serialized FORM geometry value (`{kind, faces, attributes: {points, faces, corners}, userData}`).
fn inspected_value(value: &Json) -> InspectedGeometry {
    let columns = |domain: &str| -> Vec<AttributeColumn> {
        value["attributes"][domain]
            .as_object()
            .map(|attributes| {
                attributes
                    .iter()
                    .map(|(name, attribute)| {
                        let size = attribute["size"].as_f64().unwrap_or(1.0) as usize;
                        let values = attribute["values"]
                            .as_array()
                            .map(|values| {
                                values
                                    .iter()
                                    .flat_map(|value| match value {
                                        Json::Array(components) => components.iter().filter_map(Json::as_f64).collect(),
                                        other => vec![other.as_f64().unwrap_or_default()],
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        AttributeColumn { name: name.clone(), size, values }
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    InspectedGeometry {
        faces: value["faces"]
            .as_array()
            .map(|faces| {
                faces
                    .iter()
                    .map(|face| {
                        face.as_array().map_or_else(Vec::new, |corners| {
                            corners.iter().filter_map(|index| index.as_f64().map(|index| index as usize)).collect()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        points: columns("points"),
        faces_attributes: columns("faces"),
        corners: columns("corners"),
        user_data: value["userData"].as_array().cloned(),
    }
}
