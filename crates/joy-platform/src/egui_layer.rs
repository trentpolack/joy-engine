// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Immediate-mode UI over the rendered scene, on both native windows and browser canvases.

use joy_render::{GpuContext, wgpu};
use winit::window::Window;

/// Owns egui's context, winit input translation, and the wgpu painter.
///
/// Native builds translate input with egui-winit; browsers use the engine's own translator (see `web_input`).
pub(crate) struct EguiLayer {
    pub context: egui::Context,
    #[cfg(not(target_arch = "wasm32"))]
    state: egui_winit::State,
    #[cfg(target_arch = "wasm32")]
    state: crate::web_input::WebEguiInput,
    painter: egui_wgpu::Renderer,
}

impl EguiLayer {
    /// Create a layer that paints into views of `output_format`.
    pub fn new(gpu: &GpuContext, window: &Window, output_format: wgpu::TextureFormat) -> Self {
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
        let painter = egui_wgpu::Renderer::new(gpu.device(), output_format, egui_wgpu::RendererOptions::default());
        Self { context, state, painter }
    }

    /// Forward a window event; returns whether egui consumed it (the game should then ignore it).
    pub fn on_window_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.state.on_window_event(window, event).consumed
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.state.on_window_event(&self.context, window, event)
        }
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
        let full_output = self.context.run_ui(raw_input, |ui| build(ui.ctx()));
        #[cfg(not(target_arch = "wasm32"))]
        self.state.handle_platform_output(window, full_output.platform_output);

        let pixels_per_point = full_output.pixels_per_point;
        let primitives = self.context.tessellate(full_output.shapes, pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels, pixels_per_point };
        // egui requires every texture update to be consumed: uploads before painting, frees after.
        let mut textures = full_output.textures_delta;
        for (id, deltas) in textures.set.drain() {
            for delta in &deltas {
                self.painter.update_texture(gpu.device(), gpu.queue(), id, delta);
            }
        }
        self.painter.update_buffers(gpu.device(), gpu.queue(), encoder, &primitives, &screen);
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
            self.painter.render(&mut pass, &primitives, &screen);
        }
        for id in textures.free.drain() {
            self.painter.free_texture(&id);
        }
    }
}
