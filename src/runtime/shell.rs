// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The game shell: one window (native) or canvas (browser), its GPU device and renderer, input, development UI, and the
//! frame loop that runs a [`Game`]'s phases.
//!
//! Startup proceeds as: create the window or canvas, create the GPU device for its surface, create the renderer and UI
//! layer, call [`Game::start`], then run frames. Each frame: tick the clock, update input, run the input, simulation,
//! and presentation phases, acquire the surface, run the render phase, paint UI, present, and run the after-frame phase.
//! A phase error stops the loop and is shown to the player. Hiding the window resets the clock; losing focus releases
//! held input.

use std::collections::HashSet;
use std::sync::Arc;

use crate::rendering::{
    BackendPreference,
    GpuContext,
    RenderError,
    Renderer,
    RendererOptions,
};
use web_time::Instant;
use winit::application::ApplicationHandler;
use winit::event::{
    DeviceEvent,
    DeviceId,
    ElementState,
    MouseButton,
    TouchPhase,
    WindowEvent,
};
use winit::event_loop::{
    ActiveEventLoop,
    ControlFlow,
    EventLoop,
};
use winit::keyboard::PhysicalKey;
use winit::window::{
    CursorGrabMode,
    Window,
    WindowId,
};

use crate::development::DevTools;
use crate::development::dock;
use crate::runtime::clock::{
    DEFAULT_MAXIMUM_FRAME_SECONDS,
    FrameClock,
};
use crate::runtime::egui_layer::EguiLayer;
use crate::runtime::game::{
    Frame,
    Game,
    GameResult,
    RenderTarget,
    ShellRequests,
    StartContext,
    Viewport,
};
use crate::runtime::gamepads::GamepadPoller;
use crate::runtime::input::{
    InputManager,
    InputMap,
    key,
    key_code_name,
};
use crate::runtime::surface::ConfiguredSurface;

/// Shell configuration; copied at startup.
#[derive(Clone, Debug)]
pub struct ShellOptions {
    /// Window title and development overlay label.
    pub title: String,
    /// Enables the development overlay (backquote toggles details). Disable for production builds.
    pub development: bool,
    /// Renderer settings (shadows, sky, display processing).
    pub renderer: RendererOptions,
    /// Preferred graphics API; browsers also honor `?backend=webgpu` or `?backend=webgl`.
    pub backend: BackendPreference,
    /// Player input actions and axes.
    pub input: InputMap,
    /// Initial native window size in logical pixels.
    pub window_size: (u32, u32),
    /// Browser canvas element ID; a full-window canvas is created when it does not exist.
    pub canvas_id: String,
    /// Upper bound on one frame's elapsed seconds.
    pub maximum_frame_seconds: f32,
}

impl Default for ShellOptions {
    fn default() -> Self {
        Self {
            title: "Joy Engine".to_owned(),
            development: cfg!(debug_assertions),
            renderer: RendererOptions::default(),
            backend: BackendPreference::Auto,
            input: InputMap::new(),
            window_size: (1280, 800),
            canvas_id: "joy-canvas".to_owned(),
            maximum_frame_seconds: DEFAULT_MAXIMUM_FRAME_SECONDS,
        }
    }
}

/// Failure starting or running the shell.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// The windowing system could not create an event loop or window.
    #[error("Window setup failed: {0}")]
    Window(String),
    /// GPU, surface, or renderer startup failed.
    #[error("Graphics startup failed: {0}")]
    Graphics(#[from] RenderError),
    /// A game phase returned an error.
    #[error("The game stopped: {0}")]
    Game(String),
}

/// Run `game` until the window closes (native) or indefinitely (browser).
///
/// Native builds block and return when the window closes or a phase fails. Browser builds return immediately after
/// handing the loop to the page.
pub fn run<G: Game>(options: ShellOptions, game: G) -> Result<(), ShellError> {
    init_logging();
    let event_loop = EventLoop::new().map_err(|error| ShellError::Window(error.to_string()))?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let app = App { options, game, phase: Phase::Created, result: Ok(()) };

    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut app = app;
        event_loop.run_app(&mut app).map_err(|error| ShellError::Window(error.to_string()))?;
        app.result
    }
    #[cfg(target_arch = "wasm32")]
    {
        use winit::platform::web::EventLoopExtWebSys;
        event_loop.spawn_app(app);
        Ok(())
    }
}

/// GPU objects created for the window's surface, before the renderer exists.
struct Startup {
    gpu: GpuContext,
    surface: ConfiguredSurface,
}

