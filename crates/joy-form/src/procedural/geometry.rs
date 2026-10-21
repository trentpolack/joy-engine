// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Editable geometry as native data: points, triangular faces, and typed attributes on points, faces, and face
//! corners, plus retained instance collections.
//!
//! Attributes keep flat columns (`size` numbers per element for numeric attributes) so building, combining, and
//! realizing geometry never allocates per element. Snapshots for named outputs and inspection convert to the
//! documented JSON record (`{kind, faces, attributes: {points, faces, corners}, materials}`) once, when they are
//! taken.

use crate::geometry_data::AttributeValue;
use serde_json::{
    Map,
    Value as Json,
    json,
};

/// Point attribute holding positions; every geometry has it.
pub const POSITION_ATTRIBUTE: &str = "position";

/// Faces attribute naming each face's material, as an index into the geometry's material table.
pub const MATERIAL_ATTRIBUTE: &str = "material";

/// Attribute names with fixed numeric shapes that realization reads.
pub const STANDARD_ATTRIBUTES: [&str; 7] = ["position", "normal", "uv", "color", "rotation", "scale", "material"];

// Serialized attribute `type` tags; numeric attributes omit the tag.
const TEXT_TYPE: &str = "text";
const BOOL_TYPE: &str = "bool";
const MATERIAL_TYPE: &str = "material";

/// An attribute domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainKind {
    Points,
    Faces,
    Corners,
}

impl DomainKind {
    /// Every domain, from least to most specific.
    pub const ALL: [Self; 3] = [Self::Points, Self::Faces, Self::Corners];

    /// The domain a script names (`"points"`, `"faces"`, or `"corners"`).
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "points" => Ok(Self::Points),
            "faces" => Ok(Self::Faces),
            "corners" => Ok(Self::Corners),
            _ => Err("Expected points, faces, or corners attribute domain.".into()),
        }
    }

    /// The script spelling.
    pub fn name(self) -> &'static str {
        match self {
            Self::Points => "points",
            Self::Faces => "faces",
            Self::Corners => "corners",
        }
    }
}

/// What an attribute stores per element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeType {
    /// A number, an XYZ vector, or an RGBA color, by component count.
    Number(usize),
    Text,
    Bool,
    /// An index into the geometry's material table, or none.
    Material,
}

impl AttributeType {
    pub fn description(self) -> String {
        match self {
            Self::Number(1) => "a number".into(),
            Self::Number(3) => "an XYZ vector".into(),
            Self::Number(4) => "an RGBA color".into(),
            Self::Number(size) => format!("{size} numbers"),
            Self::Text => "text".into(),
            Self::Bool => "true/false".into(),
            Self::Material => "a material".into(),
        }
    }
}

/// One element's attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum Element {
    /// One, three, or four components.
    Numbers(Vec<f64>),
    Text(String),
    Bool(bool),
    /// A material table index, or none.
    Material(Option<usize>),
}

impl Element {
    pub fn kind(&self) -> AttributeType {
        match self {
            Self::Numbers(components) => AttributeType::Number(components.len()),
            Self::Text(_) => AttributeType::Text,
            Self::Bool(_) => AttributeType::Bool,
            Self::Material(_) => AttributeType::Material,
        }
    }

    /// The element as a realized attribute value. Materials are not custom attributes and never realize this way.
    pub fn value(&self) -> AttributeValue<'_> {
        match self {
            Self::Numbers(components) => AttributeValue::Numbers(components),
            Self::Text(text) => AttributeValue::Text(text),
            Self::Bool(flag) => AttributeValue::Bool(*flag),
            Self::Material(_) => unreachable!("materials realize as triangle materials"),
        }
    }

    /// The element as JSON, as snapshots store it: a number, an array of components, text, a boolean, or an index
    /// (`null` for no material).
    pub fn json(&self) -> Json {
        match self {
            Self::Numbers(components) if components.len() == 1 => json!(components[0]),
            Self::Numbers(components) => json!(components),
            Self::Text(text) => json!(text),
            Self::Bool(flag) => json!(flag),
            Self::Material(index) => index.map_or(Json::Null, |index| json!(index)),
        }
    }
}

