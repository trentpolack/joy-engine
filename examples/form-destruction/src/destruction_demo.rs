// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The FORM destruction demo: one seeded chunk structure with cumulative damage, ground support, and load-path
//! stress on craterable ground, persistent rigid debris, impact markers, and the integrity, support, stress, and
//! material views.
//!
//! With the STRIKE tool, a click projects a ray from the camera onto the nearest active chunk and queues radial damage;
//! with the CHARGE tool, it blasts a crater into the ground under the aimed point, undermining the chunks that stood
//! there. The next simulation step commits either, so geometry, colliders, debris, and the HUD change together even
//! when nothing breaks. Weakened or undermined chunks that overload pulse and shake through their failure delay, then
//! fail and drop what they carried, so collapse can progress over a few seconds. Drags orbit instead of striking, the
//! wheel zooms, C switches tools, and R resets the structure and the ground and clears debris.

use std::collections::HashSet;

use joy_engine::physics::{
    ChunkDamageState,
    ChunkStressState,
    ChunkStructure,
    ChunkSupportState,
    RadialDamage,
};
use joy_engine::prelude::*;
use joy_engine::rendering::screen_ray;
use joy_engine::runtime::Stopwatch;

use super::config::destruction_config::{
    CHARGE,
    DESTRUCTION,
    INTEGRITY,
    PRESENTATION,
    STRESS,
    SUPPORT,
    StructureView,
    Tool,
    VIEW,
};
use super::destruction_debris::{
    DebrisSnapshot,
    DestructionDebris,
};
use super::ground::CraterGround;
use super::hud::{
    self,
    HudCommand,
    HudSettings,
    HudState,
};
use super::impact_debug::ImpactDebugDraw;
use super::structure::generate_structure;
use super::structure_view::create_structure_mesh_data;

/// Input action that strikes on click and orbits on drag (primary mouse button or a touch).
pub const IMPACT_ACTION: &str = "impact";
/// Input action that resets the structure.
pub const RESET_ACTION: &str = "reset";
/// Input action that switches between the STRIKE and CHARGE tools.
pub const TOOL_ACTION: &str = "tool";
/// Input action that closes CONFIG and cancels a gesture.
pub const CANCEL_ACTION: &str = "cancel";

/// The demo's input: click or drag with the primary button (or a touch), C to switch tools, R to reset, and Escape to
/// cancel.
pub fn input_map() -> InputMap {
    InputMap::new()
        .action(IMPACT_ACTION, [InputBinding::MouseButton(0)])
        .action(RESET_ACTION, [InputBinding::key(key::KEY_R)])
        .action(TOOL_ACTION, [InputBinding::key(key::KEY_C)])
        .action(CANCEL_ACTION, [InputBinding::key(key::ESCAPE)])
}

/// A press on the canvas, in logical pixels: where it began, the last orbit position, and whether it became a drag.
#[derive(Clone, Copy, Debug)]
struct PointerGesture {
    start: Vec2,
    last: Vec2,
    dragged: bool,
}

/// The last aimed impact: the chunk hit and the world-space point in meters.
#[derive(Clone, Debug, PartialEq)]
pub struct Impact {
    /// Id of the chunk the ray entered.
    pub chunk_id: String,
    /// World-space entry point.
    pub point: DVec3,
}

/// Copied diagnostics; positions are meters and timings milliseconds.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
pub struct DemoSnapshot {
    /// Chunks still part of the structure.
    pub active_chunks: usize,
    /// Debris bodies and static colliders.
    pub debris: DebrisSnapshot,
    /// Whether impact markers draw, and how many are alive.
    pub impact_debug: (bool, usize),
    /// Current structure view.
    pub view: StructureView,
    /// Committed per-chunk resistance.
    pub damage: Vec<ChunkDamageState>,
    /// Committed per-chunk support paths.
    pub support: Vec<ChunkSupportState>,
    /// Committed per-chunk load paths, with failure countdowns.
    pub stress: Vec<ChunkStressState>,
    /// Chunks released by support loss, in release order.
    pub detached_chunks: Vec<String>,
    /// Chunks that failed under load once their failure delay ran out, in failure order.
    pub overloaded_chunks: Vec<String>,
    /// Craters blasted into the ground since the last reset.
    pub craters: usize,
    /// Current tool.
    pub tool: Tool,
    /// The last aimed impact, or `None` after a miss or reset.
    pub last_impact: Option<Impact>,
    /// The notice line.
    pub notice: String,
    /// Mean CPU frame encoding, destruction update, and impact query times.
    pub timings: [f64; 3],
    /// Frames rendered.
    pub frames: u64,
}