/// Browser startup completes asynchronously and hands its result to the event loop through this slot.
#[cfg(target_arch = "wasm32")]
type StartupSlot = std::rc::Rc<std::cell::RefCell<Option<Result<Startup, RenderError>>>>;

enum Phase {
    Created,
    #[cfg(target_arch = "wasm32")]
    Starting(Arc<Window>, StartupSlot),
    Running(Box<Running>),
    Failed,
}

/// Everything the frame loop owns once startup succeeds. Field order is drop order: UI and renderer before the surface.
struct Running {
    egui: EguiLayer,
    renderer: Renderer,
    surface: ConfiguredSurface,
    gpu: GpuContext,
    window: Arc<Window>,
    input: InputManager,
    gamepads: GamepadPoller,
    dev_tools: DevTools,
    clock: FrameClock,
    requests: ShellRequests,
    started: Instant,
    /// The game asked for the pointer lock and the shell applied it.
    pointer_lock: bool,
    touches: TouchOwners,
    #[cfg(target_arch = "wasm32")]
    first_frame_presented: bool,
}

/// Which touches belong to the UI. A touch that starts on a widget stays with the UI until it ends, so dragging a slider
/// or scrolling a panel never turns into a gameplay touch (which would also hold the primary button underneath).
#[derive(Debug, Default)]
struct TouchOwners {
    interface: HashSet<u64>,
}

impl TouchOwners {
    /// Whether gameplay receives this touch event; `ui_claimed` says whether the UI claimed a starting touch.
    fn route_to_gameplay(&mut self, id: u64, phase: TouchPhase, ui_claimed: bool) -> bool {
        match phase {
            // A new touch is decided afresh, even when the platform reuses an identifier.
            TouchPhase::Started if ui_claimed => {
                self.interface.insert(id);
                false
            }
            TouchPhase::Started => {
                self.interface.remove(&id);
                true
            }
            TouchPhase::Moved => !self.interface.contains(&id),
            TouchPhase::Ended | TouchPhase::Cancelled => !self.interface.remove(&id),
        }
    }
}

struct App<G: Game> {
    options: ShellOptions,
    game: G,
    phase: Phase,
    result: Result<(), ShellError>,
}

