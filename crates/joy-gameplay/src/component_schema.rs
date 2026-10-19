// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Authoring contracts for component records: schemas, presets, conditional fields, and advisory validation for tools
//! such as Joy Editor's inspector.
//!
//! Schemas describe authored JSON; runtime behavior stays with the systems that interpret a component. Fields outside a
//! schema remain valid custom data and round trip unchanged, and component types without a schema are never reported.
//! Issue paths identify the authored fields that need attention.

use indexmap::IndexMap;
use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value;

use super::built_in_component_schemas::built_in_component_schemas;
use super::entity_definition::EntityDefinition;
use super::error::ComponentSchemaError;
use joy_core::JsonRecord;
use joy_core::format::format_number;

/// Value shapes a field can describe, serialized with the TypeScript `COMPONENT_FIELD_KIND` spellings. Authoring tools
/// choose controls from these; validation checks authored JSON against them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ComponentFieldKind {
    /// A finite JSON number.
    Number,
    /// `true` or `false`.
    Boolean,
    /// Any string.
    Text,
    /// One of the field's `choices`.
    Choice,
    /// Three finite numbers.
    Vector3,
    /// Three finite numbers (linear RGB).
    Color,
    /// A workspace asset path with one of the field's `asset_kinds` extensions.
    Asset,
    /// A free-form JSON object.
    Record,
    /// A JSON array, optionally of records described by `items`.
    List,
}

/// Show a field only while a sibling choice holds one of the listed values (for example box half extents only while
/// `shape` is `box`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentFieldCondition {
    /// Name of the sibling `choice` field.
    pub field: String,
    /// Values of the sibling that activate this field.
    pub equals: Vec<String>,
}

/// One authored field inside a component record. Numbers, vectors, and colors share bounds per channel.
///
/// `default` is the value the runtime assumes when the field is omitted; it is never written for the author.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentFieldSchema {
    /// Own property name inside the component record.
    pub name: String,
    /// Value shape.
    pub kind: ComponentFieldKind,
    /// Inspector label; the field name is used when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// One sentence of authoring help.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Display unit for numbers and vectors, such as `m`, `kg`, or `s`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// Runtime value for an omitted field; `None` means omission carries no implied value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    /// Report omission (or an empty asset path) as an issue.
    #[serde(default, skip_serializing_if = "is_false")]
    pub required: bool,
    /// Lower bound: inclusive, or exclusive when `min_exclusive` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// Makes `min` exclusive.
    #[serde(default, skip_serializing_if = "is_false")]
    pub min_exclusive: bool,
    /// Inclusive upper bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Suggested control increment; validation does not quantize to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<f64>,
    /// Require whole numbers.
    #[serde(default, skip_serializing_if = "is_false")]
    pub integer: bool,
    /// Accepted values for `choice` fields; required (nonempty) for them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    /// Workspace asset kinds (file extensions without the dot) accepted by `asset` fields; required for them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub asset_kinds: Vec<String>,
    /// Fields of each record item in a `list` field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<ComponentFieldSchema>>,
    /// Sibling condition that activates this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<ComponentFieldCondition>,
}

impl ComponentFieldSchema {
    /// A field with only a name and kind; set the remaining metadata with struct update syntax.
    pub fn new(name: impl Into<String>, kind: ComponentFieldKind) -> Self {
        Self {
            name: name.into(),
            kind,
            label: None,
            description: None,
            unit: None,
            default: None,
            required: false,
            min: None,
            min_exclusive: false,
            max: None,
            step: None,
            integer: false,
            choices: Vec::new(),
            asset_kinds: Vec::new(),
            items: None,
            when: None,
        }
    }
}

/// A named starting record offered when an author adds a component. Preset ids are unique across a catalog.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentPreset {
    /// Catalog-unique id, for example `physics.box`.
    pub id: String,
    /// Display label.
    pub label: String,
    /// Starting component record.
    pub data: JsonRecord,
}