/// The demo: owns the scene, structure, debris, markers, camera, and HUD state.
pub struct FormDestructionDemo {
    // Scene resources; the structure mesh is rebuilt from committed state.
    scene: RenderScene,
    structure: ChunkStructure,
    structure_mesh: Option<MeshInstanceId>,
    ground: CraterGround,
    debris: DestructionDebris,
    impact_debug: ImpactDebugDraw,
    camera: OrbitCamera,

    // Input and local tuning.
    gesture: Option<PointerGesture>,
    damage_radius: f64,
    damage_per_hit: f64,
    last_impact: Option<Impact>,
    damage_pending: bool,
    view: StructureView,
    tool: Tool,
    config_open: bool,
    detached_chunks: Vec<String>,
    overloaded_chunks: Vec<String>,
    reset_history: bool,

    // Presentation clock for the stress telegraph, in seconds; it never affects simulation.
    presentation_seconds: f64,

    // Bounded CPU timing samples in milliseconds; never GPU execution time.
    frame_count: u64,
    render_samples: Vec<f64>,
    damage_samples: Vec<f64>,
    query_samples: Vec<f64>,

    hud: HudState,
}

impl FormDestructionDemo {
    /// The seeded structure in the integrity view, with stress, static colliders, flat ground, and no debris.
    pub fn new() -> AppResult<Self> {
        let mut scene = RenderScene::new();
        let structure = ChunkStructure::new(&generate_structure(DESTRUCTION.seed), Some(SUPPORT.options()))?
            .with_stress(STRESS.options())?;
        let mut debris = DestructionDebris::new(&mut scene)?;
        debris.synchronize_structure(structure.active_chunks())?;
        let camera = OrbitCamera {
            target: VIEW.target,
            distance: VIEW.distance,
            radius: VIEW.radius,
            yaw: VIEW.yaw,
            pitch: VIEW.pitch,
            near_clip: Some(VIEW.near_clip),
            far_clip: Some(VIEW.far_clip),
            ..OrbitCamera::default()
        };
        let hud = HudState {
            backend: "starting".to_owned(),
            seed: DESTRUCTION.seed,
            notice: "Click the structure to strike · C switches to CHARGE".to_owned(),
            ..HudState::default()
        };
        let mut demo = Self {
            scene,
            structure,
            structure_mesh: None,
            ground: CraterGround::new(),
            debris,
            impact_debug: ImpactDebugDraw::new(),
            camera,
            gesture: None,
            damage_radius: DESTRUCTION.damage_radius,
            damage_per_hit: DESTRUCTION.damage_per_hit,
            last_impact: None,
            damage_pending: false,
            view: if INTEGRITY.enabled { StructureView::Integrity } else { StructureView::Material },
            tool: Tool::Strike,
            config_open: false,
            detached_chunks: Vec::new(),
            overloaded_chunks: Vec::new(),
            reset_history: false,
            presentation_seconds: 0.0,
            frame_count: 0,
            render_samples: Vec::new(),
            damage_samples: Vec::new(),
            query_samples: Vec::new(),
            hud,
        };
        demo.rebuild_structure_geometry()?;
        demo.refresh_hud();
        Ok(demo)
    }

