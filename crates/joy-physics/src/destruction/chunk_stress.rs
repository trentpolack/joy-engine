// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Optional load-path stress for supported chunk structures, with staged, delayed overload failure.
//!
//! Every supported chunk carries its own mass plus everything resting on it, and hands that load toward the ground
//! along the support graph: a chunk bears on the chunks directly beneath it when it has any (shared by contact area),
//! and otherwise on its side neighbors, which carry only a fraction of a material's strength. A chunk's capacity is
//! the contact area it bears on times its material strength, scaled by its remaining health, so damaging a column
//! raises the stress of the column itself. Circular bearing paths are cut at a grounded contact: a suspended chunk
//! cannot carry the chunk it depends on for support. Stress is a readable gameplay rule, not an elastic, torque, or
//! fracture solver: it is evaluated once per commit, in a fixed order, with `f64` arithmetic, so seeded runs reproduce exactly.
//!
//! Units: chunk geometry uses the structure's world units; density is mass per cubic world unit; strength is the mass
//! one square world unit of contact bears at full health. Mass units are the caller's (kilograms with meters).

use serde::{
    Deserialize,
    Serialize,
};

use super::DestructionError;
use super::chunk_structure::DestructibleChunkDefinition;
use super::chunk_support::{
    ChunkSupportGraph,
    SupportEvaluation,
};

/// Load ratio at which a chunk reads as strained when [`ChunkStressOptions::warning_ratio`] uses the default.
pub const DEFAULT_WARNING_RATIO: f64 = 0.7;

/// Load ratio at which a chunk is overloaded and starts its failure delay.
pub const OVERLOAD_RATIO: f64 = 1.0;

/// Density and bearing strength for one chunk material.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkMaterialStrength {
    /// Material name, matching [`DestructibleChunkDefinition::material`]; nonempty and unique within the options.
    pub material: String,
    /// Mass per cubic world unit; finite and positive.
    pub density: f64,
    /// Mass one square world unit of contact bears at full health before overloading; finite and positive.
    pub strength: f64,
}

/// Material table, failure staging, and timing for chunk stress.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkStressOptions {
    /// Per-material density and strength; materials not listed use the defaults below.
    pub materials: Vec<ChunkMaterialStrength>,
    /// Density for unlisted materials, in mass per cubic world unit; finite and positive.
    pub default_density: f64,
    /// Strength for unlisted materials, in mass per square world unit; finite and positive.
    pub default_strength: f64,
    /// Fraction of a material's strength that side contacts (bracing and cantilevers) bear, in `(0, 1]`.
    pub lateral_strength_fraction: f64,
    /// Load ratio at or above which a chunk reads as [`ChunkStressStage::Strained`], in `(0, 1)`.
    pub warning_ratio: f64,
    /// Seconds an overloaded chunk holds before it fails; finite and nonnegative. Zero fails it on the next
    /// [`ChunkStructure::advance_stress`](super::ChunkStructure::advance_stress).
    pub failure_delay_seconds: f64,
}

impl ChunkStressOptions {
    /// Mass of one chunk: its volume (its shape's, or its box's) times its material density, in the options' mass
    /// units.
    pub fn chunk_mass(&self, chunk: &DestructibleChunkDefinition) -> f64 {
        chunk.volume() * self.material(&chunk.material).map_or(self.default_density, |material| material.density)
    }

    /// Full-health bearing strength of a material, in mass per square world unit.
    pub fn strength(&self, material: &str) -> f64 {
        self.material(material).map_or(self.default_strength, |entry| entry.strength)
    }

    /// Reject non-positive densities or strengths, empty or duplicate material names, a lateral fraction outside
    /// `(0, 1]`, a warning ratio outside `(0, 1)`, or a negative or non-finite failure delay.
    pub fn validate(&self) -> Result<(), DestructionError> {
        let positive = |value: f64| value.is_finite() && value > 0.0;
        let valid_defaults = positive(self.default_density) && positive(self.default_strength);
        let valid_fraction = positive(self.lateral_strength_fraction) && self.lateral_strength_fraction <= 1.0;
        let valid_warning = positive(self.warning_ratio) && self.warning_ratio < OVERLOAD_RATIO;
        let valid_delay = self.failure_delay_seconds.is_finite() && self.failure_delay_seconds >= 0.0;
        if !valid_defaults || !valid_fraction || !valid_warning || !valid_delay {
            return Err(DestructionError::InvalidStressOptions);
        }

        for (index, entry) in self.materials.iter().enumerate() {
            let duplicate = self.materials[..index].iter().any(|earlier| earlier.material == entry.material);
            if entry.material.is_empty() || duplicate || !positive(entry.density) || !positive(entry.strength) {
                return Err(DestructionError::InvalidStressOptions);
            }
        }
        Ok(())
    }