/// Authoring contract for one component type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ComponentSchema {
    /// Component type (serialized as `type`).
    #[serde(rename = "type")]
    pub kind: String,
    /// Display label; also opens whole-component issue messages.
    pub label: String,
    /// One sentence describing the component.
    pub description: String,
    /// Display group, normally a [`component_category`](super::component_category) value.
    pub category: String,
    /// Field schemas in display order.
    pub fields: Vec<ComponentFieldSchema>,
    /// Starting records offered when adding the component; at least one is required.
    pub presets: Vec<ComponentPreset>,
    /// Component types this component needs on the same entity.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
}

/// One advisory validation result.
///
/// `field` is a dotted path inside the component record (`wheels.2.radius`), or `None` (serialized `null`) for
/// whole-component problems.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComponentIssue {
    /// Component type the issue belongs to.
    pub component: String,
    /// Dotted field path, or `None` for the whole component.
    pub field: Option<String>,
    /// Complete sentence describing the problem.
    pub message: String,
}

/// An owned component record created from a preset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CreatedComponent {
    /// Component type (serialized as `type`).
    #[serde(rename = "type")]
    pub kind: String,
    /// Fresh copy of the preset data, owned by the caller.
    pub data: JsonRecord,
}

/// Instance-owned set of component schemas for authoring tools and validation.
///
/// Registered schemas are copied; lookups borrow read-only records. The catalog is data only: it never constructs
/// runtime components or evaluates authored values. Registration order is display order.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentSchemaCatalog {
    schemas: IndexMap<String, ComponentSchema>,

    // Preset id → owning schema type, so tools can address one starting record directly.
    preset_owners: IndexMap<String, String>,
}

impl Default for ComponentSchemaCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl ComponentSchemaCatalog {
    /// A catalog holding this crate's simulation component schemas; `joy_scene::engine_component_catalog` adds the
    /// presentation components.
    pub fn new() -> Self {
        let mut catalog = Self::empty();
        for schema in built_in_component_schemas() {
            // The built-in schemas are valid and mutually unique; `built_in_schemas_register` tests this.
            catalog.register(schema).expect("built-in component schemas are valid");
        }
        catalog
    }

    /// A catalog without the built-in schemas (TypeScript `{builtIns: false}`).
    pub fn empty() -> Self {
        Self { schemas: IndexMap::new(), preset_owners: IndexMap::new() }
    }

    /// Register a project schema, taking ownership of it.
    ///
    /// Invalid schemas, duplicate types, and duplicate preset ids fail before anything is registered.
    pub fn register(&mut self, schema: ComponentSchema) -> Result<&mut Self, ComponentSchemaError> {
        validate_schema(&schema)?;
        if self.schemas.contains_key(&schema.kind) {
            return Err(ComponentSchemaError::DuplicateSchema(schema.kind));
        }
        if let Some(preset) = schema.presets.iter().find(|preset| self.preset_owners.contains_key(&preset.id)) {
            return Err(ComponentSchemaError::DuplicatePreset(preset.id.clone()));
        }

        for preset in &schema.presets {
            self.preset_owners.insert(preset.id.clone(), schema.kind.clone());
        }
        self.schemas.insert(schema.kind.clone(), schema);
        Ok(self)
    }

    /// Whether a schema describes this component type.
    pub fn has(&self, kind: &str) -> bool {
        self.schemas.contains_key(kind)
    }

    /// Borrow the schema for a component type, or `None` for custom component types.
    pub fn get(&self, kind: &str) -> Option<&ComponentSchema> {
        self.schemas.get(kind)
    }

    /// Borrow every schema in registration order.
    pub fn list(&self) -> impl Iterator<Item = &ComponentSchema> {
        self.schemas.values()
    }