impl<G: Game> ApplicationHandler for App<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !matches!(self.phase, Phase::Created) {
            return;
        }
        let window = match event_loop.create_window(window_attributes(&self.options)) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                self.fail(event_loop, ShellError::Window(error.to_string()));
                return;
            }
        };
        let backend = resolve_backend(self.options.backend);

        #[cfg(not(target_arch = "wasm32"))]
        {
            match pollster::block_on(start_gpu(window.clone(), backend)) {
                Ok(startup) => self.finish_startup(event_loop, window, startup),
                Err(error) => self.fail(event_loop, ShellError::Graphics(error)),
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            let slot: StartupSlot = Default::default();
            let task_slot = slot.clone();
            let task_window = window.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = start_gpu(task_window.clone(), backend).await;
                *task_slot.borrow_mut() = Some(result);
                task_window.request_redraw();
            });
            self.phase = Phase::Starting(window, slot);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        #[cfg(target_arch = "wasm32")]
        self.poll_startup(event_loop);

        let Phase::Running(running) = &mut self.phase else {
            if matches!(event, WindowEvent::CloseRequested) {
                event_loop.exit();
            }
            return;
        };
        let consumed = running.egui.on_window_event(&running.window, &event);

        match event {
            WindowEvent::CloseRequested if self.game.close_requested() => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                running.surface.resize(&running.gpu, size.width, size.height);
                running.renderer.resize(size.width, size.height, display_scale(&running.window));
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = running.window.inner_size();
                running.renderer.resize(size.width, size.height, display_scale(&running.window));
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let name = key_code_name(code);
                let pressed = event.state == ElementState::Pressed;
                if pressed && !event.repeat && name == key::BACKQUOTE {
                    running.dev_tools.toggle();
                }
                // Escape closes an open development panel instead of reaching gameplay, and keys a development tool
                // claims (such as the post-processing pause shortcut) stay with that tool.
                let context = running.egui.context();
                if pressed && !event.repeat && name == key::ESCAPE && dock::close_open_panel(context) {
                    return;
                }
                let claimed = dock::key_claimed(context, &name);
                // Releases always reach gameplay so no key sticks; presses only when the UI is not typing.
                if !pressed || (!consumed && !claimed && !running.egui.wants_keyboard()) {
                    running.input.handle_key(&name, pressed);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                if !pressed || !consumed {
                    running.input.handle_mouse_button(mouse_button_index(button), pressed);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let scale = running.window.scale_factor();
                running
                    .input
                    .handle_pointer_move(glam::Vec2::new((position.x / scale) as f32, (position.y / scale) as f32));
            }
            WindowEvent::CursorLeft { .. } => running.input.handle_pointer_leave(),
            WindowEvent::Touch(touch) => {
                let scale = running.window.scale_factor();
                let position = glam::Vec2::new((touch.location.x / scale) as f32, (touch.location.y / scale) as f32);
                // Touches that start on a UI widget belong to the UI until they end.
                if running.touches.route_to_gameplay(touch.id, touch.phase, consumed) {
                    match touch.phase {
                        TouchPhase::Started | TouchPhase::Moved => running.input.handle_touch(touch.id, Some(position)),
                        TouchPhase::Ended | TouchPhase::Cancelled => running.input.handle_touch(touch.id, None),
                    }
                }
            }
            WindowEvent::Focused(focused) => {
                running.input.set_focused(focused);
                // Losing focus releases a native pointer lock, like a browser does.
                if !focused && running.pointer_lock {
                    apply_pointer_lock(&running.window, false);
                    running.pointer_lock = false;
                }
            }
            WindowEvent::Occluded(occluded) => {
                // Hidden pages and minimized windows lose held input and must not catch up when shown again.
                running.clock.reset();
                running.input.set_visible(!occluded);
                self.game.visibility_changed(!occluded);
            }
            WindowEvent::RedrawRequested => {
                if let Err(error) = self.frame(event_loop) {
                    self.fail(event_loop, ShellError::Game(error.to_string()));
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _device_id: DeviceId, event: DeviceEvent) {
        let Phase::Running(running) = &mut self.phase else {
            return;
        };
        // Raw motion keeps arriving at the screen edge, so locked pointers read it for mouse look.
        if let DeviceEvent::MouseMotion { delta } = event
            && running.pointer_lock
        {
            let scale = running.window.scale_factor();
            running.input.handle_pointer_motion(glam::Vec2::new((delta.0 / scale) as f32, (delta.1 / scale) as f32));
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        #[cfg(target_arch = "wasm32")]
        self.poll_startup(event_loop);

        // Render continuously; presentation is paced by the display's vertical sync.
        if let Phase::Running(running) = &self.phase {
            running.window.request_redraw();
        }
        let _ = event_loop;
    }
}

impl<G: Game> App<G> {
    /// Create the renderer and UI for a started device, then call the game's start hook.
    fn finish_startup(&mut self, event_loop: &ActiveEventLoop, window: Arc<Window>, startup: Startup) {
        let Startup { gpu, mut surface } = startup;
        // Browser canvases can be laid out while the device starts; adopt the current size.
        let size = window.inner_size();
        surface.resize(&gpu, size.width, size.height);
        let scale_factor = display_scale(&window);
        let mut renderer = Renderer::new(&gpu, surface.view_format, self.options.renderer.clone());
        renderer.resize(size.width, size.height, scale_factor);
        let egui = EguiLayer::new(&gpu, &window, surface.view_format);

        let viewport = Viewport { logical_size: renderer.logical_size(), scale_factor };
        let mut context = StartContext { capabilities: gpu.capabilities(), renderer: &mut renderer, viewport };
        if let Err(error) = self.game.start(&mut context) {
            self.fail(event_loop, ShellError::Game(error.to_string()));
            return;
        }

        self.phase = Phase::Running(Box::new(Running {
            egui,
            renderer,
            surface,
            gpu,
            window: window.clone(),
            input: InputManager::new(self.options.input.clone()),
            gamepads: GamepadPoller::new(),
            dev_tools: DevTools::new(&self.options.title, self.options.development),
            clock: FrameClock::new(self.options.maximum_frame_seconds),
            requests: ShellRequests::default(),
            started: Instant::now(),
            pointer_lock: false,
            touches: TouchOwners::default(),
            #[cfg(target_arch = "wasm32")]
            first_frame_presented: false,
        }));
        window.request_redraw();
    }

    /// Move a finished browser startup into the running phase.
    #[cfg(target_arch = "wasm32")]
    fn poll_startup(&mut self, event_loop: &ActiveEventLoop) {
        let Phase::Starting(window, slot) = &self.phase else {
            return;
        };
        let Some(result) = slot.borrow_mut().take() else {
            return;
        };
        let window = window.clone();
        match result {
            Ok(startup) => self.finish_startup(event_loop, window, startup),
            Err(error) => self.fail(event_loop, ShellError::Graphics(error)),
        }
    }

    /// Run one frame's phases, render, UI, and presentation.
    fn frame(&mut self, event_loop: &ActiveEventLoop) -> GameResult {
        let Phase::Running(running) = &mut self.phase else {
            return Ok(());
        };
        let Running {
            egui,
            renderer,
            surface,
            gpu,
            window,
            input,
            gamepads,
            dev_tools,
            clock,
            requests,
            started,
            pointer_lock,
            touches: _,
            #[cfg(target_arch = "wasm32")]
            first_frame_presented,
        } = &mut **running;
        let game = &mut self.game;

        if std::mem::take(&mut requests.reset_clock) {
            clock.reset();
        }
        let (width, height) = renderer.physical_size();
        if width == 0 || height == 0 {
            // A minimized window, or a canvas the page has not laid out yet: nothing to draw and no time passes.
            clock.reset();
            return Ok(());
        }
        let time = clock.tick(started.elapsed().as_secs_f64());
        // UI text entry suspends gameplay input, as focused form fields do in the TypeScript shell.
        input.set_typing(egui.wants_keyboard());
        input.set_gamepads(gamepads.poll());
        let lock_held = *pointer_lock && pointer_lock_held(window);
        input.set_pointer_locked(lock_held);
        input.update();
        let viewport = Viewport { logical_size: renderer.logical_size(), scale_factor: display_scale(window) };
        let mut frame = Frame { time, viewport, input, dev_tools, requests };

        // Simulation-facing phases.
        game.input(&mut frame)?;
        game.simulation(&mut frame)?;
        game.presentation(&mut frame)?;

        // Acquire the presentation texture; transient surface states skip this frame's drawing.
        let texture = match surface.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) | wgpu::CurrentSurfaceTexture::Suboptimal(texture) => texture,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                surface.reconfigure(gpu);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => return Ok(()),
            wgpu::CurrentSurfaceTexture::Validation => return Err("the presentation surface failed validation".into()),
        };
        let view = texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor { format: Some(surface.view_format), ..Default::default() });
        let mut encoder =
            gpu.device().create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("joy-engine frame") });

        // Render phase.
        let mut target = RenderTarget { renderer, encoder: &mut encoder, output: &view, drawn: false };
        game.render(&mut frame, &mut target)?;
        if !target.drawn {
            clear(target.encoder, &view);
        }
        let diagnostics = renderer.diagnostics().clone();

        // Immediate-mode UI over the scene.
        let size = [surface.config.width, surface.config.height];
        egui.paint(gpu, window, &mut encoder, &view, size, |context| {
            game.ui(context, &mut frame);
            frame.dev_tools.ui(context, &diagnostics, viewport);
        });

        gpu.queue().submit([encoder.finish()]);
        window.pre_present_notify();
        gpu.queue().present(texture);

        #[cfg(target_arch = "wasm32")]
        if !*first_frame_presented {
            crate::browser::dom::mark_canvas_ready(window, gpu.capabilities().backend.label());
            *first_frame_presented = true;
        }

        game.after_frame(&mut frame)?;
        frame.dev_tools.frame(time.elapsed_seconds);
        // A browser may release the lock on its own (Escape), so a new request locks again even if the shell still
        // thinks it holds one.
        if let Some(locked) = frame.requests.pointer_lock.take()
            && (locked != *pointer_lock || (locked && !lock_held))
        {
            apply_pointer_lock(window, locked);
            *pointer_lock = locked;
        }
        if frame.requests.exit {
            event_loop.exit();
        }
        Ok(())
    }

    /// Stop the loop and make the failure visible.
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: ShellError) {
        log::error!("{}: {error}", self.options.title);
        #[cfg(target_arch = "wasm32")]
        crate::browser::dom::report_failure(&self.options.title, &error.to_string());
        self.phase = Phase::Failed;
        self.result = Err(error);
        if cfg!(not(target_arch = "wasm32")) {
            event_loop.exit();
        }
    }
}

