// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The schema-driven component inspector as data.
//!
//! [`build_property_tree`] turns an entity's components into [`PropertyNode`]s: component schemas choose controls,
//! labels, units, bounds, field order, and missing-field suggestions (**+ FIELD**); fields a sibling choice disables
//! are marked inactive; advisory issues attach to their component and field. Custom components and fields outside a
//! schema keep generic controls (records, arrays, booleans, numbers, and text), so any JSON stays editable without
//! source. The UI renders the tree and reports edits as paths and JSON values; the level editor commits them.

use std::collections::HashMap;

use joy_engine::gameplay::{
    ComponentFieldKind,
    ComponentFieldSchema,
    ComponentIssue,
    ComponentSchemaCatalog,
    is_component_field_active,
};
use joy_engine::serde_json::{
    Map,
    Value,
};

use crate::interaction::numeric_controls::{
    NumberBounds,
    format_number,
};
use crate::server::AssetEntry;

/// Inspector rows rendered before large data is left to Advanced Source.
const MAX_ROWS: usize = 600;

/// Deepest nested path rendered.
const MAX_DEPTH: usize = 12;

/// Units for custom fields, chosen by name since they have no schema.
const CUSTOM_UNITS: [(&str, &str); 10] = [
    ("mass", "kg"),
    ("radius", "m"),
    ("halfExtents", "m"),
    ("size", "m"),
    ("position", "m"),
    ("lifetime", "s"),
    ("duration", "s"),
    ("range", "m"),
    ("speed", "m/s"),
    ("intensity", "linear"),
];

/// Custom triples drawn as compact vectors.
const CUSTOM_VECTOR_NAMES: [&str; 6] = ["size", "halfExtents", "color", "position", "rotation", "scale"];

/// Per-path display metadata for asset-declared parameters (FORM and JOYFX bounds), keyed by dotted path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FieldMetadata {
    /// Label override.
    pub label: Option<String>,
    /// Unit override.
    pub unit: Option<String>,
    /// Lower bound.
    pub min: Option<f64>,
    /// Upper bound.
    pub max: Option<f64>,
    /// Increment.
    pub step: Option<f64>,
    /// Fixed choices.
    pub choices: Vec<String>,
    /// Help shown on hover, such as a FORM parameter's `info`.
    pub description: Option<String>,
    /// Whole numbers only (FORM `int`, `bool`, `seed`, and `enum` parameters).
    pub integer: bool,
    /// Labels for a numeric option index (a FORM `enum` parameter); the field stores the chosen index.
    pub options: Vec<String>,
    /// Whether the number is a random seed, edited with a reroll button.
    pub seed: bool,
}

/// Everything a tree build borrows.
pub struct PropertyContext<'a> {
    /// Component schemas.
    pub schemas: &'a ComponentSchemaCatalog,
    /// Advisory issues for the resolved components.
    pub issues: &'a [ComponentIssue],
    /// Asset parameter metadata by dotted path.
    pub metadata: &'a HashMap<String, FieldMetadata>,
    /// The project's assets, for asset selectors.
    pub assets: &'a [AssetEntry],
}

/// How one scalar field is edited.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldControl {
    /// A typed number with bounds.
    Number(NumberBounds),
    /// Free text; `placeholder` explains a `null` value.
    Text {
        /// Hint shown for empty text.
        placeholder: Option<String>,
    },
    /// A checkbox.
    Boolean,
    /// One of fixed choices; the authored value is always offered.
    Choice(Vec<String>),
    /// A project asset of accepted kinds: `(value, label)` options, starting with the unassigned choice.
    Asset(Vec<(String, String)>),
    /// A numeric option index shown with these labels; the field stores the chosen index.
    Indexed(Vec<String>),
    /// A whole-number random seed with these bounds, edited with a reroll button.
    Seed(NumberBounds),
}

/// A scalar field.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyField {
    /// Component type followed by the field path.
    pub path: Vec<String>,
    /// Label with its unit.
    pub label: String,
    /// Schema help.
    pub description: Option<String>,
    /// The displayed value.
    pub value: Value,
    /// The control.
    pub control: FieldControl,
    /// Why the field is unused under its sibling choice, when it is.
    pub inactive: Option<String>,
    /// The advisory issue on this field.
    pub issue: Option<String>,
}

