// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The PARTICLE LAB session: the working
//! document, the accepted preview effect, Live application with debounce, playback, incremental seeking, property
//! undo, recovery storage, and the performance monitor. The interface module draws it; nothing here uses egui.
//!
//! The working document and the accepted preview are kept apart: edits change the document and schedule an apply; an
//! apply compiles a candidate, starts a new effect, prepares its images, and only then replaces the preview. An invalid
//! draft (a script that does not compile, a rejected numeric field) never replaces the accepted preview, and export
//! refuses rejected numeric drafts.

use std::collections::{
    HashMap,
    HashSet,
};

use joy_engine::development::lab_values::REPAIR_DRAFTS_MESSAGE;
use joy_engine::development::{
    NumericDrafts,
    PropertyHistory,
    ScriptDiagnostic,
};
use joy_engine::math::Vec2;
use joy_engine::particles::{
    CompiledParticleEffect,
    PARTICLE_STEP_SECONDS,
    ParticleEffect,
    ParticleEffectAsset,
    ParticleEffectOptions,
    ParticleParameter,
    ParticleParameterValue,
    ParticleValueKind,
    compile_particle_effect,
    compile_particle_script,
    encode_particle_texture_source,
    read_particle_texture_source,
};
use joy_engine::runtime::{
    Stopwatch,
    Storage,
};

use crate::constants::{
    DEFAULT_SEED,
    EmitterPreset,
    LIVE_DELAY_SECONDS,
    MAXIMUM_FRAME_SECONDS,
    MAXIMUM_REPLAY_SECONDS,
    PERFORMANCE_REFRESH_SECONDS,
    PerformanceMode,
    SEEK_STEPS_PER_FRAME,
    STORAGE_KEY,
};
use crate::document::{
    self,
    PropertySnapshot,
    calculate_duration,
    property_snapshot,
    read_recovery_asset,
    write_asset,
};
use crate::examples::copy_example;
use crate::storage::{
    read_stored_text,
    store_text,
};
use crate::viewport::ParticleViewport;

/// Largest uploaded texture, in bytes.
pub const MAXIMUM_TEXTURE_UPLOAD_BYTES: usize = 1024 * 1024;

/// Seconds between recovery saves while editing.
const RECOVERY_SAVE_INTERVAL_SECONDS: f64 = 0.5;

/// The preview's relation to the working document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewState {
    /// The preview shows the working document.
    Live,
    /// Edits are waiting to be applied.
    Pending,
}

/// How much of the effect replays before looping or stopping.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ReplayWindow {
    /// Until the effect ends.
    Full,
    /// A fixed window in seconds (2, 5, or custom up to 30).
    Seconds(f64),
}

impl ReplayWindow {
    /// The window in seconds, or zero for the full effect.
    pub fn seconds(self) -> f64 {
        match self {
            Self::Full => 0.0,
            Self::Seconds(seconds) => seconds,
        }
    }
}

/// An incremental seek in progress.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Seek {
    remaining_steps: u64,
    resume_playback: bool,
}

/// Per-emitter profiling samples between monitor refreshes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct EmitterSample {
    spawn_ms: f64,
    update_ms: f64,
    spawn_sample: f64,
    update_sample: f64,
}

/// One row of the detailed performance table.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmitterPerformance {
    /// Emitter id.
    pub id: String,
    /// Live particles.
    pub live: u64,
    /// Spawn CPU per simulation step, in milliseconds.
    pub spawn_ms: f64,
    /// Update CPU per simulation step, in milliseconds.
    pub update_ms: f64,
    /// Geometry CPU in the last render, in milliseconds.
    pub geometry_ms: f64,
    /// Triangles in the last render.
    pub triangles: u64,
    /// Particle lights.
    pub lights: usize,
    /// Particles spawned since profiling began.
    pub spawned: u64,
    /// Particles dropped since profiling began.
    pub dropped: u64,
    /// Script operations since profiling began.
    pub operations: i64,
}