/// Create the GPU device for a window's surface.
async fn start_gpu(window: Arc<Window>, backend: BackendPreference) -> Result<Startup, RenderError> {
    let instance = GpuContext::create_instance(backend).await;
    let surface = instance.create_surface(window.clone()).map_err(|error| RenderError::Surface(error.to_string()))?;
    let gpu = GpuContext::new(instance, Some(&surface)).await?;
    let size = window.inner_size();
    let surface = ConfiguredSurface::new(&gpu, surface, size.width, size.height)?;
    Ok(Startup { gpu, surface })
}

/// Physical pixels per logical pixel for layout and rendering.
///
/// Browsers measure the canvas backing store and `devicePixelRatio` separately, and the two disagree under some zoom and
/// emulation settings; the ratio of the canvas's backing size to its CSS size is what is actually displayed, so the web
/// uses it. Pointer coordinates keep winit's own ratio, which is how winit encoded them.
pub(crate) fn display_scale(window: &Window) -> f32 {
    #[cfg(target_arch = "wasm32")]
    {
        crate::browser::dom::canvas_scale(window)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        window.scale_factor() as f32
    }
}

/// Native window or browser canvas attributes.
fn window_attributes(options: &ShellOptions) -> winit::window::WindowAttributes {
    let attributes = Window::default_attributes().with_title(options.title.clone());

    // Browser canvases keep the size the page's CSS gives them; only native windows take the configured size.
    #[cfg(target_arch = "wasm32")]
    let attributes = {
        use winit::platform::web::WindowAttributesExtWebSys;
        attributes
            .with_canvas(crate::browser::dom::game_canvas(&options.canvas_id))
            .with_prevent_default(true)
            .with_focusable(true)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let attributes =
        attributes.with_inner_size(winit::dpi::LogicalSize::new(options.window_size.0, options.window_size.1));
    attributes
}

/// Apply the browser's `?backend=` override.
fn resolve_backend(preference: BackendPreference) -> BackendPreference {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(value) = crate::browser::dom::query_parameter("backend") {
            return BackendPreference::from_query_value(Some(&value));
        }
    }
    preference
}