    /// Create an owned component record from a preset id of any registered schema.
    pub fn create_component(&self, preset_id: &str) -> Result<CreatedComponent, ComponentSchemaError> {
        let preset = self
            .preset_owners
            .get(preset_id)
            .and_then(|kind| self.schemas.get(kind))
            .and_then(|schema| Some((schema, schema.presets.iter().find(|preset| preset.id == preset_id)?)));
        let Some((schema, preset)) = preset else {
            return Err(ComponentSchemaError::UnknownPreset(preset_id.to_owned()));
        };
        Ok(CreatedComponent { kind: schema.kind.clone(), data: preset.data.clone() })
    }

    /// Resolve the field schema for a path inside a component record, descending through list items.
    ///
    /// `path` names fields and list item indices, for example `["wheels", "0", "radius"]`. Returns `None` for unknown
    /// types and fields, free-form records, and paths ending on a list item (an item is a record, not a field).
    pub fn field(&self, kind: &str, path: &[&str]) -> Option<&ComponentFieldSchema> {
        let mut fields: Option<&[ComponentFieldSchema]> = self.schemas.get(kind).map(|schema| schema.fields.as_slice());
        let mut field: Option<&ComponentFieldSchema> = None;

        let mut index = 0;
        while index < path.len() {
            let candidates = fields?;
            let found = candidates.iter().find(|candidate| candidate.name == path[index])?;
            field = Some(found);

            // A list item index selects one record; the segment after it names a field of that record. A path that
            // ends on the index addresses the record itself, which has no field schema.
            if found.kind == ComponentFieldKind::List && index + 1 < path.len() {
                index += 1;
                if index + 1 >= path.len() {
                    return None;
                }
                fields = found.items.as_deref();
                index += 1;
                continue;
            }
            fields = None;
            index += 1;
        }
        field
    }

    /// Check one authored component against its schema. Custom component types have no issues.
    pub fn validate(&self, kind: &str, data: &Value) -> Vec<ComponentIssue> {
        let Some(schema) = self.schemas.get(kind) else {
            return Vec::new();
        };
        let Value::Object(record) = data else {
            return vec![ComponentIssue {
                component: kind.to_owned(),
                field: None,
                message: format!("{} must be a record.", schema.label),
            }];
        };
        validate_fields(kind, &schema.fields, record, "")
    }

    /// Check every schema-described component on an entity, plus required companion components.
    ///
    /// Linked objects should be resolved first. Issues follow the entity's component order.
    pub fn validate_entity(&self, entity: &EntityDefinition) -> Vec<ComponentIssue> {
        let mut issues = Vec::new();
        for (kind, data) in &entity.components {
            issues.extend(validate_fields_of(self, kind, data));

            let Some(schema) = self.schemas.get(kind) else {
                continue;
            };
            for required in &schema.requires {
                if entity.components.contains_key(required) {
                    continue;
                }
                let required_label = self.schemas.get(required).map_or(required.as_str(), |other| other.label.as_str());
                issues.push(ComponentIssue {
                    component: kind.clone(),
                    field: None,
                    message: format!("{} requires a {required_label} component.", schema.label),
                });
            }
        }
        issues
    }
}