    /// Restore chunk health, the flat ground, and geometry, discard queued hits, and release all debris and markers.
    pub fn reset(&mut self) -> AppResult {
        // The ground goes first so the structure re-anchors on flat ground as it resets.
        self.ground.clear();
        self.debris.set_ground(&mut self.scene, &self.ground)?;
        let ground = &self.ground;
        self.structure.set_ground_surface(CHARGE.bearing_tolerance, |x, z| ground.height_at(x, z))?;
        self.structure.reset();
        self.debris.reset(&mut self.scene);
        self.debris.synchronize_structure(self.structure.active_chunks())?;
        self.detached_chunks.clear();
        self.overloaded_chunks.clear();
        self.impact_debug.clear(&mut self.scene);
        self.last_impact = None;
        self.damage_pending = false;
        self.rebuild_structure_geometry()?;
        self.reset_history = true;
        self.refresh_hud();
        self.hud.notice = "Structure reset".to_owned();
        Ok(())
    }

    /// Apply the current tool through a logical viewport point: STRIKE damages the nearest active chunk, CHARGE
    /// craters the ground under the aimed point. The next simulation step commits either.
    pub fn use_tool_at(&mut self, point: Vec2, viewport: Viewport) -> AppResult {
        match self.tool {
            Tool::Strike => self.impact_at(point, viewport),
            Tool::Charge => self.charge_at(point, viewport),
        }
    }

    /// Aim through a logical viewport point onto the nearest active chunk and queue damage there; the next simulation
    /// step commits it. Points over empty space change nothing but the notice.
    pub fn impact_at(&mut self, point: Vec2, viewport: Viewport) -> AppResult {
        let frame = self.camera.camera_frame(viewport.aspect());
        let Some(ray) = screen_ray(point, frame.view_projection(), viewport.logical_size) else {
            return Ok(());
        };
        let origin = ray.origin.as_dvec3();
        let direction = ray.direction.as_dvec3();
        let stopwatch = Stopwatch::start();
        let hit = self.structure.raycast(origin, direction, DESTRUCTION.maximum_aim_distance)?;
        retain(&mut self.query_samples, stopwatch.elapsed_milliseconds());
        let Some(hit) = hit else {
            self.last_impact = None;
            self.hud.notice = "No structure at this point".to_owned();
            return Ok(());
        };
        let impact = Impact {
            chunk_id: hit.chunk.id.clone(),
            point: hit.point,
        };
        self.impact_debug.add(&mut self.scene, impact.point, ray.direction, self.damage_radius as f32)?;
        self.structure.queue_damage(RadialDamage {
            center: impact.point,
            radius: self.damage_radius,
            damage: self.damage_per_hit,
        })?;
        self.last_impact = Some(impact);
        self.damage_pending = true;
        Ok(())
    }

    /// Aim through a logical viewport point and blast a crater into the ground beneath it: under the aimed chunk when
    /// the ray hits the structure, otherwise where it meets the ground. The ground's collider and mesh change at once;
    /// the structure re-anchors and its support and stress change at the next simulation step.
    pub fn charge_at(&mut self, point: Vec2, viewport: Viewport) -> AppResult {
        let frame = self.camera.camera_frame(viewport.aspect());
        let Some(ray) = screen_ray(point, frame.view_projection(), viewport.logical_size) else {
            return Ok(());
        };
        let origin = ray.origin.as_dvec3();
        let direction = ray.direction.as_dvec3();
        let structure_hit = self.structure.raycast(origin, direction, DESTRUCTION.maximum_aim_distance)?;
        let aimed = match structure_hit {
            Some(hit) => Some(hit.point),
            None => self.ground.raycast(origin, direction, DESTRUCTION.maximum_aim_distance),
        };
        let Some(aimed) = aimed else {
            self.hud.notice = "Aim the charge at the ground or the structure".to_owned();
            return Ok(());
        };

        let crater = self.ground.add_charge(aimed.x, aimed.z);
        self.debris.set_ground(&mut self.scene, &self.ground)?;
        let ground = &self.ground;
        self.structure.set_ground_surface(CHARGE.bearing_tolerance, |x, z| ground.height_at(x, z))?;
        let [x, z] = crater.center;
        let marker = DVec3::new(x, GROUND_MARKER_HEIGHT, z);
        self.impact_debug.add(&mut self.scene, marker, ray.direction, CHARGE.radius as f32)?;
        self.last_impact = None;
        self.damage_pending = true;
        self.hud.notice = format!("Charge blasted a {:.1} m crater", CHARGE.radius * 2.0);
        Ok(())
    }