/// A spatial or color triple (or quadruple) edited per channel.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyVector {
    /// Component type followed by the field path.
    pub path: Vec<String>,
    /// Uppercase label with its unit.
    pub label: String,
    /// Schema help.
    pub description: Option<String>,
    /// Channel values.
    pub values: Vec<f64>,
    /// Channel labels (`X`, `Y`, `Z` or `R`, `G`, `B`).
    pub channels: Vec<&'static str>,
    /// Per-channel bounds.
    pub bounds: NumberBounds,
    /// Whether the vector is a linear RGB color.
    pub color: bool,
    /// Why the vector is unused under its sibling choice, when it is.
    pub inactive: Option<String>,
    /// The advisory issue on this vector.
    pub issue: Option<String>,
}

/// A component, record, or list.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyGroup {
    /// Component type followed by the record path.
    pub path: Vec<String>,
    /// Heading (schema label, list item number, or name), with the item count for lists.
    pub label: String,
    /// The component type, shown beside a schema label that differs from it.
    pub type_name: Option<String>,
    /// Schema description of a component.
    pub description: Option<String>,
    /// Every issue of a component, listed above its fields.
    pub issues: Vec<String>,
    /// Fields, vectors, and nested groups in display order.
    pub children: Vec<PropertyNode>,
    /// Schema fields the record omits that apply now: `(name, label)` for **+ FIELD**.
    pub missing_fields: Vec<(String, String)>,
    /// Whether the group is a list (its add action appends an item).
    pub list: bool,
    /// Whether the group is a whole component.
    pub component: bool,
    /// Whether the group starts expanded.
    pub open: bool,
}

/// One inspector node.
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyNode {
    /// A component, record, or list.
    Group(PropertyGroup),
    /// A compact vector.
    Vector(PropertyVector),
    /// A scalar field.
    Field(PropertyField),
    /// Data beyond the inspector's bounds; it is preserved and editable in Advanced Source.
    Note(String),
}

/// Kinds offered for a new custom field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CustomFieldKind {
    /// Zero.
    Number,
    /// Empty text.
    Text,
    /// False.
    Boolean,
    /// An empty record.
    Group,
    /// An empty list.
    Array,
}

impl CustomFieldKind {
    /// Every kind, in menu order.
    pub const ALL: [Self; 5] = [Self::Number, Self::Text, Self::Boolean, Self::Group, Self::Array];

    /// Menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Number => "number",
            Self::Text => "text",
            Self::Boolean => "boolean",
            Self::Group => "group",
            Self::Array => "array",
        }
    }

    /// The starting value.
    pub fn initial(self) -> Value {
        match self {
            Self::Number => Value::from(0),
            Self::Text => Value::from(""),
            Self::Boolean => Value::Bool(false),
            Self::Group => Value::Object(Map::new()),
            Self::Array => Value::Array(Vec::new()),
        }
    }
}

/// Build inspector nodes for `components` (resolved, with display-only parameter defaults filled in).
pub fn build_property_tree(components: &Map<String, Value>, context: &PropertyContext<'_>) -> Vec<PropertyNode> {
    let mut builder = TreeBuilder {
        context,
        rows: 0,
    };
    components.iter().map(|(name, value)| builder.field(vec![name.clone()], value, None)).collect()
}

/// The value **+ FIELD** writes: the field's runtime default, otherwise the smallest valid value of its kind.
pub fn initial_field_value(field: &ComponentFieldSchema) -> Value {
    if let Some(default) = &field.default {
        return default.clone();
    }
    match field.kind {
        ComponentFieldKind::Number => {
            let minimum = field.min.unwrap_or(0.0);
            let value = if field.min_exclusive { minimum + field.step.unwrap_or(1.0) } else { minimum };
            crate::interaction::numeric_controls::json_number(value)
        }
        ComponentFieldKind::Boolean => Value::Bool(false),
        ComponentFieldKind::Choice => Value::from(field.choices.first().cloned().unwrap_or_default()),
        ComponentFieldKind::Vector3 => Value::from(vec![0, 0, 0]),
        ComponentFieldKind::Color => Value::from(vec![1, 1, 1]),
        ComponentFieldKind::Record => Value::Object(Map::new()),
        ComponentFieldKind::List => Value::Array(Vec::new()),
        ComponentFieldKind::Text | ComponentFieldKind::Asset => Value::from(""),
    }
}