    fn material(&self, material: &str) -> Option<&ChunkMaterialStrength> {
        self.materials.iter().find(|entry| entry.material == material)
    }
}

/// How close a chunk is to failing under its load.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChunkStressStage {
    /// Below the warning ratio, inactive, or unsupported.
    Stable,
    /// At or above the warning ratio but still within capacity: the telegraph stage.
    Strained,
    /// Overloaded; the chunk fails once its failure delay runs out unless its load drops first.
    Failing,
}

/// Copied stress diagnostics for one chunk, from the latest commit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkStressState {
    /// Stable chunk id.
    pub chunk_id: String,
    /// Mass bearing on this chunk's supports: its own mass plus everything it carries. Zero when inactive or
    /// unsupported.
    pub load: f64,
    /// Mass its supports bear at its current health before overloading. Zero when inactive or unsupported.
    pub capacity: f64,
    /// `load / capacity`; zero when inactive or unsupported, infinite when a supported chunk has no health-scaled
    /// capacity left.
    pub ratio: f64,
    /// Stage derived from the ratio.
    pub stage: ChunkStressStage,
    /// Seconds until a failing chunk fails, or `None` for every other stage.
    pub seconds_until_failure: Option<f64>,
}

/// Static stress inputs for a fixed chunk collection: masses, bearing strengths, and incoming contacts.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChunkStressModel {
    // Validated options, kept for diagnostics and mass queries.
    options: ChunkStressOptions,

    // Per-chunk values in generation order.
    masses: Vec<f64>,
    strengths: Vec<f64>,
    footprints: Vec<f64>,

    // Contacts that can carry each chunk: (carrier index, contact area, lateral), from the support graph.
    carriers: Vec<Vec<CarrierContact>>,
}

/// One contact through which a chunk can hand load to a carrier.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CarrierContact {
    carrier: usize,
    contact_area: f64,
    lateral: bool,
}

/// Load and capacity for one chunk from one evaluation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ChunkLoad {
    pub(crate) load: f64,
    pub(crate) capacity: f64,
}

impl ChunkStressModel {
    /// Validate options and precompute per-chunk inputs from the support graph's contacts.
    pub(crate) fn new(
        chunks: &[&DestructibleChunkDefinition],
        graph: &ChunkSupportGraph,
        options: ChunkStressOptions,
    ) -> Result<Self, DestructionError> {
        options.validate()?;

        // Support edges point from the supporting chunk to the supported one; stress needs them reversed.
        let mut carriers = vec![Vec::new(); chunks.len()];
        for (source, edges) in graph.edges().iter().enumerate() {
            for edge in edges {
                carriers[edge.target].push(CarrierContact {
                    carrier: source,
                    contact_area: edge.contact_area,
                    lateral: edge.lateral,
                });
            }
        }

        Ok(Self {
            masses: chunks.iter().map(|chunk| options.chunk_mass(chunk)).collect(),
            strengths: chunks.iter().map(|chunk| options.strength(&chunk.material)).collect(),
            footprints: chunks.iter().map(|chunk| footprint_area(chunk)).collect(),
            carriers,
            options,
        })
    }

    /// The validated options.
    pub(crate) fn options(&self) -> &ChunkStressOptions {
        &self.options
    }