    /// Switch what a click does.
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.hud.notice = match tool {
            Tool::Strike => "STRIKE: click a chunk to damage it".to_owned(),
            Tool::Charge => "CHARGE: click the ground or a wall base to undermine it".to_owned(),
        };
    }

    /// Advance the stress telegraph and overloaded chunks' failure delays by `elapsed_seconds`, then commit anything
    /// queued or due. The interactive loop calls this every simulation step; tests call it to skip ahead.
    pub fn advance(&mut self, elapsed_seconds: f64) -> AppResult {
        self.presentation_seconds += elapsed_seconds;
        if self.structure.advance_stress(elapsed_seconds)? {
            self.damage_pending = true;
        }
        self.commit_damage()?;

        // Failing chunks pulse and shake every frame until they fail or are relieved.
        if self.structure.failing_chunk_count() > 0 {
            self.rebuild_structure_geometry()?;
        }
        Ok(())
    }

    /// Show the structure in `view`, rebuilding its colors from committed state.
    pub fn set_view(&mut self, view: StructureView) -> AppResult {
        self.view = view;
        self.rebuild_structure_geometry()?;
        self.reset_history = true;
        Ok(())
    }

    /// Set the impact radius (meters) and damage per impact used by later clicks.
    pub fn set_damage(&mut self, radius: f64, damage: f64) {
        self.damage_radius = radius;
        self.damage_per_hit = damage;
        self.refresh_hud();
    }

    /// Show or hide impact markers; hiding releases existing markers.
    pub fn set_impact_debug(&mut self, enabled: bool) {
        self.impact_debug.set_enabled(&mut self.scene, enabled);
    }

    /// Copy diagnostics.
    #[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
    pub fn snapshot(&self) -> DemoSnapshot {
        DemoSnapshot {
            active_chunks: self.structure.active_chunk_count(),
            debris: self.debris.snapshot(),
            impact_debug: (self.impact_debug.enabled(), self.impact_debug.count()),
            view: self.view,
            damage: self.structure.damage_state(),
            support: self.structure.support_state(),
            stress: self.structure.stress_state(),
            detached_chunks: self.detached_chunks.clone(),
            overloaded_chunks: self.overloaded_chunks.clone(),
            craters: self.ground.craters().len(),
            tool: self.tool,
            last_impact: self.last_impact.clone(),
            notice: self.hud.notice.clone(),
            timings: [mean(&self.render_samples), mean(&self.damage_samples), mean(&self.query_samples)],
            frames: self.frame_count,
        }
    }

    /// Commit queued damage, undermining, and due overload failures at the simulation boundary: remove broken,
    /// overloaded, and unsupported chunks from the structure and its colliders, release them as debris, and refresh
    /// geometry and the HUD.
    fn commit_damage(&mut self) -> AppResult {
        if !std::mem::take(&mut self.damage_pending) {
            return Ok(());
        }
        let stopwatch = Stopwatch::start();
        let changes = self.structure.commit();
        let released: HashSet<&str> =
            changes.detached.iter().chain(&changes.overloaded).map(|chunk| chunk.id.as_str()).collect();
        for chunk in &changes.detached {
            if !self.detached_chunks.contains(&chunk.id) {
                self.detached_chunks.push(chunk.id.clone());
            }
        }
        for chunk in &changes.overloaded {
            if !self.overloaded_chunks.contains(&chunk.id) {
                self.overloaded_chunks.push(chunk.id.clone());
            }
        }
        // Static colliders leave before their replacement dynamic bodies arrive. Impact breaks launch pieces,
        // overloaded chunks lean out of the wall as they give way, and support failures drop from rest.
        self.debris.synchronize_structure(self.structure.active_chunks())?;
        let broken = changes.removed.iter().filter(|chunk| !released.contains(chunk.id.as_str()));
        self.debris.spawn(&mut self.scene, broken, true)?;
        self.debris.spawn_crumbled(&mut self.scene, &changes.overloaded)?;
        self.debris.spawn(&mut self.scene, &changes.detached, false)?;
        self.rebuild_structure_geometry()?;
        self.reset_history = true;
        retain(&mut self.damage_samples, stopwatch.elapsed_milliseconds());
        self.refresh_hud();

        let failing = self.structure.failing_chunk_count();
        self.hud.notice = if !changes.removed.is_empty() {
            if changes.active_chunk_count > 0 {
                let broken = changes.removed.len() - released.len();
                let mut notice = format!("{broken} broken · {} lost support", changes.detached.len());
                if !changes.overloaded.is_empty() {
                    notice.push_str(&format!(" · {} failed under load", changes.overloaded.len()));
                }
                if failing > 0 {
                    notice.push_str(&format!(" · {failing} more overloaded"));
                }
                notice
            } else {
                "Structure cleared · reset to rebuild".to_owned()
            }
        } else if failing > 0 {
            format!(
                "{failing} {} overloaded · failing in {:.1} s",
                if failing == 1 { "chunk" } else { "chunks" },
                STRESS.failure_delay_seconds
            )
        } else {
            match self.target_damage() {
                Some(target) => {
                    let hits = (target.remaining_hit_points / self.damage_per_hit).ceil();
                    format!(
                        "Wall resisted · {hits} more {} here to break",
                        if hits == 1.0 { "impact" } else { "impacts" }
                    )
                }
                None => "Impact applied".to_owned(),
            }
        };
        Ok(())
    }

    fn rebuild_structure_geometry(&mut self) -> AppResult {
        if let Some(previous) = self.structure_mesh.take() {
            self.scene.remove(previous);
        }
        if self.structure.active_chunk_count() == 0 {
            return Ok(());
        }
        let mesh = create_structure_mesh_data(&self.structure, self.view, self.presentation_seconds);
        let asset = MeshAsset::new(&mesh)?;
        self.structure_mesh = Some(self.scene.insert(MeshInstance::new(asset)));
        Ok(())
    }

    /// Committed damage of the last impacted chunk.
    fn target_damage(&self) -> Option<ChunkDamageState> {
        let impact = self.last_impact.as_ref()?;
        self.structure.damage_state().into_iter().find(|state| state.chunk_id == impact.chunk_id)
    }

    fn refresh_hud(&mut self) {
        let hits = |hit_points: f64| (hit_points / self.damage_per_hit).ceil();
        self.hud.chunks = format!("{} active", self.structure.active_chunk_count());
        self.hud.failing = match self.structure.failing_chunk_count() {
            0 => "none".to_owned(),
            count => format!("{count} failing"),
        };
        self.hud.tool = self.tool;
        self.hud.debris = format!("{} retained", self.debris.count());
        self.hud.baseline = format!(
            "Solid wall: {} hits · Frame: {} hits",
            hits(DESTRUCTION.hit_points),
            hits(DESTRUCTION.frame_hit_points)
        );

        let target = self.target_damage();
        self.hud.target = match &target {
            Some(target) if target.active => {
                let left = hits(target.remaining_hit_points);
                format!(
                    "{:.1} / {} resistance · {left} {} left",
                    target.remaining_hit_points,
                    target.maximum_hit_points,
                    if left == 1.0 { "hit" } else { "hits" }
                )
            }
            Some(_) => "Chunk released".to_owned(),
            None => "Click a chunk to inspect its resistance".to_owned(),
        };
        let maximum = target.as_ref().map_or(DESTRUCTION.hit_points, |target| target.maximum_hit_points);
        let value = match &target {
            Some(target) if !target.active => 0.0,
            Some(target) => target.remaining_hit_points,
            None => maximum,
        };
        self.hud.meter = (value as f32, maximum as f32);

        let stress = self
            .last_impact
            .as_ref()
            .and_then(|impact| self.structure.stress_states().iter().find(|state| state.chunk_id == impact.chunk_id));
        self.hud.stress = match stress {
            None => format!(
                "Load paths · strained at {:.0}% · fails {:.1} s after overload",
                STRESS.warning_ratio * 100.0,
                STRESS.failure_delay_seconds
            ),
            Some(state) if state.capacity <= 0.0 => "Load: no longer carrying anything".to_owned(),
            Some(state) => {
                let mut line = format!(
                    "Load {:.1} t of {:.1} t capacity · {:.0}%",
                    state.load / KILOGRAMS_PER_TONNE,
                    state.capacity / KILOGRAMS_PER_TONNE,
                    state.ratio * 100.0
                );
                if let Some(remaining) = state.seconds_until_failure {
                    line.push_str(&format!(" · fails in {remaining:.1} s"));
                }
                line
            }
        };

        let support = self.last_impact.as_ref().and_then(|impact| {
            self.structure.support_state().into_iter().find(|state| state.chunk_id == impact.chunk_id)
        });
        self.hud.support = match support {
            None => format!("Ground support · at most {:.2} m sideways", SUPPORT.maximum_cantilever_distance),
            Some(state) if !state.supported => {
                if self.detached_chunks.contains(&state.chunk_id) {
                    "Support lost · detached with health remaining".to_owned()
                } else if self.overloaded_chunks.contains(&state.chunk_id) {
                    "Failed under load · crumbled with health remaining".to_owned()
                } else {
                    "Broken by impact · no longer part of the wall".to_owned()
                }
            }
            Some(state) if state.anchored => "Support: anchored to the ground".to_owned(),
            Some(state) => match state.lateral_distance {
                Some(distance) if distance > 0.0 => format!("Support: braced {distance:.2} m sideways"),
                _ => "Support: vertical path to the ground".to_owned(),
            },
        };
    }

    fn apply(&mut self, command: HudCommand) -> AppResult {
        match command {
            HudCommand::ToggleIntegrity => {
                let view = if self.view == StructureView::Integrity {
                    StructureView::Material
                } else {
                    StructureView::Integrity
                };
                self.set_view(view)?;
            }
            HudCommand::ToggleSupport => {
                let view =
                    if self.view == StructureView::Support { StructureView::Material } else { StructureView::Support };
                self.set_view(view)?;
            }
            HudCommand::ToggleStress => {
                let view =
                    if self.view == StructureView::Stress { StructureView::Material } else { StructureView::Stress };
                self.set_view(view)?;
            }
            HudCommand::SelectTool(tool) => self.set_tool(tool),
            HudCommand::ToggleConfig => self.config_open = !self.config_open,
            HudCommand::CloseConfig => self.config_open = false,
            HudCommand::Radius(radius) => self.damage_radius = radius,
            HudCommand::Damage(damage) => self.set_damage(self.damage_radius, damage),
            HudCommand::ImpactDebug(enabled) => self.set_impact_debug(enabled),
            HudCommand::Reset => self.reset()?,
        }
        Ok(())
    }

    /// Track one press: small movements stay a click, larger ones orbit, and a release without dragging strikes.
    fn handle_pointer(&mut self, frame: &mut Frame<'_>) -> AppResult {
        let action = frame.input.action(IMPACT_ACTION);
        let pointer = frame.input.pointer();
        if action.pressed && self.gesture.is_none() && pointer.active {
            let position = pointer.position;
            self.gesture = Some(PointerGesture {
                start: position,
                last: position,
                dragged: false,
            });
        }
        let Some(mut gesture) = self.gesture else {
            return Ok(());
        };
        let travel = pointer.position.distance(gesture.start);
        if action.down {
            if gesture.dragged || travel > VIEW.drag_threshold_pixels {
                // Orbit from where the drag started, so small early movements accumulate.
                gesture.dragged = true;
                let delta = pointer.position - gesture.last;
                self.camera.yaw -= delta.x * VIEW.orbit_sensitivity;
                self.camera.pitch = (self.camera.pitch + (delta.y * VIEW.orbit_sensitivity))
                    .clamp(VIEW.minimum_pitch, VIEW.maximum_pitch);
                gesture.last = pointer.position;
            }
            self.gesture = Some(gesture);
            return Ok(());
        }
        // Released: focus loss and hidden pages release held input too, and those never strike.
        self.gesture = None;
        let click = !gesture.dragged && travel <= VIEW.drag_threshold_pixels && !frame.input.suspended();
        if click {
            self.use_tool_at(pointer.position, frame.viewport)?;
        }
        Ok(())
    }
}