/// `record` with a new custom field, or a message when the name is blank or taken.
pub fn add_custom_field(record: &Value, name: &str, kind: CustomFieldKind) -> Result<Value, String> {
    let name = name.trim();
    let Value::Object(fields) = record else {
        return Err("Only records take named fields.".to_owned());
    };
    if name.is_empty() || fields.contains_key(name) {
        return Err("Use a unique property name.".to_owned());
    }
    let mut next = fields.clone();
    next.insert(name.to_owned(), kind.initial());
    Ok(Value::Object(next))
}

/// `list` with a starter item: a copy of the last item, or zero for an empty list.
pub fn append_list_item(list: &Value) -> Option<Value> {
    let Value::Array(items) = list else {
        return None;
    };
    let mut next = items.clone();
    next.push(items.last().cloned().unwrap_or(Value::from(0)));
    Some(Value::Array(next))
}

/// The value at `path` inside `root` (record keys and list indices).
pub fn value_at<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut value = root;
    for part in path {
        value = match value {
            Value::Object(record) => record.get(part)?,
            Value::Array(items) => items.get(part.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(value)
}

/// JavaScript `String(value)` for display and choice matching.
pub fn display_string(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.as_f64().map_or_else(|| number.to_string(), format_number),
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        other => other.to_string(),
    }
}

/// Walks components into nodes, counting rows against the inspector bound.
struct TreeBuilder<'a, 'b> {
    context: &'a PropertyContext<'b>,
    rows: usize,
}

/// Schema lookups that borrow the context rather than the builder, so the builder can recurse.
fn field_schema<'a>(context: &'a PropertyContext<'_>, path: &[String]) -> Option<&'a ComponentFieldSchema> {
    if path.len() < 2 {
        return None;
    }
    let rest: Vec<&str> = path[1..].iter().map(String::as_str).collect();
    context.schemas.field(&path[0], &rest)
}

/// The schema fields describing a record: a component's fields, or a list item's fields.
fn record_fields<'a>(context: &'a PropertyContext<'_>, path: &[String]) -> Option<&'a [ComponentFieldSchema]> {
    if path.len() == 1 {
        return context.schemas.get(&path[0]).map(|schema| schema.fields.as_slice());
    }
    let parent: Vec<&str> = path[1..path.len() - 1].iter().map(String::as_str).collect();
    let list = context.schemas.field(&path[0], &parent)?;
    let is_item =
        list.kind == ComponentFieldKind::List && path.last().is_some_and(|part| part.parse::<usize>().is_ok());
    if is_item { list.items.as_deref() } else { None }
}

impl TreeBuilder<'_, '_> {
    /// One field, vector, or group; `container` is the record holding it, for sibling conditions.
    fn field(&mut self, path: Vec<String>, value: &Value, container: Option<&Map<String, Value>>) -> PropertyNode {
        self.rows += 1;
        if self.rows > MAX_ROWS || path.len() > MAX_DEPTH {
            return PropertyNode::Note("Large data is preserved. Use Advanced Source for this section.".to_owned());
        }
        let context = self.context;
        let schema = field_schema(context, &path);
        if let Some(values) = vector_values(schema, &path, value) {
            return PropertyNode::Vector(self.vector(path, values, schema, container));
        }
        if matches!(value, Value::Object(_) | Value::Array(_)) {
            return PropertyNode::Group(self.group(path, value));
        }
        PropertyNode::Field(self.row(path, value, schema, container))
    }

    /// A component, record, or list. Schema fields come first in schema order; custom fields follow in authored order.
    fn group(&mut self, path: Vec<String>, value: &Value) -> PropertyGroup {
        let name = path.last().cloned().unwrap_or_default();
        let component = path.len() == 1;
        let context = self.context;
        let component_schema = if component { context.schemas.get(&name) } else { None };
        let list_field = field_schema(context, &path);
        let schema_label = component_schema
            .map(|schema| schema.label.clone())
            .or_else(|| list_field.and_then(|field| field.label.clone()));
        let count = value.as_array().map(Vec::len);
        let label = format!(
            "{}{}",
            group_label(&path, schema_label.as_deref()),
            count.map_or(String::new(), |count| format!(" · {count}"))
        );
        let type_name =
            component_schema.filter(|schema| schema.label.to_lowercase() != name.to_lowercase()).map(|_| name.clone());
        let issues = if component {
            self.context
                .issues
                .iter()
                .filter(|issue| issue.component == name)
                .map(|issue| issue.message.clone())
                .collect()
        } else {
            Vec::new()
        };

        let fields = if value.is_array() { None } else { record_fields(context, &path) };
        let mut children = Vec::new();
        match value {
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    let mut child = path.clone();
                    child.push(index.to_string());
                    children.push(self.field(child, item, None));
                }
            }
            Value::Object(record) => {
                for key in ordered_keys(record, fields) {
                    let mut child = path.clone();
                    child.push(key.clone());
                    if let Some(entry) = record.get(&key) {
                        children.push(self.field(child, entry, Some(record)));
                    }
                }
            }
            _ => {}
        }

        let missing_fields = match (fields, value) {
            (Some(fields), Value::Object(record)) => fields
                .iter()
                .filter(|field| !record.contains_key(&field.name) && is_component_field_active(field, record))
                .map(|field| (field.name.clone(), field.label.clone().unwrap_or_else(|| field.name.clone())))
                .collect(),
            _ => Vec::new(),
        };
        PropertyGroup {
            path: path.clone(),
            label,
            type_name,
            description: component_schema.map(|schema| schema.description.clone()),
            issues,
            children,
            missing_fields,
            list: value.is_array(),
            component,
            open: path.len() < 3,
        }
    }

    /// A scalar field with the control its schema, metadata, or value type implies.
    fn row(
        &mut self,
        path: Vec<String>,
        value: &Value,
        schema: Option<&ComponentFieldSchema>,
        container: Option<&Map<String, Value>>,
    ) -> PropertyField {
        let key = path.join(".");
        let name = path.last().cloned().unwrap_or_default();
        let default_metadata = FieldMetadata::default();
        let metadata = self.context.metadata.get(&key).unwrap_or(&default_metadata);
        let unit = metadata
            .unit
            .clone()
            .or_else(|| schema.and_then(|schema| schema.unit.clone()))
            .or_else(|| if schema.is_none() { custom_unit(&name).map(str::to_owned) } else { None });
        let base = metadata
            .label
            .clone()
            .or_else(|| schema.and_then(|schema| schema.label.clone()))
            .unwrap_or_else(|| name.replace('_', " "));
        let label = match &unit {
            Some(unit) => format!("{base} · {unit}"),
            None => base,
        };

        let choices = if !metadata.choices.is_empty() {
            Some(metadata.choices.clone())
        } else {
            schema.filter(|schema| !schema.choices.is_empty()).map(|schema| schema.choices.clone())
        };
        let control = if schema.is_some_and(|schema| schema.kind == ComponentFieldKind::Asset) {
            FieldControl::Asset(
                self.asset_options(value, &schema.map(|schema| schema.asset_kinds.clone()).unwrap_or_default()),
            )
        } else if let Some(choices) = choices {
            let mut offered = vec![display_string(value)];
            for choice in choices {
                if !offered.contains(&choice) {
                    offered.push(choice);
                }
            }
            FieldControl::Choice(offered)
        } else if !metadata.options.is_empty() && value.is_number() {
            FieldControl::Indexed(metadata.options.clone())
        } else {
            let bounds = NumberBounds {
                min: metadata.min.or_else(|| schema.and_then(|schema| schema.min)),
                max: metadata.max.or_else(|| schema.and_then(|schema| schema.max)),
                step: metadata
                    .step
                    .or_else(|| schema.and_then(|schema| if schema.integer { Some(1.0) } else { schema.step })),
                integer: metadata.integer || schema.is_some_and(|schema| schema.integer),
            };
            match value {
                Value::Bool(_) => FieldControl::Boolean,
                Value::Number(_) if metadata.seed => FieldControl::Seed(bounds),
                Value::Number(_) => FieldControl::Number(bounds),
                Value::Null => FieldControl::Text {
                    placeholder: Some("null — enter text to replace".to_owned()),
                },
                _ => FieldControl::Text {
                    placeholder: None,
                },
            }
        };
        PropertyField {
            inactive: inactive_reason(schema, container),
            issue: self.issue(&path),
            path,
            label,
            description: metadata.description.clone().or_else(|| schema.and_then(|schema| schema.description.clone())),
            value: value.clone(),
            control,
        }
    }

    /// A compact vector.
    fn vector(
        &mut self,
        path: Vec<String>,
        values: Vec<f64>,
        schema: Option<&ComponentFieldSchema>,
        container: Option<&Map<String, Value>>,
    ) -> PropertyVector {
        let name = path.last().cloned().unwrap_or_default();
        let color = match schema {
            Some(schema) => schema.kind == ComponentFieldKind::Color,
            None => name == "color",
        };
        let unit = schema
            .and_then(|schema| schema.unit.clone())
            .or_else(|| if schema.is_none() { custom_unit(&name).map(str::to_owned) } else { None });
        let base = schema.and_then(|schema| schema.label.clone()).unwrap_or(name);
        let label = match unit {
            Some(unit) => format!("{base} · {unit}"),
            None => base,
        }
        .to_uppercase();
        let channels = if color { vec!["R", "G", "B", "A"] } else { vec!["X", "Y", "Z", "W"] }
            .into_iter()
            .take(values.len())
            .collect();
        PropertyVector {
            inactive: inactive_reason(schema, container),
            issue: self.issue(&path),
            path,
            label,
            description: schema.and_then(|schema| schema.description.clone()),
            values,
            channels,
            bounds: NumberBounds {
                min: schema.and_then(|schema| schema.min),
                max: schema.and_then(|schema| schema.max),
                step: schema.and_then(|schema| schema.step),
                integer: false,
            },
            color,
        }
    }

    /// Asset selector options limited to `kinds`, keeping a missing reference visible.
    fn asset_options(&self, value: &Value, kinds: &[String]) -> Vec<(String, String)> {
        let mut options = vec![(String::new(), "Choose an asset…".to_owned())];
        let assets: Vec<&AssetEntry> = self.context.assets.iter().filter(|asset| kinds.contains(&asset.kind)).collect();
        if let Value::String(current) = value
            && !current.is_empty()
            && !assets.iter().any(|asset| asset.path == *current)
        {
            options.push((current.clone(), format!("{current} (missing)")));
        }
        for asset in assets {
            options.push((asset.path.clone(), asset.path.strip_prefix("assets/").unwrap_or(&asset.path).to_owned()));
        }
        options
    }

    /// The issue on one field, if any.
    fn issue(&self, path: &[String]) -> Option<String> {
        let field = path[1..].join(".");
        self.context
            .issues
            .iter()
            .find(|issue| issue.component == path[0] && issue.field.as_deref() == Some(field.as_str()))
            .map(|issue| issue.message.clone())
    }
}