/// One attribute's values for every element of its domain.
#[derive(Clone, Debug, PartialEq)]
pub enum Column {
    Number { size: usize, values: Vec<f64> },
    Text(Vec<String>),
    Bool(Vec<bool>),
    Material(Vec<Option<usize>>),
}

impl Column {
    /// A column of `length` copies of `element`.
    pub fn filled(element: &Element, length: usize) -> Self {
        match element {
            Element::Numbers(components) => Self::Number {
                size: components.len(),
                values: components.iter().copied().cycle().take(components.len() * length).collect(),
            },
            Element::Text(text) => Self::Text(vec![text.clone(); length]),
            Element::Bool(flag) => Self::Bool(vec![*flag; length]),
            Element::Material(index) => Self::Material(vec![*index; length]),
        }
    }

    pub fn kind(&self) -> AttributeType {
        match self {
            Self::Number {
                size,
                ..
            } => AttributeType::Number(*size),
            Self::Text(_) => AttributeType::Text,
            Self::Bool(_) => AttributeType::Bool,
            Self::Material(_) => AttributeType::Material,
        }
    }

    /// Components per element (1 for non-numeric columns), for allocation budgets.
    pub fn size(&self) -> usize {
        match self {
            Self::Number {
                size,
                ..
            } => *size,
            _ => 1,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Number {
                size,
                values,
            } => values.len() / size.max(&1),
            Self::Text(values) => values.len(),
            Self::Bool(values) => values.len(),
            Self::Material(values) => values.len(),
        }
    }

    /// The default element of this column's type: zero components, empty text, false, or no material.
    pub fn default_element(&self) -> Element {
        match self {
            Self::Number {
                size,
                ..
            } => Element::Numbers(vec![0.0; *size]),
            Self::Text(_) => Element::Text(String::new()),
            Self::Bool(_) => Element::Bool(false),
            Self::Material(_) => Element::Material(None),
        }
    }

    /// The numeric components of element `index` (empty for other columns).
    pub fn numbers(&self, index: usize) -> &[f64] {
        match self {
            Self::Number {
                size,
                values,
            } => &values[(index * size)..((index + 1) * size)],
            _ => &[],
        }
    }

