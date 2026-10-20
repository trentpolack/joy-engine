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

/// Whether the page's pointer lock belongs to the window's canvas.
pub(crate) fn pointer_locked(window: &winit::window::Window) -> bool {
    use winit::platform::web::WindowExtWebSys;
    let Some(canvas) = window.canvas() else {
        return false;
    };
    let locked =
        web_sys::window().and_then(|page| page.document()).and_then(|document| document.pointer_lock_element());
    locked.is_some_and(|element| {
        let element: &wasm_bindgen::JsValue = element.as_ref();
        let canvas: &wasm_bindgen::JsValue = canvas.as_ref();
        element == canvas
    })
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

/// Whether the canvas's WebGPU context kept the `"extended"` tone mapping it was configured with. Browsers without HDR
/// canvases drop the setting (or cannot report it) and clamp to standard range, so the shell presents standard output
/// to them instead. Reads the context through reflection because the bindings the engine enables do not cover it.
pub(crate) fn canvas_presents_extended_range(window: &winit::window::Window) -> bool {
    use winit::platform::web::WindowExtWebSys;
    let Some(canvas) = window.canvas() else {
        return false;
    };
    let Ok(Some(context)) = canvas.get_context("webgpu") else {
        return false;
    };
    let property = |object: &wasm_bindgen::JsValue, name: &str| {
        js_sys::Reflect::get(object, &wasm_bindgen::JsValue::from_str(name)).ok().filter(|value| !value.is_undefined())
    };
    let Some(get_configuration) =
        property(&context, "getConfiguration").and_then(|method| method.dyn_into::<js_sys::Function>().ok())
    else {
        return false;
    };
    let Ok(configuration) = get_configuration.call0(&context) else {
        return false;
    };
    let mode = property(&configuration, "toneMapping").and_then(|tone_mapping| property(&tone_mapping, "mode"));
    mode.and_then(|mode| mode.as_string()).as_deref() == Some(EXTENDED_TONE_MAPPING)
}

/// The WebGPU canvas tone-mapping mode that presents values above SDR white.
const EXTENDED_TONE_MAPPING: &str = "extended";

/// Publish readiness only after presenting a frame, for the loading screen and browser captures.
pub(crate) fn mark_canvas_ready(window: &winit::window::Window, backend: &str) {
    use winit::platform::web::WindowExtWebSys;
    if let Some(canvas) = window.canvas() {
        let _ = canvas.set_attribute("data-backend", backend);
        let _ = canvas.set_attribute("data-ready", "true");
    }
}

/// Show a startup or runtime failure where the player can see it, as a fixed panel along the bottom of the page.
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