/// Channel values when a value uses the compact vector control: schema vectors and colors, or custom triples named
/// like the engine's spatial and color fields.
fn vector_values(schema: Option<&ComponentFieldSchema>, path: &[String], value: &Value) -> Option<Vec<f64>> {
    let items = value.as_array()?;
    if !(items.len() == 3 || items.len() == 4) {
        return None;
    }
    let values: Vec<f64> = items.iter().filter_map(Value::as_f64).collect();
    if values.len() != items.len() {
        return None;
    }
    let qualifies = match schema {
        Some(schema) => matches!(schema.kind, ComponentFieldKind::Vector3 | ComponentFieldKind::Color),
        None => path.last().is_some_and(|name| CUSTOM_VECTOR_NAMES.contains(&name.as_str())),
    };
    qualifies.then_some(values)
}

/// Record keys with schema fields first (schema order), then custom keys (authored order).
fn ordered_keys(record: &Map<String, Value>, fields: Option<&[ComponentFieldSchema]>) -> Vec<String> {
    let Some(fields) = fields else {
        return record.keys().cloned().collect();
    };
    let mut keys: Vec<String> =
        fields.iter().map(|field| field.name.clone()).filter(|name| record.contains_key(name)).collect();
    for key in record.keys() {
        if !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    keys
}

/// A group's heading: its schema label, `#n` for list items, or its name.
fn group_label(path: &[String], schema_label: Option<&str>) -> String {
    let name = path.last().map(String::as_str).unwrap_or_default();
    if let Some(label) = schema_label {
        return label.to_owned();
    }
    if path.len() > 1
        && let Ok(index) = name.parse::<usize>()
    {
        return format!("#{}", index + 1);
    }
    name.replace('_', " ")
}

/// Why a schema field is unused under its record's current sibling values.
fn inactive_reason(schema: Option<&ComponentFieldSchema>, container: Option<&Map<String, Value>>) -> Option<String> {
    let (schema, container) = (schema?, container?);
    let condition = schema.when.as_ref()?;
    if is_component_field_active(schema, container) {
        return None;
    }
    let current = container.get(&condition.field).map_or_else(|| "undefined".to_owned(), display_string);
    Some(format!("Unused while {} is {current}.", condition.field))
}

/// The conventional unit of a custom field name.
fn custom_unit(name: &str) -> Option<&'static str> {
    CUSTOM_UNITS.iter().find(|(field, _)| *field == name).map(|(_, unit)| *unit)
}