    /// Resolve load and capacity for every chunk, in generation order.
    ///
    /// `graph` and `health_fractions` use generation order; `support` is the evaluation for the same active set.
    /// Load ranks place grounded carriers before the chunks they carry, including bearings that support
    /// discovery reached later. Walking those ranks backward accumulates every dependent's mass before forwarding it.
    pub(crate) fn evaluate(
        &self,
        graph: &ChunkSupportGraph,
        health_fractions: &[f64],
        support: &SupportEvaluation,
    ) -> Vec<ChunkLoad> {
        let mut loads: Vec<ChunkLoad> = vec![
            ChunkLoad {
                load: 0.0,
                capacity: 0.0,
            };
            self.masses.len()
        ];
        let ranks = self.load_ranks(graph, support);
        let mut order: Vec<(usize, usize)> =
            ranks.iter().enumerate().filter_map(|(index, rank)| rank.map(|rank| (rank, index))).collect();
        // Highest rank first: every chunk is visited after everything it carries.
        order.sort_unstable_by_key(|(rank, _)| std::cmp::Reverse(*rank));

        for &(rank, index) in &order {
            loads[index].load += self.masses[index];
            let health = health_fractions[index].clamp(0.0, 1.0);
            let strength = self.strengths[index] * health;

            // Anchored chunks bear on the ground across their whole footprint.
            if graph.anchors()[index] {
                loads[index].capacity = self.footprints[index] * strength;
                continue;
            }

            // Bear on the chunks beneath when there are any; otherwise hang from side neighbors at reduced strength.
            let reached_earlier =
                |contact: &&CarrierContact| ranks[contact.carrier].is_some_and(|carrier_rank| carrier_rank < rank);
            let vertical: Vec<&CarrierContact> =
                self.carriers[index].iter().filter(reached_earlier).filter(|contact| !contact.lateral).collect();
            let (contacts, strength) = if vertical.is_empty() {
                let lateral: Vec<&CarrierContact> =
                    self.carriers[index].iter().filter(reached_earlier).filter(|contact| contact.lateral).collect();
                (lateral, strength * self.options.lateral_strength_fraction)
            } else {
                (vertical, strength)
            };

            let total_area: f64 = contacts.iter().map(|contact| contact.contact_area).sum();
            loads[index].capacity = total_area * strength;
            if total_area <= 0.0 {
                continue;
            }
            let load = loads[index].load;
            for contact in contacts {
                loads[contact.carrier].load += load * (contact.contact_area / total_area);
            }
        }
        loads
    }

    /// Stage for a load ratio under these options.
    pub(crate) fn stage(&self, ratio: f64) -> ChunkStressStage {
        if ratio >= OVERLOAD_RATIO {
            ChunkStressStage::Failing
        } else if ratio >= self.options.warning_ratio {
            ChunkStressStage::Strained
        } else {
            ChunkStressStage::Stable
        }
    }

    /// Order load paths from ground outward, waiting for all supported bearings before placing their dependents.
    fn load_ranks(&self, graph: &ChunkSupportGraph, support: &SupportEvaluation) -> Vec<Option<usize>> {
        let anchors = graph.anchors();
        let mut ranks = vec![None; self.masses.len()];
        let mut grounded = anchors.to_vec();
        let mut pending_bearings: Vec<usize> = self
            .carriers
            .iter()
            .map(|contacts| {
                contacts.iter().filter(|contact| !contact.lateral && support.ranks[contact.carrier].is_some()).count()
            })
            .collect();
        let mut candidates: Vec<usize> =
            support.ranks.iter().enumerate().filter_map(|(index, rank)| rank.map(|_| index)).collect();
        candidates.sort_unstable_by_key(|&index| (!anchors[index], support.ranks[index]));

        for rank in 0..candidates.len() {
            // A lateral-only chunk must already touch a grounded carrier. Vertical topological ordering alone can
            // put a hanging chunk before all its side contacts and strand its load.
            let ready = candidates.iter().copied().find(|&index| {
                ranks[index].is_none() && grounded[index] && (anchors[index] || pending_bearings[index] == 0)
            });
            // A mixed-height side/stack loop can make all remaining bearings wait on each other. Cut that loop at
            // its earliest grounded frontier, using only already ordered contacts there. The support search ensures
            // a frontier exists, and each iteration places one chunk: no iterative stress solver or load cycles.
            let next = ready
                .or_else(|| candidates.iter().copied().find(|&index| ranks[index].is_none() && grounded[index]))
                .expect("every supported chunk has a contact path from an active anchor");
            ranks[next] = Some(rank);
            for contact in &graph.edges()[next] {
                grounded[contact.target] = true;
                if !contact.lateral {
                    pending_bearings[contact.target] -= 1;
                }
            }
        }
        ranks
    }
}

/// Load over capacity, zero without load, and infinite when load meets no capacity.
pub(crate) fn load_ratio(load: &ChunkLoad) -> f64 {
    if load.load <= 0.0 {
        return 0.0;
    }
    if load.capacity <= 0.0 {
        return f64::INFINITY;
    }
    load.load / load.capacity
}

/// Area a chunk bears on the ground with: its box's footprint, or its shape's downward-facing area.
fn footprint_area(chunk: &DestructibleChunkDefinition) -> f64 {
    match &chunk.shape {
        Some(shape) => shape.footprint_area(),
        None => chunk.size[0] * chunk.size[2],
    }
}
