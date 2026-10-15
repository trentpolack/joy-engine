// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Browser integration: logging, query parameters, the game canvas, and a visible failure panel.

use wasm_bindgen::JsCast;

/// Route `log` output and panics to the browser console. Repeated calls are harmless.
pub(crate) fn init_logging() {
    console_error_panic_hook::set_once();
    let _ = console_log::init_with_level(log::Level::Info);
}

/// Read a URL query parameter (for example `?backend=webgl`).
pub(crate) fn query_parameter(name: &str) -> Option<String> {
    let search = web_sys::window()?.location().search().ok()?;
    web_sys::UrlSearchParams::new_with_str(&search).ok()?.get(name)
}

/// Backing-store pixels per CSS pixel of the window's canvas, falling back to `devicePixelRatio` before layout.
pub(crate) fn canvas_scale(window: &winit::window::Window) -> f32 {
    use winit::platform::web::WindowExtWebSys;
    let physical_width = window.inner_size().width;
    match window.canvas() {
        Some(canvas) if canvas.client_width() > 0 && physical_width > 0 => {
            physical_width as f32 / canvas.client_width() as f32
        }
        _ => window.scale_factor() as f32,
    }
}

/// Find the game canvas by element ID, or create a full-window canvas with that ID.
pub(crate) fn game_canvas(id: &str) -> Option<web_sys::HtmlCanvasElement> {
    let document = web_sys::window()?.document()?;
    if let Some(element) = document.get_element_by_id(id) {
        return element.dyn_into().ok();
    }
    let canvas: web_sys::HtmlCanvasElement = document.create_element("canvas").ok()?.dyn_into().ok()?;
    canvas.set_id(id);
    let style = canvas.style();
    let _ = style.set_property("width", "100vw");
    let _ = style.set_property("height", "100vh");
    let _ = style.set_property("display", "block");
    document.body()?.append_child(&canvas).ok()?;
    Some(canvas)
}

/// Show a startup or runtime failure where the player can see it, matching the TypeScript failure panels.
pub(crate) fn report_failure(title: &str, message: &str) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    let Ok(panel) = document.create_element("div") else {
        return;
    };
    panel.set_id("joy-failure");
    panel
        .set_text_content(Some(&format!("{title} could not start: {message} Reload the page or try another browser.")));
    if let Ok(element) = panel.clone().dyn_into::<web_sys::HtmlElement>() {
        let style = element.style();
        for (property, value) in [
            ("position", "fixed"),
            ("inset", "auto 16px 16px 16px"),
            ("padding", "16px"),
            ("background", "#14242c"),
            ("color", "#e8eeee"),
            ("border", "1px solid #ff6a00"),
            ("font", "14px system-ui, sans-serif"),
            ("z-index", "10"),
        ] {
            let _ = style.set_property(property, value);
        }
    }
    if let Some(body) = document.body() {
        let _ = body.append_child(&panel);
    }
}
