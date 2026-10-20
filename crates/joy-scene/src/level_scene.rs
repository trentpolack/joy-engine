// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Presentation of authored levels: entities with `render` and `light` components become scene instances and point
//! lights. Used identically by games, the editor's previews, and tools, so a level looks the same everywhere.

use std::collections::HashMap;

use indexmap::IndexMap;
use joy_core::{
    JsonRecord,
    LinearColor,
    PrimitiveShape,
    Transform,
};
use joy_gameplay::{
    EntityDefinition,
    EntityState,
    EntityTransform,
    PresentedEntity,
    component_type,
};

use joy_renderer::{
    MeshAsset,
    MeshInstanceId,
    PointLight,
    RenderError,
    RenderScene,
};

use crate::components::{
    ComponentError,
    LightComponent,
    RENDER_COMPONENT_DEFAULTS,
    RenderComponent,
    RenderSource,
};
use crate::form_assets::{
    primitive_box,
    primitive_sphere,
};

/// Read access to an entity's presentation data; implemented for authored definitions and runtime entity state.
pub trait LevelEntity {
    /// Stable entity identifier.
    fn entity_id(&self) -> &str;
    /// World placement.
    fn entity_transform(&self) -> &EntityTransform;
    /// The authored component record of `kind`, if present.
    fn entity_component(&self, kind: &str) -> Option<&JsonRecord>;
}

impl LevelEntity for EntityDefinition {
    fn entity_id(&self) -> &str {
        &self.id
    }

    fn entity_transform(&self) -> &EntityTransform {
        &self.transform
    }

    fn entity_component(&self, kind: &str) -> Option<&JsonRecord> {
        self.components.get(kind)
    }
}

impl LevelEntity for EntityState {
    fn entity_id(&self) -> &str {
        self.id().as_str()
    }

    fn entity_transform(&self) -> &EntityTransform {
        &self.transform
    }

    fn entity_component(&self, kind: &str) -> Option<&JsonRecord> {
        self.component(kind)
    }
}

/// Runtime entities at their presentation placement (physics bodies blended between fixed steps); see
/// `World::presented_entities`.
impl LevelEntity for PresentedEntity<'_> {
    fn entity_id(&self) -> &str {
        self.state.id().as_str()
    }

    fn entity_transform(&self) -> &EntityTransform {
        &self.transform
    }

    fn entity_component(&self, kind: &str) -> Option<&JsonRecord> {
        self.state.component(kind)
    }
}

/// An entity's component could not be presented.
#[derive(Debug, thiserror::Error)]
pub enum LevelSceneError {
    /// A `render` or `light` record was invalid (for example, a negative light range or an unsafe asset path).
    #[error("Entity {entity}: {source}")]
    Component {
        /// The entity's identifier.
        entity: String,
        /// What was wrong with the record.
        source: ComponentError,
    },
    /// A mesh asset failed to load or compile.
    #[error("Entity {entity}: {source}")]
    Mesh {
        /// The entity's identifier.
        entity: String,
        /// The loading or compilation failure.
        source: RenderError,
    },
}

/// Retained presentation of a level's entities in a borrowed [`RenderScene`].
///
/// Entities without a `render` component, or whose render record names neither a resolvable asset nor a box or sphere,
/// are invisible. Entity scale multiplies `render.size` (full dimensions). Instances are owned by this object and live
/// in the scene passed to [`sync`](Self::sync); always pass the same scene.
pub struct LevelScene {
    box_asset: MeshAsset,
    sphere_asset: MeshAsset,
    instances: IndexMap<String, MeshInstanceId>,
    lights: Vec<PointLight>,
}

impl Default for LevelScene {
    fn default() -> Self {
        Self::new()
    }
}

impl LevelScene {
    /// Create empty presentation with shared placeholder primitives.
    pub fn new() -> Self {
        Self {
            box_asset: primitive_box(),
            sphere_asset: primitive_sphere(),
            instances: IndexMap::new(),
            lights: Vec::new(),
        }
    }

    /// The shared unit box (full extents of one), for game-owned details drawn alongside the level.
    pub fn box_asset(&self) -> &MeshAsset {
        &self.box_asset
    }

    /// The shared sphere of diameter one.
    pub fn sphere_asset(&self) -> &MeshAsset {
        &self.sphere_asset
    }

    /// World-space point lights from enabled `light` components after the latest sync.
    pub fn lights(&self) -> &[PointLight] {
        &self.lights
    }

    /// The scene instance presenting `entity_id`, if it is visible.
    pub fn instance(&self, entity_id: &str) -> Option<MeshInstanceId> {
        self.instances.get(entity_id).copied()
    }

    /// Synchronize primitives and lights from `entities`; asset-backed render components stay hidden.
    pub fn sync<'a, E: LevelEntity + 'a>(
        &mut self,
        scene: &mut RenderScene,
        entities: impl IntoIterator<Item = &'a E>,
        selected: Option<&str>,
    ) -> Result<(), LevelSceneError> {
        self.sync_with(scene, entities, selected, |_, _| Ok(None))
    }