impl App for FormDestructionDemo {
    fn start(&mut self, context: &mut StartContext<'_>) -> AppResult {
        self.hud.backend = context.capabilities.backend.label().to_owned();
        Ok(())
    }

    fn input(&mut self, frame: &mut Frame<'_>) -> AppResult {
        if frame.input.pressed(CANCEL_ACTION) {
            self.config_open = false;
            self.gesture = None;
        }
        if frame.input.pressed(RESET_ACTION) {
            self.reset()?;
        }
        if frame.input.pressed(TOOL_ACTION) {
            self.set_tool(if self.tool == Tool::Strike { Tool::Charge } else { Tool::Strike });
        }
        self.handle_pointer(frame)
    }

    fn simulation(&mut self, frame: &mut Frame<'_>) -> AppResult {
        let elapsed = frame.time.elapsed_seconds.min(PRESENTATION.maximum_frame_seconds);
        self.advance(f64::from(elapsed))?;
        self.debris.update(&mut self.scene, elapsed)?;
        self.impact_debug.update(&mut self.scene, elapsed);
        Ok(())
    }

    fn render(&mut self, frame: &mut Frame<'_>, target: &mut RenderTarget<'_>) -> AppResult {
        if std::mem::take(&mut self.reset_history) {
            target.renderer().reset_history();
        }
        let material = self.view == StructureView::Material;
        let material_lights = [PRESENTATION.primary_light, PRESENTATION.secondary_light];
        let debug_lights = [INTEGRITY.light];
        let inputs = FrameInputs {
            lights: if material { &material_lights } else { &debug_lights },
            ambient: Some(if material { PRESENTATION.ambient } else { INTEGRITY.ambient }),
            clear_color: PRESENTATION.clear_color,
            ..FrameInputs::default()
        };
        let stopwatch = Stopwatch::start();
        target.draw(&self.scene, &self.camera, &inputs)?;
        retain(&mut self.render_samples, stopwatch.elapsed_milliseconds());
        self.frame_count += 1;
        if self.frame_count.is_multiple_of(PRESENTATION.hud_interval_frames) {
            self.refresh_hud();
        }
        frame.dev_tools.set("chunks", self.hud.chunks.clone());
        frame.dev_tools.set("debris", self.hud.debris.clone());
        frame.dev_tools.set(
            "CPU encode / damage / query",
            format!(
                "{:.2} / {:.2} / {:.3} ms",
                mean(&self.render_samples),
                mean(&self.damage_samples),
                mean(&self.query_samples)
            ),
        );
        Ok(())
    }

