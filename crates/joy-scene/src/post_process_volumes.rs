// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Resolution of a level's `postProcess` volumes into the settings the renderer applies at a view point.
//!
//! [`PostProcessVolumes`] reads every entity's `postProcess` component (see
//! [`PostProcessComponent`](crate::PostProcessComponent)) and, each frame, resolves the settings at a view point:
//! starting from the engine defaults, enabled volumes apply in priority order (equal priorities in authored entity
//! order), each moved toward by its weight times its coverage of the point. Unbounded volumes always cover it; bounded
//! volumes cover it only inside their shape, fading in over `blendDistance` meters inside the surface. The resolver is
//! presentation only: it never touches simulation state or gameplay randomness, and games pass the point they choose
//! (usually the camera eye).

use joy_core::Transform;
use joy_core::math::{
    Mat4,
    Vec3,
};
use joy_gameplay::{
    LevelData,
    component_type,
};
use joy_renderer::{
    PostProcessOverrides,
    PostProcessSettings,
};

use crate::components::{
    PostProcessComponent,
    PostProcessShape,
};
use crate::level_scene::{
    LevelEntity,
    LevelSceneError,
};

/// One volume placed in the world.
#[derive(Clone, Debug)]
struct PlacedVolume {
    /// Owning entity id.
    entity_id: String,
    /// Interpreted component.
    component: PostProcessComponent,
    /// World space to the entity's unscaled local space (inverse of translation and rotation).
    world_to_local: Mat4,
    /// The entity's per-axis scale, applied to the shape in local space.
    scale: Vec3,
}

/// A level's post-process volumes and the settings they resolve to; see the module documentation.
#[derive(Clone, Debug, Default)]
pub struct PostProcessVolumes {
    /// Volumes in application order: priority ascending, then authored order.
    volumes: Vec<PlacedVolume>,
    /// The last resolution's per-volume weights and result, reused while the weights hold.
    resolved: Option<(Vec<f32>, PostProcessSettings)>,
}

impl PostProcessVolumes {
    /// No volumes; resolves to the engine defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// The volumes of a level's authored entities.
    pub fn from_level(level: &LevelData) -> Result<Self, LevelSceneError> {
        let mut volumes = Self::new();
        volumes.sync(&level.entities)?;
        Ok(volumes)
    }

    /// Replace the volumes with those of `entities` (authored definitions or runtime entity state).
    ///
    /// Every record is validated first, so a failed sync leaves the previous volumes intact.
    pub fn sync<'a, E: LevelEntity + 'a>(
        &mut self,
        entities: impl IntoIterator<Item = &'a E>,
    ) -> Result<(), LevelSceneError> {
        let mut volumes = Vec::new();
        for entity in entities {
            let Some(record) = entity.entity_component(component_type::POST_PROCESS) else {
                continue;
            };
            let component = PostProcessComponent::read(record)
                .map_err(|source| LevelSceneError::Component { entity: entity.entity_id().to_owned(), source })?;
            let transform = entity.entity_transform();
            let placement = Transform::new(transform.position, transform.rotation, Vec3::ONE);
            volumes.push(PlacedVolume {
                entity_id: entity.entity_id().to_owned(),
                component,
                world_to_local: placement.matrix().inverse(),
                scale: transform.scale,
            });
        }

        // A stable sort keeps authored order among equal priorities.
        volumes.sort_by(|left, right| left.component.priority.total_cmp(&right.component.priority));
        self.volumes = volumes;
        self.resolved = None;
        Ok(())
    }

    /// Number of volumes.
    pub fn len(&self) -> usize {
        self.volumes.len()
    }

    /// Whether there are no volumes.
    pub fn is_empty(&self) -> bool {
        self.volumes.is_empty()
    }

    /// The interpreted component of the volume on `entity_id`.
    pub fn volume(&self, entity_id: &str) -> Option<&PostProcessComponent> {
        self.volumes.iter().find(|volume| volume.entity_id == entity_id).map(|volume| &volume.component)
    }

    /// The entity whose volume sets the level's overall look: the lowest-priority enabled unbounded volume.
    pub fn global_volume(&self) -> Option<&str> {
        self.volumes
            .iter()
            .find(|volume| volume.component.enabled && volume.component.unbounded)
            .map(|volume| volume.entity_id.as_str())
    }

    /// Replace one volume's settings, for live editing; returns whether `entity_id` has a volume.
    pub fn set_overrides(&mut self, entity_id: &str, overrides: PostProcessOverrides) -> bool {
        let Some(volume) = self.volumes.iter_mut().find(|volume| volume.entity_id == entity_id) else {
            return false;
        };
        if !volume.component.overrides.same_values(&overrides) {
            volume.component.overrides = overrides;
            self.resolved = None;
        }
        true
    }

    /// Each volume's contribution at `view_point`: weight times coverage, in application order.
    pub fn contributions(&self, view_point: Vec3) -> Vec<(&str, f32)> {
        self.volumes.iter().map(|volume| (volume.entity_id.as_str(), contribution(volume, view_point))).collect()
    }

    /// The settings at `view_point` (world space, meters). Repeated calls with unchanged contributions reuse the
    /// previous result.
    pub fn resolve(&mut self, view_point: Vec3) -> PostProcessSettings {
        let weights: Vec<f32> = self.volumes.iter().map(|volume| contribution(volume, view_point)).collect();
        if let Some((previous, settings)) = &self.resolved
            && *previous == weights
        {
            return settings.clone();
        }

        let mut settings = PostProcessSettings::DEFAULT;
        for (volume, weight) in self.volumes.iter().zip(&weights) {
            if *weight > 0.0 {
                settings = settings.blend_toward(&volume.component.overrides, *weight);
            }
        }
        self.resolved = Some((weights, settings.clone()));
        settings
    }
}

/// A volume's weight times its coverage of `view_point`, in `[0, 1]`.
fn contribution(volume: &PlacedVolume, view_point: Vec3) -> f32 {
    let component = &volume.component;
    if !component.enabled || component.weight <= 0.0 {
        return 0.0;
    }
    if component.unbounded {
        return component.weight;
    }

    // Distance from the point to the shape's surface, inside it, in world meters; negative outside.
    let local = volume.world_to_local.transform_point3(view_point);
    let depth = match component.shape {
        PostProcessShape::Box { half_extents } => {
            let half_extents = half_extents * volume.scale.abs();
            (half_extents - local.abs()).min_element()
        }
        PostProcessShape::Sphere { radius } => (radius * volume.scale.abs().max_element()) - local.length(),
    };
    if depth < 0.0 {
        return 0.0;
    }
    let coverage = if component.blend_distance > 0.0 { (depth / component.blend_distance).min(1.0) } else { 1.0 };
    component.weight * coverage
}