    /// Synchronize with a mesh resolver for asset-backed render components (see
    /// [`FormAssetLibrary`](crate::FormAssetLibrary)).
    ///
    /// `resolve_mesh` receives the entity id and its interpreted render component and returns the asset to draw, or
    /// `None` to fall back to the component's primitive shape. Every record is validated before the scene changes, so a
    /// failed sync leaves the previous presentation intact. The selected entity is drawn with a lightened tint.
    pub fn sync_with<'a, E: LevelEntity + 'a>(
        &mut self,
        scene: &mut RenderScene,
        entities: impl IntoIterator<Item = &'a E>,
        selected: Option<&str>,
        mut resolve_mesh: impl FnMut(&str, &RenderComponent) -> Result<Option<MeshAsset>, RenderError>,
    ) -> Result<(), LevelSceneError> {
        struct Placement {
            id: String,
            asset: MeshAsset,
            transform: Transform,
            tint: LinearColor,
        }

        let mut lights = Vec::new();
        let mut placements = Vec::new();
        for entity in entities {
            let id = entity.entity_id();
            let component_error = |source| LevelSceneError::Component {
                entity: id.to_owned(),
                source,
            };
            let transform = entity.entity_transform();

            if let Some(record) = entity.entity_component(component_type::LIGHT) {
                let light = LightComponent::read(record).map_err(component_error)?;
                if light.enabled {
                    lights.push(PointLight {
                        position: transform.position,
                        color: light.radiance(),
                        range: light.range,
                        casts_shadow: light.casts_shadow,
                    });
                }
            }

            let Some(record) = entity.entity_component(component_type::RENDER) else {
                continue;
            };
            let render = RenderComponent::read(record).map_err(component_error)?;
            let resolved = match render.source {
                RenderSource::Mesh {
                    ..
                } => resolve_mesh(id, &render).map_err(|source| LevelSceneError::Mesh {
                    entity: id.to_owned(),
                    source,
                })?,
                _ => None,
            };
            let (asset, color) = match (resolved, primitive_shape(record)) {
                (Some(asset), _) => (asset, render.color),
                (None, Some(shape)) => {
                    let asset = if shape == PrimitiveShape::Box { &self.box_asset } else { &self.sphere_asset };
                    // An unresolved mesh drawn as its fallback shape takes the primitive default color unless the
                    // record sets one.
                    let color =
                        if has_color(record) { render.color } else { RENDER_COMPONENT_DEFAULTS.primitive_color };
                    (asset.clone(), color)
                }
                (None, None) => continue,
            };
            // Lift toward white without a cap, so HDR tints brighten rather than dim when selected.
            let highlight = |value: f32| (value * 0.65) + 0.35;
            let tint = if selected == Some(id) {
                LinearColor::rgb(highlight(color.red), highlight(color.green), highlight(color.blue))
            } else {
                color
            };
            let transform = Transform::new(transform.position, transform.rotation, transform.scale * render.size);
            placements.push(Placement {
                id: id.to_owned(),
                asset,
                transform,
                tint,
            });
        }

        let mut live = HashMap::with_capacity(placements.len());
        for placement in placements {
            let existing = self
                .instances
                .get(&placement.id)
                .copied()
                .filter(|id| scene.get(*id).is_some_and(|instance| instance.asset.ptr_eq(&placement.asset)));
            let instance_id = match existing {
                Some(id) => id,
                None => {
                    if let Some(stale) = self.instances.get(&placement.id) {
                        scene.remove(*stale);
                    }
                    scene.create_mesh(placement.asset)
                }
            };
            if let Some(instance) = scene.get_mut(instance_id) {
                instance.transform = placement.transform;
                instance.tint = placement.tint;
            }
            self.instances.insert(placement.id.clone(), instance_id);
            live.insert(placement.id, ());
        }
        self.instances.retain(|id, instance| {
            let keep = live.contains_key(id);
            if !keep {
                scene.remove(*instance);
            }
            keep
        });
        self.lights = lights;
        Ok(())
    }

    /// Remove every instance this object created from `scene` and forget the lights.
    pub fn clear(&mut self, scene: &mut RenderScene) {
        for (_, instance) in self.instances.drain(..) {
            scene.remove(instance);
        }
        self.lights.clear();
    }
}

/// The record's drawable primitive shape; a mesh resolver returning `None` falls back to it.
fn primitive_shape(record: &JsonRecord) -> Option<PrimitiveShape> {
    record
        .get("shape")
        .and_then(serde_json::Value::as_str)
        .and_then(PrimitiveShape::parse)
        .filter(|shape| matches!(shape, PrimitiveShape::Box | PrimitiveShape::Sphere))
}

/// Whether the record authors a valid color (three finite numbers).
fn has_color(record: &JsonRecord) -> bool {
    record
        .get("color")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|color| color.len() == 3 && color.iter().all(|value| value.as_f64().is_some_and(f64::is_finite)))
}

#[cfg(test)]
#[path = "level_scene.test.rs"]
mod tests;
