// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Immediate-mode UI over the rendered scene, on both native windows and browser canvases.

use joy_renderer::{
    DisplayOutput,
    GpuContext,
    OutputEncoding,
};
use winit::window::Window;

use crate::linear_composite::{
    INTERFACE_FORMAT,
    LinearComposite,
};

/// Owns egui's context, winit input translation, and the wgpu painter.
///
/// Native builds translate input with egui-winit; browsers use the engine's own translator (see `web_input`). UI white is
/// SDR white on every output: extended linear (scRGB) targets paint through a composite that scales it to the display's
/// SDR white level, and the other encodings take egui's output as it is.
pub struct EguiLayer {
    /// The egui context every frame's UI runs in.
    pub context: egui::Context,
    #[cfg(not(target_arch = "wasm32"))]
    state: egui_winit::State,
    #[cfg(target_arch = "wasm32")]
    state: crate::web_input::WebEguiInput,
    painter: egui_wgpu::Renderer,
    /// Present for extended linear outputs, where egui paints into an intermediate first.
    composite: Option<LinearComposite>,
}

impl EguiLayer {
    /// Create a layer that paints into views of `output_format` carrying `display`'s signal. The encoding is fixed for
    /// the layer's lifetime; [`EguiLayer::set_display_output`] follows later SDR white level changes.
    pub fn new(gpu: &GpuContext, window: &Window, output_format: wgpu::TextureFormat, display: DisplayOutput) -> Self {
        let context = egui::Context::default();
        crate::theme::apply(&context);
        let max_texture_side = gpu.capabilities().max_texture_dimension as usize;
        #[cfg(not(target_arch = "wasm32"))]
        let state = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture_side),
        );
        #[cfg(target_arch = "wasm32")]
        let state = {
            let _ = window;
            crate::web_input::WebEguiInput::new(max_texture_side)
        };
        let composite = (display.encoding == OutputEncoding::ExtendedLinear).then(|| {
            LinearComposite::new(gpu.device(), gpu.queue(), output_format, display.effective_sdr_white_level())
        });
        let painter_format = if composite.is_some() { INTERFACE_FORMAT } else { output_format };
        let painter = egui_wgpu::Renderer::new(gpu.device(), painter_format, egui_wgpu::RendererOptions::default());
        Self { context, state, painter, composite }
    }

    /// Follow a change in the display's SDR white level. The encoding stays the one the layer was created for.
    pub fn set_display_output(&mut self, gpu: &GpuContext, display: DisplayOutput) {
        if let Some(composite) = &self.composite {
            composite.set_sdr_white_level(gpu.queue(), display.effective_sdr_white_level());
        }
    }

    /// Forward a window event; returns whether egui consumed it (the game should then ignore it).
    pub fn on_window_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let consumed = self.state.on_window_event(window, event).consumed;
            // egui-winit answers from the previous pass's hover, which misses a tap that lands on a widget directly.
            if let winit::event::WindowEvent::Touch(touch) = event
                && touch.phase == winit::event::TouchPhase::Started
            {
                let scale = self.context.pixels_per_point();
                let point = egui::pos2(touch.location.x as f32 / scale, touch.location.y as f32 / scale);
                return claims_press(&self.context, point);
            }
            consumed
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.state.on_window_event(&self.context, window, event)
        }
    }

    /// The UI context, for development tools that coordinate with the shell between frames.
    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    /// Whether a text field or other widget wants keyboard input, so gameplay should not receive keys.
    pub fn wants_keyboard(&self) -> bool {
        self.context.egui_wants_keyboard_input()
    }

    /// Run one UI pass and paint it over `output`.
    pub fn paint(
        &mut self,
        gpu: &GpuContext,
        window: &Window,
        encoder: &mut wgpu::CommandEncoder,
        output: &wgpu::TextureView,
        size_in_pixels: [u32; 2],
        build: impl FnMut(&egui::Context),
    ) {
        let mut build = build;
        #[cfg(not(target_arch = "wasm32"))]
        let raw_input = self.state.take_egui_input(window);
        #[cfg(target_arch = "wasm32")]
        let raw_input = self.state.take_input(window);
        let mut full_output = self.context.run_ui(raw_input, |ui| build(ui.ctx()));
        let platform_output = std::mem::take(&mut full_output.platform_output);
        self.state.handle_platform_output(window, platform_output);

        let Some(composite) = &mut self.composite else {
            paint_output(&mut self.painter, gpu, &self.context, encoder, output, size_in_pixels, full_output);
            return;
        };

        // Extended linear output: paint into the cleared intermediate, then blend it over the scene.
        let interface = composite.interface_view(gpu.device(), size_in_pixels);
        clear_interface(encoder, interface);
        paint_output(&mut self.painter, gpu, &self.context, encoder, interface, size_in_pixels, full_output);
        composite.composite(encoder, output);
    }
}

/// Clear the UI intermediate to transparent before egui paints into it.
fn clear_interface(encoder: &mut wgpu::CommandEncoder, interface: &wgpu::TextureView) {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ui clear"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: interface,
            resolve_target: None,
            ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            depth_slice: None,
        })],
        ..Default::default()
    });
}

/// Tessellate one UI pass's output and paint it over `output` with `painter`, consuming every texture update (uploads
/// before painting, frees after), as egui requires. Shared by the windowed shell and `HeadlessShell`.
pub fn paint_output(
    painter: &mut egui_wgpu::Renderer,
    gpu: &GpuContext,
    context: &egui::Context,
    encoder: &mut wgpu::CommandEncoder,
    output: &wgpu::TextureView,
    size_in_pixels: [u32; 2],
    full_output: egui::FullOutput,
) {
    let pixels_per_point = full_output.pixels_per_point;
    let primitives = context.tessellate(full_output.shapes, pixels_per_point);
    let screen = egui_wgpu::ScreenDescriptor { size_in_pixels, pixels_per_point };
    let mut textures = full_output.textures_delta;
    for (id, deltas) in textures.set.drain() {
        for delta in &deltas {
            painter.update_texture(gpu.device(), gpu.queue(), id, delta);
        }
    }
    painter.update_buffers(gpu.device(), gpu.queue(), encoder, &primitives, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: output,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                depth_slice: None,
            })],
            ..Default::default()
        });
        let mut pass = pass.forget_lifetime();
        painter.render(&mut pass, &primitives, &screen);
    }
    for id in textures.free.drain() {
        painter.free_texture(&id);
    }
}

/// egui modifiers for the keys a browser reports. Command is the platform's shortcut key, as egui-winit maps it
/// natively: Cmd (the Meta key) on Apple platforms, Ctrl elsewhere.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) fn browser_modifiers(alt: bool, ctrl: bool, shift: bool, meta: bool, apple: bool) -> egui::Modifiers {
    egui::Modifiers { alt, ctrl, shift, mac_cmd: apple && meta, command: if apple { meta } else { ctrl } }
}

/// Whether egui claims a press at `position` (points): a widget already holds the pointer, or an interactable area
/// (a HUD panel, button, or development tool) lies under the press.
///
/// egui's own hover state still describes the previous pointer position when a press arrives together with a jump
/// (touch and pen taps, synthetic clicks), so presses test the areas at their own position; otherwise a tap on a HUD
/// button would also reach gameplay behind it.
pub(crate) fn claims_press(context: &egui::Context, position: egui::Pos2) -> bool {
    context.egui_is_using_pointer()
        || context.layer_id_at(position).is_some_and(|layer| layer.order != egui::Order::Background)
}

#[cfg(test)]
#[path = "egui_layer.test.rs"]
mod tests;