    /// Element `index` as a realized attribute value, borrowed from the column.
    pub fn value(&self, index: usize) -> AttributeValue<'_> {
        match self {
            Self::Number {
                ..
            } => AttributeValue::Numbers(self.numbers(index)),
            Self::Text(values) => AttributeValue::Text(&values[index]),
            Self::Bool(values) => AttributeValue::Bool(values[index]),
            Self::Material(_) => unreachable!("materials realize as triangle materials"),
        }
    }

    pub fn get(&self, index: usize) -> Element {
        match self {
            Self::Number {
                ..
            } => Element::Numbers(self.numbers(index).to_vec()),
            Self::Text(values) => Element::Text(values[index].clone()),
            Self::Bool(values) => Element::Bool(values[index]),
            Self::Material(values) => Element::Material(values[index]),
        }
    }

    /// Replace element `index`; the caller has checked the element's type.
    pub fn set(&mut self, index: usize, element: &Element) {
        match (self, element) {
            (
                Self::Number {
                    size,
                    values,
                },
                Element::Numbers(components),
            ) => values[(index * *size)..((index + 1) * *size)].copy_from_slice(components),
            (Self::Text(values), Element::Text(text)) => values[index] = text.clone(),
            (Self::Bool(values), Element::Bool(flag)) => values[index] = *flag,
            (Self::Material(values), Element::Material(material)) => values[index] = *material,
            _ => unreachable!("attribute types are checked before writes"),
        }
    }

    /// Append an element of this column's type.
    pub fn push(&mut self, element: &Element) {
        match (self, element) {
            (
                Self::Number {
                    values,
                    ..
                },
                Element::Numbers(components),
            ) => values.extend_from_slice(components),
            (Self::Text(values), Element::Text(text)) => values.push(text.clone()),
            (Self::Bool(values), Element::Bool(flag)) => values.push(*flag),
            (Self::Material(values), Element::Material(material)) => values.push(*material),
            _ => unreachable!("attribute types are checked before writes"),
        }
    }

    /// Append `count` default elements.
    pub fn push_defaults(&mut self, count: usize) {
        match self {
            Self::Number {
                size,
                values,
            } => values.extend(std::iter::repeat_n(0.0, count * *size)),
            Self::Text(values) => values.extend(std::iter::repeat_n(String::new(), count)),
            Self::Bool(values) => values.extend(std::iter::repeat_n(false, count)),
            Self::Material(values) => values.extend(std::iter::repeat_n(None, count)),
        }
    }

    /// Append every element of `other`, which has the same type.
    pub fn extend(&mut self, other: &Column) {
        match (self, other) {
            (
                Self::Number {
                    values,
                    ..
                },
                Self::Number {
                    values: more,
                    ..
                },
            ) => values.extend_from_slice(more),
            (Self::Text(values), Self::Text(more)) => values.extend(more.iter().cloned()),
            (Self::Bool(values), Self::Bool(more)) => values.extend_from_slice(more),
            (Self::Material(values), Self::Material(more)) => values.extend_from_slice(more),
            _ => unreachable!("attribute types are checked before combining"),
        }
    }

    /// Remove the elements in `start..end`.
    pub fn remove_range(&mut self, start: usize, end: usize) {
        match self {
            Self::Number {
                size,
                values,
            } => {
                values.drain((start * *size)..(end * *size));
            }
            Self::Text(values) => {
                values.drain(start..end);
            }
            Self::Bool(values) => {
                values.drain(start..end);
            }
            Self::Material(values) => {
                values.drain(start..end);
            }
        }
    }

    /// Swap elements `a` and `b`.
    pub fn swap(&mut self, a: usize, b: usize) {
        match self {
            Self::Number {
                size,
                values,
            } => {
                for component in 0..*size {
                    values.swap((a * *size) + component, (b * *size) + component);
                }
            }
            Self::Text(values) => values.swap(a, b),
            Self::Bool(values) => values.swap(a, b),
            Self::Material(values) => values.swap(a, b),
        }
    }

    /// The column as a snapshot attribute record (`{"size", "values", "type"?}`).
    fn json(&self) -> Json {
        let values: Vec<Json> = match self {
            Self::Number {
                size: 1,
                values,
            } => values.iter().map(|value| json!(value)).collect(),
            Self::Number {
                size,
                values,
            } => values.chunks(*size).map(|components| json!(components)).collect(),
            _ => (0..self.len()).map(|index| self.get(index).json()).collect(),
        };
        let mut record = json!({"size": self.size(), "values": values});
        match self {
            Self::Text(_) => record["type"] = json!(TEXT_TYPE),
            Self::Bool(_) => record["type"] = json!(BOOL_TYPE),
            Self::Material(_) => record["type"] = json!(MATERIAL_TYPE),
            Self::Number {
                ..
            } => {}
        }
        record
    }
}

/// One domain's attributes in declaration order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Domain {
    entries: Vec<(String, Column)>,
}