    fn ui(&mut self, context: &egui::Context, _frame: &mut Frame<'_>) {
        let settings = HudSettings {
            view: self.view,
            tool: self.tool,
            config_open: self.config_open,
            damage_radius: self.damage_radius,
            damage_per_hit: self.damage_per_hit,
            impact_debug: self.impact_debug.enabled(),
        };
        for command in hud::draw(context, &self.hud, settings) {
            if let Err(error) = self.apply(command) {
                self.hud.notice = error.to_string();
            }
        }

        // Wheel zoom belongs to the canvas, not to panels.
        let scroll = context.input(|input| input.smooth_scroll_delta.y);
        if scroll != 0.0 && !context.is_pointer_over_egui() {
            let distance = self.camera.distance * (-scroll * VIEW.zoom_sensitivity).exp();
            self.camera.distance = distance.clamp(VIEW.minimum_distance, VIEW.maximum_distance);
        }
    }
}

/// Kilograms per tonne, for load readouts.
const KILOGRAMS_PER_TONNE: f64 = 1000.0;

/// Height of a charge's marker above the undisturbed ground, so it clears the crater rim.
const GROUND_MARKER_HEIGHT: f64 = 0.2;

/// Keep the newest samples of a bounded timing distribution.
fn retain(samples: &mut Vec<f64>, value: f64) {
    samples.push(value);
    if samples.len() > PRESENTATION.timing_sample_count {
        samples.remove(0);
    }
}

/// Mean of a timing distribution, or zero without samples.
fn mean(samples: &[f64]) -> f64 {
    if samples.is_empty() { 0.0 } else { samples.iter().sum::<f64>() / samples.len() as f64 }
}
