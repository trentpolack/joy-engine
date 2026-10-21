// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM placements: the points a script tags with user data so its host places something there.
//!
//! FORM stays a geometry language; a script asks for content by emitting isolated points inside
//! `with userData({kind: "tree", object: "assets/objects/tree.joyobject"}) { point(x, y, z) }`. Hosts read those points
//! as [`FormPlacement`]s in the asset's local space (script units). Two conventions are shared by every host:
//!
//! - `kind` names what the point is for; games map kinds to their own rules (structures, spawns, encounters).
//! - `object` names a `.joyobject` template; [`placement_entities`] turns those placements into linked entity
//!   instances, so a level built from FORM assets gets real entities (with their render, light, effect, and physics
//!   components) without game code. Optional `rotation` (XYZ Euler radians) and `scale` (a number or an XYZ triple)
//!   orient and size the instance relative to its parent.
//!
//! Placement entity ids are `<parent id>/<placement index>`, stable for a given script and parameter set.

use joy_core::math::Vec3;
use joy_core::{
    JsonRecord,
    euler_from_quat,
    quat_from_euler,
};
use joy_form::GeometryData;
use joy_gameplay::{
    EntityDefinition,
    EntityTransform,
    read_vector,
};
use serde_json::Value;

/// User data field naming what a placement is for.
pub const PLACEMENT_KIND: &str = "kind";

/// User data field naming the `.joyobject` template instanced at a placement.
pub const PLACEMENT_OBJECT: &str = "object";

/// User data field holding a placement's XYZ Euler rotation in radians.
pub const PLACEMENT_ROTATION: &str = "rotation";

/// User data field holding a placement's uniform (number) or per-axis (XYZ) scale.
pub const PLACEMENT_SCALE: &str = "scale";

/// Separator between a parent entity id and a placement index in spawned entity ids.
pub const PLACEMENT_ID_SEPARATOR: char = '/';

/// Most object placements one asset may spawn, so a script cannot flood a level with entities.
pub const MAXIMUM_OBJECT_PLACEMENTS: usize = 4096;

/// One tagged FORM point: its user data and position in the asset's local space.
#[derive(Clone, Debug, PartialEq)]
pub struct FormPlacement {
    /// The point's position in script units, in the asset's local space.
    pub position: Vec3,
    /// The point's whole user data record.
    pub data: JsonRecord,
}

impl FormPlacement {
    /// The `kind` field, when it is text.
    pub fn kind(&self) -> Option<&str> {
        self.data.get(PLACEMENT_KIND).and_then(Value::as_str)
    }

    /// The `object` template path, when it is nonempty text.
    pub fn object(&self) -> Option<&str> {
        self.data.get(PLACEMENT_OBJECT).and_then(Value::as_str).filter(|path| !path.is_empty())
    }

    /// The placement's own rotation (XYZ Euler radians); zero when absent or malformed.
    pub fn rotation(&self) -> Vec3 {
        self.data.get(PLACEMENT_ROTATION).and_then(read_vector).unwrap_or(Vec3::ZERO)
    }

    /// The placement's own scale: a positive number for uniform scale or a positive XYZ triple; one otherwise.
    pub fn scale(&self) -> Vec3 {
        match self.data.get(PLACEMENT_SCALE) {
            Some(Value::Number(number)) => {
                let value = number.as_f64().unwrap_or(1.0) as f32;
                if value.is_finite() && value > 0.0 { Vec3::splat(value) } else { Vec3::ONE }
            }
            Some(value) => read_vector(value).filter(|scale| scale.min_element() > 0.0).unwrap_or(Vec3::ONE),
            None => Vec3::ONE,
        }
    }

    /// The placement's world transform under `parent`: the parent's scale, rotation, and position applied to the local
    /// point, rotations composed (parent first), and scales multiplied per axis.
    pub fn world_transform(&self, parent: &EntityTransform) -> EntityTransform {
        let parent_rotation = quat_from_euler(parent.rotation);
        let position = parent.position + (parent_rotation * (self.position * parent.scale));
        let rotation = euler_from_quat(parent_rotation * quat_from_euler(self.rotation()));
        EntityTransform {
            position,
            rotation,
            scale: parent.scale * self.scale(),
        }
    }
}

/// Every point of `geometry` that carries a user data record, in point order.
///
/// Only isolated points (`point(...)` commands) are placements; triangle vertices keep their user data for exports and
/// inspection.
pub fn form_placements(geometry: &GeometryData) -> Vec<FormPlacement> {
    let Some(user_data) = geometry.user_data.as_deref() else {
        return Vec::new();
    };
    let mut placements = Vec::new();
    for &point in &geometry.points {
        let index = point as usize;
        let (Some(Value::Object(record)), Some(coordinates)) =
            (user_data.get(index), geometry.positions.get((index * 3)..((index * 3) + 3)))
        else {
            continue;
        };
        placements.push(FormPlacement {
            position: Vec3::new(coordinates[0] as f32, coordinates[1] as f32, coordinates[2] as f32),
            data: record.clone(),
        });
    }
    placements
}

/// Linked entity instances for `parent`'s object placements, in placement order, with world transforms.
///
/// Each entity links its placement's `object` template (resolve templates with `resolve_level_objects` before running
/// them) and overrides a `formPlacement` component holding the placement record, so behaviors can read the script's
/// data.
/// Fails when the placements name more than [`MAXIMUM_OBJECT_PLACEMENTS`] objects.
pub fn placement_entities(
    parent: &EntityDefinition,
    placements: &[FormPlacement],
) -> Result<Vec<EntityDefinition>, PlacementLimitError> {
    let mut entities = Vec::new();
    for (index, placement) in placements.iter().enumerate() {
        let Some(object) = placement.object() else {
            continue;
        };
        if entities.len() == MAXIMUM_OBJECT_PLACEMENTS {
            return Err(PlacementLimitError {
                entity: parent.id.clone(),
            });
        }
        let mut entity = EntityDefinition::new(placement_entity_id(&parent.id, index));
        entity.name = format!("{} {}", parent.name, placement.kind().unwrap_or(object));
        entity.transform = placement.world_transform(&parent.transform);
        entity.template = Some(object.to_owned());
        // Instances inherit everything from their template; the placement record rides along as an override.
        let mut components = JsonRecord::new();
        components.insert(PLACEMENT_COMPONENT.to_owned(), Value::Object(placement.data.clone()));
        let mut overrides = JsonRecord::new();
        overrides.insert(OVERRIDE_COMPONENTS.to_owned(), Value::Object(components));
        entity.overrides = Some(overrides);
        entities.push(entity);
    }
    Ok(entities)
}

/// The id of the entity spawned for placement `index` of `parent_id`.
pub fn placement_entity_id(parent_id: &str, index: usize) -> String {
    format!("{parent_id}{PLACEMENT_ID_SEPARATOR}{index}")
}

/// Component carrying a spawned entity's placement record.
pub const PLACEMENT_COMPONENT: &str = "formPlacement";

/// The object-instance override record that holds component overrides.
const OVERRIDE_COMPONENTS: &str = "components";

/// A FORM asset asked for more object placements than one asset may spawn.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Entity {entity}: FORM placements name more than {MAXIMUM_OBJECT_PLACEMENTS} objects.")]
pub struct PlacementLimitError {
    /// The parent entity whose asset emitted the placements.
    pub entity: String,
}

#[cfg(test)]
#[path = "form_placements.test.rs"]
mod tests;