impl Domain {
    pub fn get(&self, name: &str) -> Option<&Column> {
        self.entries.iter().find(|(entry, _)| entry == name).map(|(_, column)| column)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut Column> {
        self.entries.iter_mut().find(|(entry, _)| entry == name).map(|(_, column)| column)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Add a new attribute after the existing ones.
    pub fn insert(&mut self, name: &str, column: Column) {
        self.entries.push((name.to_owned(), column));
    }

    /// Remove an attribute, keeping the others' order.
    pub fn remove(&mut self, name: &str) -> Option<Column> {
        let index = self.entries.iter().position(|(entry, _)| entry == name)?;
        Some(self.entries.remove(index).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Column)> {
        self.entries.iter().map(|(name, column)| (name.as_str(), column))
    }

    pub fn columns_mut(&mut self) -> impl Iterator<Item = &mut Column> {
        self.entries.iter_mut().map(|(_, column)| column)
    }

    pub fn names(&self) -> Vec<String> {
        self.entries.iter().map(|(name, _)| name.clone()).collect()
    }

    /// Total stored components, for allocation budgets.
    pub fn components(&self) -> usize {
        self.entries.iter().map(|(_, column)| column.len() * column.size()).sum()
    }

    fn json(&self) -> Json {
        Json::Object(self.entries.iter().map(|(name, column)| (name.clone(), column.json())).collect::<Map<_, _>>())
    }
}

/// Editable geometry: points with positions, triangular faces over point indices, attributes, and the material
/// records the faces `material` attribute indexes.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub faces: Vec<[usize; 3]>,
    pub points: Domain,
    pub face_attributes: Domain,
    pub corners: Domain,
    pub materials: Vec<Json>,
}

impl Default for Geometry {
    fn default() -> Self {
        let mut points = Domain::default();
        points.insert(
            POSITION_ATTRIBUTE,
            Column::Number {
                size: 3,
                values: Vec::new(),
            },
        );
        Self {
            faces: Vec::new(),
            points,
            face_attributes: Domain::default(),
            corners: Domain::default(),
            materials: Vec::new(),
        }
    }
}

impl Geometry {
    pub fn count(&self, domain: DomainKind) -> usize {
        match domain {
            DomainKind::Points => self.points.get(POSITION_ATTRIBUTE).map_or(0, Column::len),
            DomainKind::Faces => self.faces.len(),
            DomainKind::Corners => self.faces.len() * 3,
        }
    }

    pub fn domain(&self, domain: DomainKind) -> &Domain {
        match domain {
            DomainKind::Points => &self.points,
            DomainKind::Faces => &self.face_attributes,
            DomainKind::Corners => &self.corners,
        }
    }

    pub fn domain_mut(&mut self, domain: DomainKind) -> &mut Domain {
        match domain {
            DomainKind::Points => &mut self.points,
            DomainKind::Faces => &mut self.face_attributes,
            DomainKind::Corners => &mut self.corners,
        }
    }

    /// Point `index`'s position.
    pub fn position(&self, index: usize) -> [f64; 3] {
        let components = self.points.get(POSITION_ATTRIBUTE).expect("geometry always has positions").numbers(index);
        [components[0], components[1], components[2]]
    }

    /// Every point position, flattened.
    pub fn positions(&self) -> &[f64] {
        match self.points.get(POSITION_ATTRIBUTE) {
            Some(Column::Number {
                values,
                ..
            }) => values,
            _ => &[],
        }
    }

    /// Points plus three components per face corner, for operation budgets.
    pub fn size(&self) -> usize {
        self.count(DomainKind::Points) + (self.faces.len() * 3)
    }

    /// Total stored attribute components, for allocation budgets.
    pub fn components(&self) -> usize {
        self.points.components() + self.face_attributes.components() + self.corners.components()
    }

    /// The documented snapshot record.
    pub fn json(&self) -> Json {
        let mut record = json!({
            "kind": "geometry",
            "faces": self.faces,
            "attributes": {
                "points": self.points.json(),
                "faces": self.face_attributes.json(),
                "corners": self.corners.json(),
            },
        });
        if self.face_attributes.contains(MATERIAL_ATTRIBUTE) {
            record["materials"] = json!(self.materials);
        }
        record
    }
}

/// A retained instance collection: a source geometry placed at every site point, with an optional material record
/// (`null` for none) that overrides the source's.
#[derive(Clone, Debug, PartialEq)]
pub struct Instances {
    pub source: Geometry,
    pub sites: Geometry,
    pub material: Json,
}

impl Instances {
    /// Site points times the source's points and corners, for operation budgets.
    pub fn size(&self) -> usize {
        self.sites.count(DomainKind::Points) * self.source.size()
    }

    /// The documented snapshot record.
    pub fn json(&self) -> Json {
        json!({"kind": "instances", "source": self.source.json(), "sites": self.sites.json(), "material": self.material})
    }
}