/// Lock (hide and capture) or release the pointer. Native platforms without a true lock confine the cursor to the window
/// instead; raw motion still arrives either way.
fn apply_pointer_lock(window: &Window, locked: bool) {
    if locked {
        let grabbed = window
            .set_cursor_grab(CursorGrabMode::Locked)
            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
        if let Err(error) = grabbed {
            log::warn!("Pointer lock unavailable: {error}");
        }
    } else {
        let _ = window.set_cursor_grab(CursorGrabMode::None);
    }
    window.set_cursor_visible(!locked);
}

/// Whether the platform still holds a requested pointer lock: the page's lock element in browsers (which release it on
/// Escape), and always natively (focus loss releases it through the shell).
fn pointer_lock_held(window: &Window) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        crate::browser::dom::pointer_locked(window)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = window;
        true
    }
}

/// DOM-style mouse button index: 0 primary, 1 middle, 2 secondary.
fn mouse_button_index(button: MouseButton) -> u16 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::Back => 3,
        MouseButton::Forward => 4,
        MouseButton::Other(index) => index,
    }
}

/// Clear an undrawn frame to black.
fn clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
            depth_slice: None,
        })],
        ..Default::default()
    });
}

/// Route logs to stderr (native) or the console (browser).
fn init_logging() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = env_logger::Builder::from_env(
            env_logger::Env::default().default_filter_or("info,wgpu_core=warn,wgpu_hal=warn,naga=warn"),
        )
        .try_init();
    }
    #[cfg(target_arch = "wasm32")]
    crate::browser::dom::init_logging();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touches_that_start_on_the_interface_stay_there_until_released() {
        let mut owners = TouchOwners::default();
        // A drag that starts on a slider: none of it reaches gameplay.
        assert!(!owners.route_to_gameplay(7, TouchPhase::Started, true));
        assert!(!owners.route_to_gameplay(7, TouchPhase::Moved, false), "moving off the widget stays with the UI");
        assert!(!owners.route_to_gameplay(7, TouchPhase::Ended, false));
        // A touch on the scene reaches gameplay throughout, even with the same identifier reused.
        assert!(owners.route_to_gameplay(7, TouchPhase::Started, false));
        assert!(owners.route_to_gameplay(7, TouchPhase::Moved, true), "passing over a widget does not steal it");
        assert!(owners.route_to_gameplay(7, TouchPhase::Cancelled, false));
        // Simultaneous touches are owned independently.
        assert!(!owners.route_to_gameplay(1, TouchPhase::Started, true));
        assert!(owners.route_to_gameplay(2, TouchPhase::Started, false));
        assert!(!owners.route_to_gameplay(1, TouchPhase::Moved, false));
        assert!(owners.route_to_gameplay(2, TouchPhase::Moved, false));
    }
}