/// The monitor's displayed values, refreshed every quarter second.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PerformanceView {
    /// Smoothed preview frames per second, or `None` before the first sample.
    pub preview_fps: Option<f64>,
    /// Average simulation CPU per frame over the refresh window, in milliseconds.
    pub simulation_ms: f64,
    /// Per-emitter rows in detailed profiling.
    pub emitters: Vec<EmitterPerformance>,
}

/// The application session.
pub struct ParticleLab {
    // The working document and its unsaved-change tracking.
    asset: ParticleEffectAsset,
    revision: u64,
    saved_revision: u64,

    // The accepted preview.
    effect: Option<ParticleEffect>,
    /// The preview's camera, grid, textures, and statistics.
    pub viewport: ParticleViewport,

    // Editing state that never reaches the exported effect.
    /// Selected emitter index.
    pub selected_emitter: usize,
    /// Parameters shown collapsed.
    pub collapsed_parameters: HashSet<String>,
    /// Rejected numeric drafts in the inspector and parameter panels.
    pub drafts: NumericDrafts,
    history: PropertyHistory<PropertySnapshot>,

    // Preview controls.
    live: bool,
    /// Whether playback loops.
    pub looping: bool,
    playing: bool,
    /// Playback speed multiplier.
    pub speed: f64,
    seed: i64,
    replay: ReplayWindow,
    duration: f64,
    /// Whether the author is dragging the timeline.
    pub scrubbing: bool,

    // Scheduling.
    clock: f64,
    apply_due: Option<f64>,
    preview_state: PreviewState,
    seek: Option<Seek>,
    frame_next_effect: bool,
    pending_frame: bool,
    generation: u64,
    error: Option<String>,

    // Monitoring.
    performance_mode: PerformanceMode,
    performance: PerformanceView,
    fps: f64,
    simulation_ms: f64,
    telemetry_frames: u32,
    telemetry_clock: f64,
    profile_steps: u64,
    emitter_samples: HashMap<String, EmitterSample>,

    // Recovery.
    storage: Option<Storage>,
    recovery_dirty: bool,
    last_recovery_save: f64,
}