/// Whether a field applies to its record under the field's sibling condition.
pub fn is_component_field_active(field: &ComponentFieldSchema, record: &JsonRecord) -> bool {
    let Some(condition) = &field.when else {
        return true;
    };
    match record.get(&condition.field) {
        Some(Value::String(value)) => condition.equals.iter().any(|candidate| candidate == value),
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Validation of authored records
// ---------------------------------------------------------------------------------------------------------------------

/// Validate an entity's component record (always an object) against the catalog.
fn validate_fields_of(catalog: &ComponentSchemaCatalog, kind: &str, record: &JsonRecord) -> Vec<ComponentIssue> {
    match catalog.schemas.get(kind) {
        None => Vec::new(),
        Some(schema) => validate_fields(kind, &schema.fields, record, ""),
    }
}

/// Validate the fields of one record, recursing into list items with an item schema.
fn validate_fields(
    component: &str,
    fields: &[ComponentFieldSchema],
    record: &JsonRecord,
    prefix: &str,
) -> Vec<ComponentIssue> {
    let issue = |field: String, message: String| ComponentIssue {
        component: component.to_owned(),
        field: Some(field),
        message,
    };
    let mut issues = Vec::new();
    for field in fields {
        if !is_component_field_active(field, record) {
            continue;
        }
        let path = format!("{prefix}{}", field.name);
        let label = sentence_start(field.label.as_deref().unwrap_or(&field.name));

        let Some(value) = record.get(&field.name) else {
            if field.required {
                issues.push(issue(path, format!("{label} is required.")));
            }
            continue;
        };
        if let Some(message) = describe_invalid_value(field, value) {
            issues.push(issue(path, format!("{label} {message}")));
            continue;
        }

        // Only list items with a declared item schema have nested fields to check.
        if let (ComponentFieldKind::List, Some(items), Value::Array(values)) = (field.kind, &field.items, value) {
            for (index, item) in values.iter().enumerate() {
                let Value::Object(item) = item else {
                    issues.push(issue(
                        format!("{path}.{index}"),
                        format!("{label} item {} must be a record.", index + 1),
                    ));
                    continue;
                };
                issues.extend(validate_fields(component, items, item, &format!("{path}.{index}.")));
            }
        }
    }
    issues
}

/// Describe why a present value does not satisfy its field, as a predicate completing "<label> ...".
fn describe_invalid_value(field: &ComponentFieldSchema, value: &Value) -> Option<String> {
    match field.kind {
        ComponentFieldKind::Number => {
            let Some(number) = value.as_f64().filter(|number| number.is_finite()) else {
                return Some("must be a finite number.".to_owned());
            };
            if field.integer && number.fract() != 0.0 {
                return Some("must be a whole number.".to_owned());
            }
            describe_out_of_range(field, &[number])
        }
        ComponentFieldKind::Boolean => (!value.is_boolean()).then(|| "must be true or false.".to_owned()),
        ComponentFieldKind::Text => (!value.is_string()).then(|| "must be text.".to_owned()),
        ComponentFieldKind::Choice => {
            let accepted = value.as_str().is_some_and(|text| field.choices.iter().any(|choice| choice == text));
            (!accepted).then(|| format!("must be one of {}.", field.choices.join(", ")))
        }
        ComponentFieldKind::Vector3 | ComponentFieldKind::Color => {
            let channels: Option<Vec<f64>> = match value {
                Value::Array(items) if items.len() == 3 => {
                    items.iter().map(|item| item.as_f64().filter(|number| number.is_finite())).collect()
                }
                _ => None,
            };
            match channels {
                None => Some("must contain three finite numbers.".to_owned()),
                Some(channels) => describe_out_of_range(field, &channels),
            }
        }
        ComponentFieldKind::Asset => describe_invalid_asset(field, value),
        ComponentFieldKind::Record => (!value.is_object()).then(|| "must be a record.".to_owned()),
        ComponentFieldKind::List => (!value.is_array()).then(|| "must be a list.".to_owned()),
    }
}

/// Check every numeric channel against the field's bounds.
fn describe_out_of_range(field: &ComponentFieldSchema, values: &[f64]) -> Option<String> {
    let channels = if values.len() > 1 { "values " } else { "" };
    if let Some(minimum) = field.min {
        let below = values.iter().any(|value| if field.min_exclusive { *value <= minimum } else { *value < minimum });
        if below {
            let bound = format_number(minimum);
            return Some(if field.min_exclusive {
                format!("{channels}must be greater than {bound}.")
            } else {
                format!("{channels}must be at least {bound}.")
            });
        }
    }
    if let Some(maximum) = field.max
        && values.iter().any(|value| *value > maximum)
    {
        return Some(format!("{channels}must be at most {}.", format_number(maximum)));
    }
    None
}

/// Accept an unassigned (empty) path for optional fields and require a matching extension otherwise.
fn describe_invalid_asset(field: &ComponentFieldSchema, value: &Value) -> Option<String> {
    let Value::String(path) = value else {
        return Some("must be an asset path.".to_owned());
    };
    if path.is_empty() {
        return field.required.then(|| "needs an asset.".to_owned());
    }
    let matches = field.asset_kinds.iter().any(|kind| path.ends_with(&format!(".{kind}")));
    if !field.asset_kinds.is_empty() && !matches {
        let extensions: Vec<String> = field.asset_kinds.iter().map(|kind| format!(".{kind}")).collect();
        return Some(format!("must reference a {} asset.", extensions.join(" or ")));
    }
    None
}

/// Capitalize a label's first character so it can open an issue sentence.
fn sentence_start(label: &str) -> String {
    let mut characters = label.chars();
    match characters.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(characters).collect(),
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// Schema validation
// ---------------------------------------------------------------------------------------------------------------------

/// Reject schemas that tools could not render or validate consistently.
fn validate_schema(schema: &ComponentSchema) -> Result<(), ComponentSchemaError> {
    if schema.kind.trim().is_empty() {
        return Err(ComponentSchemaError::EmptyType);
    }
    if schema.presets.is_empty() {
        return Err(ComponentSchemaError::NoPresets(schema.kind.clone()));
    }
    validate_field_list(&schema.kind, &schema.fields)?;

    let mut preset_ids: Vec<&str> = Vec::with_capacity(schema.presets.len());
    for preset in &schema.presets {
        if preset.id.trim().is_empty() {
            return Err(ComponentSchemaError::InvalidPreset(schema.kind.clone()));
        }
        if preset_ids.contains(&preset.id.as_str()) {
            return Err(ComponentSchemaError::RepeatedPreset { kind: schema.kind.clone(), preset: preset.id.clone() });
        }
        preset_ids.push(&preset.id);
    }
    Ok(())
}

/// Validate field names and kind-specific metadata, recursing into list item schemas.
fn validate_field_list(kind: &str, fields: &[ComponentFieldSchema]) -> Result<(), ComponentSchemaError> {
    let field_error = |field: &ComponentFieldSchema| (kind.to_owned(), field.name.clone());
    let mut names: Vec<&str> = Vec::with_capacity(fields.len());
    for field in fields {
        if field.name.trim().is_empty() || names.contains(&field.name.as_str()) {
            return Err(ComponentSchemaError::InvalidFieldNames(kind.to_owned()));
        }
        names.push(&field.name);

        if field.kind == ComponentFieldKind::Choice && field.choices.is_empty() {
            let (kind, field) = field_error(field);
            return Err(ComponentSchemaError::MissingChoices { kind, field });
        }
        if field.kind == ComponentFieldKind::Asset && field.asset_kinds.is_empty() {
            let (kind, field) = field_error(field);
            return Err(ComponentSchemaError::MissingAssetKinds { kind, field });
        }
        if let Some(items) = &field.items {
            if field.kind != ComponentFieldKind::List {
                let (kind, field) = field_error(field);
                return Err(ComponentSchemaError::ItemsOnNonList { kind, field });
            }
            validate_field_list(kind, items)?;
        }
    }

    // Conditions refer to sibling choices so tools can toggle them without evaluating code.
    for field in fields {
        let Some(condition) = &field.when else {
            continue;
        };
        let sibling = fields.iter().find(|candidate| candidate.name == condition.field);
        if sibling.is_none_or(|sibling| sibling.kind != ComponentFieldKind::Choice) {
            let (kind, field) = field_error(field);
            return Err(ComponentSchemaError::InvalidCondition { kind, field });
        }
    }
    Ok(())
}

/// Serde helper: omit false flags.
fn is_false(flag: &bool) -> bool {
    !*flag
}