impl ParticleLab {
    /// A session restoring the recovery draft from `storage` (or the first example) and applying it. `storage` is
    /// `None` when hosted in Joy Editor, which owns files and recovery.
    pub fn new(storage: Option<Storage>) -> Self {
        let recovered = storage
            .as_ref()
            .and_then(|storage| read_stored_text(storage, STORAGE_KEY))
            .and_then(|text| read_recovery_asset(&text).ok());
        let asset = recovered.unwrap_or_else(|| copy_example(0));
        let mut lab = Self {
            history: PropertyHistory::new(property_snapshot(&asset)),
            duration: calculate_duration(&asset),
            asset,
            revision: 0,
            saved_revision: 0,
            effect: None,
            viewport: ParticleViewport::new(),
            selected_emitter: 0,
            collapsed_parameters: HashSet::new(),
            drafts: NumericDrafts::default(),
            live: true,
            looping: true,
            playing: true,
            speed: 1.0,
            seed: DEFAULT_SEED,
            replay: ReplayWindow::Full,
            scrubbing: false,
            clock: 0.0,
            apply_due: None,
            preview_state: PreviewState::Live,
            seek: None,
            frame_next_effect: true,
            pending_frame: false,
            generation: 0,
            error: None,
            performance_mode: PerformanceMode::Overview,
            performance: PerformanceView::default(),
            fps: 0.0,
            simulation_ms: 0.0,
            telemetry_frames: 0,
            telemetry_clock: 0.0,
            profile_steps: 0,
            emitter_samples: HashMap::new(),
            storage,
            recovery_dirty: false,
            last_recovery_save: 0.0,
        };
        lab.apply(false);
        lab
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Document
    // -----------------------------------------------------------------------------------------------------------------

    /// The working document.
    pub fn asset(&self) -> &ParticleEffectAsset {
        &self.asset
    }

    /// The working document for in-place edits; call [`document_changed`](Self::document_changed) (or
    /// [`property_committed`](Self::property_committed)) after changing it.
    pub fn asset_mut(&mut self) -> &mut ParticleEffectAsset {
        &mut self.asset
    }

    /// The selected emitter's index, clamped to the document.
    pub fn current_emitter(&self) -> usize {
        self.selected_emitter.min(self.asset.emitters.len().saturating_sub(1))
    }

    /// Record an edit: remember the draft for recovery and schedule a Live apply.
    pub fn document_changed(&mut self) {
        self.revision += 1;
        self.recovery_dirty = true;
        self.schedule_apply();
    }

    /// Record a committed property gesture (one undo entry), then [`document_changed`](Self::document_changed).
    pub fn property_committed(&mut self) {
        self.history.commit(property_snapshot(&self.asset));
        self.document_changed();
    }

    /// Record a structural edit that property undo does not cover (adding or removing parameters, emitters, or bursts).
    pub fn structure_changed(&mut self) {
        self.history.rebase(property_snapshot(&self.asset));
        self.document_changed();
    }

    /// Record a script edit; property undo cannot cross it.
    pub fn script_changed(&mut self) {
        self.history.reset(property_snapshot(&self.asset));
        self.document_changed();
    }

    /// Replace the document (opened file, example, or host document): new history, no drafts, first emitter, and a
    /// framing of the new effect.
    pub fn load_document(&mut self, asset: ParticleEffectAsset) {
        self.history.reset(property_snapshot(&asset));
        self.drafts.clear();
        self.asset = asset;
        self.collapsed_parameters.clear();
        self.selected_emitter = 0;
        self.frame_next_effect = true;
        self.generation += 1;
        self.duration = calculate_duration(&self.asset);
        self.revision += 1;
        self.saved_revision = self.revision;
        self.recovery_dirty = true;
        self.apply(false);
    }

    /// Load bundled example `index`, unless a rejected numeric draft must be repaired first.
    pub fn load_example(&mut self, index: usize) -> Result<(), String> {
        self.drafts.require_valid()?;
        self.load_document(copy_example(index));
        // Examples are new work, not the opened file.
        self.saved_revision = u64::MAX;
        Ok(())
    }

    /// Open portable `.joyfx` text: fully validated, or an error leaving the document in place.
    pub fn open_text(&mut self, text: &str) -> Result<(), String> {
        let asset = document::read_asset(text).inspect_err(|message| self.error = Some(message.clone()))?;
        self.load_document(asset);
        Ok(())
    }

    /// Export text for Save, refusing rejected numeric drafts.
    pub fn export_text(&mut self) -> Result<String, String> {
        self.drafts.require_valid().inspect_err(|message| self.error = Some(message.clone()))?;
        Ok(write_asset(&self.asset))
    }

    /// Counts document replacements (open, example, host), so editors start new histories.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The document and the drafts together, for panels that edit values in place.
    pub fn edit_parts(&mut self) -> (&mut ParticleEffectAsset, &mut NumericDrafts) {
        (&mut self.asset, &mut self.drafts)
    }

    /// Record a lab control's edit: any change schedules an apply; a committed gesture also records one undo entry.
    pub fn control_edited(&mut self, response: &joy_engine::development::ControlResponse) {
        self.property_edit(response.changed, response.committed());
    }

    /// Record property edits from several controls: `changed` schedules an apply; `committed` records one undo entry
    /// from the last commit point.
    pub fn property_edit(&mut self, changed: bool, committed: bool) {
        if changed {
            self.document_changed();
        }
        if committed {
            self.history.commit(property_snapshot(&self.asset));
        }
    }

    /// Whether the document changed since it was opened or saved.
    pub fn has_unsaved_changes(&self) -> bool {
        self.revision != self.saved_revision
    }

    /// Record that the current document was saved.
    pub fn mark_saved(&mut self) {
        self.saved_revision = self.revision;
    }

    /// Stop writing recovery drafts (hosted tabs, whose host owns files).
    pub fn set_storage(&mut self, storage: Option<Storage>) {
        self.storage = storage;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Property undo
    // -----------------------------------------------------------------------------------------------------------------

    /// Whether property undo has an entry.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether property redo has an entry.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undo the last property edit.
    pub fn undo(&mut self) {
        if self.drafts.has_rejected() {
            return;
        }
        if let Some(snapshot) = self.history.undo(&property_snapshot(&self.asset)) {
            document::restore_properties(&mut self.asset, &snapshot);
            self.document_changed();
        }
    }

    /// Redo the last undone property edit.
    pub fn redo(&mut self) {
        if self.drafts.has_rejected() {
            return;
        }
        if let Some(snapshot) = self.history.redo(&property_snapshot(&self.asset)) {
            document::restore_properties(&mut self.asset, &snapshot);
            self.document_changed();
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Structural edits
    // -----------------------------------------------------------------------------------------------------------------

    /// Select emitter `index`, unless a rejected draft must be repaired first.
    pub fn select_emitter(&mut self, index: usize) -> Result<(), String> {
        self.drafts.require_valid()?;
        self.selected_emitter = index.min(self.asset.emitters.len().saturating_sub(1));
        Ok(())
    }

    /// Insert an emitter from `preset` and select it (property history restarts).
    pub fn add_emitter(&mut self, preset: EmitterPreset) -> Result<(), String> {
        self.drafts.require_valid()?;
        self.selected_emitter = document::add_emitter(&mut self.asset, preset);
        self.history.reset(property_snapshot(&self.asset));
        self.document_changed();
        Ok(())
    }

    /// Duplicate the selected emitter as a root emitter and select the copy.
    pub fn duplicate_emitter(&mut self) -> Result<(), String> {
        self.drafts.require_valid()?;
        let current = self.current_emitter();
        if let Some(index) = document::duplicate_emitter(&mut self.asset, current) {
            self.selected_emitter = index;
            self.structure_changed();
        }
        Ok(())
    }

    /// Delete the selected emitter.
    pub fn delete_emitter(&mut self) -> Result<(), String> {
        self.drafts.require_valid()?;
        let current = self.current_emitter();
        match document::delete_emitter(&mut self.asset, current) {
            Ok(next) => {
                self.selected_emitter = next;
                self.structure_changed();
                Ok(())
            }
            Err(message) => {
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    /// Add a scalar parameter.
    pub fn add_parameter(&mut self) -> Result<(), String> {
        self.drafts.require_valid()?;
        document::add_parameter(&mut self.asset);
        self.structure_changed();
        Ok(())
    }

    /// Show an edit's refusal in the error panel.
    pub fn show_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
    }

    /// Upload a PNG (at most 1 MiB and 2048 × 2048 pixels) as the selected emitter's texture.
    pub fn upload_texture(&mut self, png: &[u8]) -> Result<(), String> {
        let result = (|| {
            if png.len() > MAXIMUM_TEXTURE_UPLOAD_BYTES {
                return Err("Choose a PNG texture up to 1 MiB.".to_owned());
            }
            let source = encode_particle_texture_source(png).map_err(|error| match error {
                joy_engine::particles::ParticleTextureError::InvalidDimensions => {
                    "Texture dimensions cannot exceed 2048 × 2048 pixels.".to_owned()
                }
                joy_engine::particles::ParticleTextureError::InvalidHeader => {
                    "Choose a PNG texture up to 1 MiB.".to_owned()
                }
                other => other.to_string(),
            })?;
            read_particle_texture_source(&source).map_err(|error| error.to_string())?;
            Ok(source)
        })();
        match result {
            Ok(source) => {
                let index = self.current_emitter();
                self.asset.emitters[index].texture = Some(document::default_texture(&source));
                self.structure_changed();
                Ok(())
            }
            Err(message) => {
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Applying
    // -----------------------------------------------------------------------------------------------------------------

    /// Whether Live applies edits automatically.
    pub fn live(&self) -> bool {
        self.live
    }

    /// Turn Live on or off. Turning it off cancels the queued automatic apply; turning it on applies pending edits
    /// after the usual delay.
    pub fn set_live(&mut self, live: bool) {
        self.live = live;
        self.apply_due = None;
        if live && self.preview_state == PreviewState::Pending {
            self.schedule_apply();
        }
    }

    /// The preview's relation to the document.
    pub fn preview_state(&self) -> PreviewState {
        self.preview_state
    }

    /// Whether an automatic apply is queued.
    pub fn apply_queued(&self) -> bool {
        self.apply_due.is_some()
    }

    /// The error shown in the compose panel, if any.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Mark edits pending and, with Live on, queue an apply after the debounce delay; newer edits restart the delay.
    pub fn schedule_apply(&mut self) {
        self.preview_state = PreviewState::Pending;
        self.apply_due = self.live.then_some(self.clock + LIVE_DELAY_SECONDS);
    }

    /// Apply the working document. Automatic applies obey Live; explicit Apply works regardless. Rejected numeric
    /// drafts block both. On failure the accepted preview and its images stay and the error is shown. Returns whether
    /// the preview was replaced.
    pub fn apply(&mut self, automatic: bool) -> bool {
        if automatic && !self.live {
            return false;
        }
        if self.drafts.has_rejected() {
            return false;
        }
        self.apply_due = None;
        self.seek = None;
        let candidate = match compile_particle_effect(&self.asset.to_json()) {
            Ok(candidate) => candidate,
            Err(error) => {
                self.error = Some(error.to_string());
                return false;
            }
        };
        let options = ParticleEffectOptions {
            seed: self.seed,
            parameters: parameter_values(candidate.asset()),
            profiling: self.performance_mode == PerformanceMode::Detailed,
            ..Default::default()
        };
        let mut effect =
            match ParticleEffect::new(&candidate, options).and_then(|mut effect| effect.step().map(|()| effect)) {
                Ok(effect) => effect,
                Err(error) => {
                    self.error = Some(error.to_string());
                    return false;
                }
            };
        if let Err(message) = self.viewport.prepare_effect(&candidate) {
            self.error = Some(message);
            return false;
        }

        effect.set_profiling(self.performance_mode == PerformanceMode::Detailed);
        self.simulation_ms = 0.0;
        self.telemetry_frames = 0;
        self.emitter_samples.clear();
        self.profile_steps = 0;
        self.selected_emitter = self.current_emitter();
        self.duration = calculate_duration(&self.asset);
        self.viewport.set_effect(Some(&candidate));
        self.effect = Some(effect);
        if std::mem::take(&mut self.frame_next_effect) {
            self.pending_frame = true;
        }
        self.error = None;
        self.preview_state = PreviewState::Live;
        self.recovery_dirty = true;
        self.save_recovery();
        true
    }

    /// The accepted preview effect.
    pub fn effect(&self) -> Option<&ParticleEffect> {
        self.effect.as_ref()
    }

    /// The viewport and the accepted effect together, for drawing.
    pub fn viewport_and_effect(&mut self) -> (&mut ParticleViewport, Option<&ParticleEffect>) {
        (&mut self.viewport, self.effect.as_ref())
    }

    /// The accepted definition.
    pub fn definition(&self) -> Option<&CompiledParticleEffect> {
        self.effect.as_ref().map(ParticleEffect::definition)
    }

    /// Diagnostics for the selected emitter's spawn and update scripts against the document's parameters, for the
    /// script editors.
    pub fn script_diagnostics(&self) -> (Vec<ScriptDiagnostic>, Vec<ScriptDiagnostic>) {
        let Some(emitter) = self.asset.emitters.get(self.current_emitter()) else {
            return (Vec::new(), Vec::new());
        };
        let inputs: Vec<String> =
            self.asset.parameters.iter().flat_map(|(name, parameter)| parameter.input_names(name)).collect();
        let input_names: Vec<&str> = inputs.iter().map(String::as_str).collect();
        let containers: Vec<(&str, ParticleValueKind)> = self
            .asset
            .parameters
            .iter()
            .filter_map(|(name, parameter)| match parameter {
                ParticleParameter::Vector3 { .. } => Some((name.as_str(), ParticleValueKind::Vector)),
                ParticleParameter::Color { .. } => Some((name.as_str(), ParticleValueKind::Color)),
                ParticleParameter::Scalar { .. } => None,
            })
            .collect();
        let check = |source: &str| match compile_particle_script(source, &input_names, &containers) {
            Ok(_) => Vec::new(),
            Err(error) => vec![ScriptDiagnostic::from(&error)],
        };
        (check(&emitter.spawn), check(&emitter.update))
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Playback
    // -----------------------------------------------------------------------------------------------------------------

    /// Whether playback runs.
    pub fn playing(&self) -> bool {
        self.playing
    }

    /// The preview seed.
    pub fn seed(&self) -> i64 {
        self.seed
    }

    /// Change the seed and apply.
    pub fn set_seed(&mut self, seed: i64) {
        self.seed = seed;
        self.apply(false);
    }

    /// The replay window.
    pub fn replay_window(&self) -> ReplayWindow {
        self.replay
    }

    /// Change the replay window (custom windows are 0–30 seconds, exclusive of zero) and restart.
    pub fn set_replay_window(&mut self, window: ReplayWindow) -> Result<(), String> {
        let seconds = window.seconds();
        if !seconds.is_finite()
            || !(0.0..=MAXIMUM_REPLAY_SECONDS).contains(&seconds)
            || (matches!(window, ReplayWindow::Seconds(_)) && seconds <= 0.0)
        {
            return Err("Enter a duration greater than 0 and at most 30 seconds.".to_owned());
        }
        self.replay = window;
        self.restart();
        Ok(())
    }

    /// The timeline length: the replay window or the full horizon.
    pub fn timeline_seconds(&self) -> f64 {
        let window = self.replay.seconds();
        if window > 0.0 { window } else { self.duration }
    }

    /// Current preview time in seconds.
    pub fn time(&self) -> f64 {
        self.effect.as_ref().map_or(0.0, ParticleEffect::time)
    }

    /// Restart playback from time zero.
    pub fn restart(&mut self) {
        self.seek = None;
        if let Some(effect) = &mut self.effect
            && let Err(error) = effect.reset()
        {
            self.error = Some(error.to_string());
        }
        self.emitter_samples.clear();
        self.profile_steps = 0;
        self.playing = true;
    }

    /// Pause and advance one fixed 60 Hz step.
    pub fn step(&mut self) {
        self.seek = None;
        self.playing = false;
        if let Some(effect) = &mut self.effect
            && let Err(error) = effect.step()
        {
            self.error = Some(error.to_string());
        }
    }

    /// Toggle playback.
    pub fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    /// Seek to `seconds` (0–30) incrementally: playback pauses, the effect resets, and the seek runs up to 120 steps per
    /// frame, resuming playback when it finishes unless a newer seek, apply, restart, or step superseded it.
    pub fn seek(&mut self, seconds: f64) {
        let Some(effect) = &mut self.effect else {
            return;
        };
        let resume_playback = self.playing;
        self.playing = false;
        if let Err(error) = effect.reset() {
            self.error = Some(error.to_string());
            return;
        }
        self.emitter_samples.clear();
        self.profile_steps = 0;
        let remaining_steps = (seconds.clamp(0.0, MAXIMUM_REPLAY_SECONDS) / PARTICLE_STEP_SECONDS).round() as u64;
        self.seek = Some(Seek { remaining_steps, resume_playback });
        self.advance_seek();
    }

    /// Whether a seek is still running.
    pub fn seeking(&self) -> bool {
        self.seek.is_some()
    }

    fn advance_seek(&mut self) {
        let (Some(seek), Some(effect)) = (&mut self.seek, &mut self.effect) else {
            return;
        };
        let steps = seek.remaining_steps.min(u64::from(SEEK_STEPS_PER_FRAME));
        for _ in 0..steps {
            if let Err(error) = effect.step() {
                self.error = Some(error.to_string());
                seek.remaining_steps = 0;
                break;
            }
        }
        seek.remaining_steps -= steps.min(seek.remaining_steps);
        if seek.remaining_steps == 0 {
            self.playing = seek.resume_playback;
            self.seek = None;
        }
    }

    /// Advance the session by one frame of `elapsed_seconds` real time: seek work, playback with loop and replay
    /// window, queued Live applies, recovery saves, and monitor sampling.
    pub fn frame(&mut self, elapsed_seconds: f64) {
        let elapsed = elapsed_seconds.max(0.0);
        self.clock += elapsed;
        // A refusal for rejected drafts lasts only until the drafts are repaired.
        if self.error.as_deref() == Some(REPAIR_DRAFTS_MESSAGE) && !self.drafts.has_rejected() {
            self.error = None;
        }
        let monitoring = self.performance_mode != PerformanceMode::Disabled;
        if monitoring && elapsed > 0.0 {
            let fps = 1.0 / elapsed;
            self.fps = if self.fps > 0.0 { (self.fps * 0.9) + (fps * 0.1) } else { fps };
        }
        if self.seek.is_some() {
            self.advance_seek();
        } else if self.playing && !self.scrubbing && self.effect.is_some() {
            let watch = Stopwatch::start();
            self.simulate(elapsed);
            if monitoring {
                self.simulation_ms += watch.elapsed_milliseconds();
            }
        }
        if self.apply_due.is_some_and(|due| self.clock >= due) {
            self.apply(true);
        }
        if self.recovery_dirty && self.clock - self.last_recovery_save >= RECOVERY_SAVE_INTERVAL_SECONDS {
            self.save_recovery();
        }
        if monitoring {
            self.telemetry_frames += 1;
            if self.clock - self.telemetry_clock >= PERFORMANCE_REFRESH_SECONDS {
                self.refresh_performance();
                self.telemetry_clock = self.clock;
            }
        }
    }

    /// Run playback for one frame, with the loop and replay window rules.
    fn simulate(&mut self, elapsed: f64) {
        let Some(effect) = &mut self.effect else {
            return;
        };
        let mut remaining = elapsed.min(MAXIMUM_FRAME_SECONDS) * self.speed;
        let result = (|| {
            while remaining > 0.0 {
                let slice = remaining.min(MAXIMUM_FRAME_SECONDS);
                effect.update(slice)?;
                remaining -= slice;
            }
            let window = self.replay.seconds();
            let reached_window = window > 0.0 && effect.time() >= window;
            if reached_window && !self.looping {
                self.playing = false;
            }
            if self.looping && (if window > 0.0 { reached_window } else { !effect.is_alive() }) {
                effect.reset()?;
                self.emitter_samples.clear();
                self.profile_steps = 0;
            }
            Ok::<(), joy_engine::particles::ParticleEffectError>(())
        })();
        if let Err(error) = result {
            self.playing = false;
            self.error = Some(error.to_string());
        }
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Monitoring
    // -----------------------------------------------------------------------------------------------------------------

    /// The monitor mode.
    pub fn performance_mode(&self) -> PerformanceMode {
        self.performance_mode
    }

    /// Change the monitor mode; detailed profiling starts fresh totals without restarting the simulation.
    pub fn set_performance_mode(&mut self, mode: PerformanceMode) {
        if self.performance_mode == mode {
            return;
        }
        self.performance_mode = mode;
        self.simulation_ms = 0.0;
        self.telemetry_frames = 0;
        self.telemetry_clock = self.clock;
        self.fps = 0.0;
        self.emitter_samples.clear();
        self.profile_steps = 0;
        self.performance.emitters.clear();
        if let Some(effect) = &mut self.effect {
            effect.set_profiling(mode == PerformanceMode::Detailed);
        }
        let definition = self.effect.as_ref().map(|effect| effect.definition().clone());
        self.viewport.set_performance_mode(mode, definition.as_ref());
        self.refresh_performance();
    }

    /// The monitor's displayed values.
    pub fn performance(&self) -> &PerformanceView {
        &self.performance
    }

    fn refresh_performance(&mut self) {
        if self.performance_mode == PerformanceMode::Disabled {
            return;
        }
        let frames = f64::from(self.telemetry_frames.max(1));
        self.performance.preview_fps = (self.fps > 0.0).then_some(self.fps);
        self.performance.simulation_ms = self.simulation_ms / frames;
        if self.performance_mode == PerformanceMode::Detailed
            && let Some(effect) = &self.effect
            && effect.profile().enabled
        {
            let profile = effect.profile();
            let stats = self.viewport.stats();
            let reset = profile.steps < self.profile_steps;
            let steps = (profile.steps - if reset { 0 } else { self.profile_steps }).max(1) as f64;
            let paused = !self.playing && profile.steps == self.profile_steps;
            let mut rows = Vec::with_capacity(profile.emitters.len());
            for emitter in &profile.emitters {
                let previous = if reset { None } else { self.emitter_samples.get(&emitter.id).copied() };
                let spawn = if paused {
                    previous.map_or(0.0, |sample| sample.spawn_sample)
                } else {
                    (emitter.spawn_ms - previous.map_or(0.0, |sample| sample.spawn_ms)).max(0.0) / steps
                };
                let update = if paused {
                    previous.map_or(0.0, |sample| sample.update_sample)
                } else {
                    (emitter.update_ms - previous.map_or(0.0, |sample| sample.update_ms)).max(0.0) / steps
                };
                self.emitter_samples.insert(
                    emitter.id.clone(),
                    EmitterSample {
                        spawn_ms: emitter.spawn_ms,
                        update_ms: emitter.update_ms,
                        spawn_sample: spawn,
                        update_sample: update,
                    },
                );
                let geometry = stats.by_emitter.iter().find(|row| row.id == emitter.id);
                rows.push(EmitterPerformance {
                    id: emitter.id.clone(),
                    live: emitter.live,
                    spawn_ms: spawn,
                    update_ms: update,
                    geometry_ms: geometry.map_or(0.0, |row| row.geometry_ms),
                    triangles: geometry.map_or(0, |row| row.triangles),
                    lights: geometry.map_or(0, |row| row.lights),
                    spawned: emitter.spawned,
                    dropped: emitter.dropped,
                    operations: emitter.operations,
                });
            }
            self.performance.emitters = rows;
            self.profile_steps = profile.steps;
        }
        self.simulation_ms = 0.0;
        self.telemetry_frames = 0;
    }

    // -----------------------------------------------------------------------------------------------------------------
    // Framing and recovery
    // -----------------------------------------------------------------------------------------------------------------

    /// Fit the camera to the current particles for a preview of `region_size` (the F shortcut).
    pub fn frame_current_effect(&mut self, region_size: Vec2) {
        self.viewport.frame_effect(self.effect.as_ref(), region_size, false);
    }

    /// Frame the new effect once, after an apply that loaded a document; the interface calls this with the preview
    /// region's size.
    pub fn frame_pending_effect(&mut self, region_size: Vec2) {
        if std::mem::take(&mut self.pending_frame) {
            self.viewport.frame_effect(self.effect.as_ref(), region_size, true);
        }
    }

    /// Save the working draft for recovery (never while hosted).
    fn save_recovery(&mut self) {
        self.recovery_dirty = false;
        self.last_recovery_save = self.clock;
        if let Some(storage) = &self.storage {
            store_text(storage, STORAGE_KEY, &write_asset(&self.asset));
        }
    }
}

/// The parameter values an effect instance starts with: each parameter's authored value.
pub fn parameter_values(asset: &ParticleEffectAsset) -> Vec<(String, ParticleParameterValue)> {
    asset.parameters.iter().map(|(name, parameter)| (name.clone(), parameter.default_value())).collect()
}
