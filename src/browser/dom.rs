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

/// Offer `contents` as a downloaded file named `file_name`: a UTF-8 text `Blob` behind a temporary object URL, which
/// avoids URL length limits for large exports.
pub(crate) fn download_text(file_name: &str, contents: &str) -> Result<(), String> {
    let parts = js_sys::Array::of1(&wasm_bindgen::JsValue::from_str(contents));
    download_blob(file_name, &parts, "text/plain;charset=utf-8")
}

/// Offer `bytes` as a downloaded binary file named `file_name` (GLB exports, images).
pub(crate) fn download_bytes(file_name: &str, bytes: &[u8], media_type: &str) -> Result<(), String> {
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(bytes));
    download_blob(file_name, &parts, media_type)
}

/// Download a `Blob` of `parts` through a temporary object URL. The URL is revoked right after the link is clicked,
/// which every major browser allows for same-document downloads.
fn download_blob(file_name: &str, parts: &js_sys::Array, media_type: &str) -> Result<(), String> {
    let describe = |error: wasm_bindgen::JsValue| error.as_string().unwrap_or_else(|| format!("{error:?}"));
    let document = web_sys::window().and_then(|window| window.document()).ok_or("the page has no document")?;
    let options = web_sys::BlobPropertyBag::new();
    options.set_type(media_type);
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(parts, &options).map_err(describe)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(describe)?;
    let clicked = (|| -> Result<(), String> {
        let link = document.create_element("a").map_err(describe)?;
        link.set_attribute("href", &url).map_err(describe)?;
        link.set_attribute("download", file_name).map_err(describe)?;
        let link: web_sys::HtmlElement = link.dyn_into().map_err(|_| "links are not HTML elements".to_owned())?;
        link.click();
        Ok(())
    })();
    let _ = web_sys::Url::revoke_object_url(&url);
    clicked
}

/// Whether the page runs on an Apple platform, where Cmd is the shortcut key.
pub(crate) fn apple_platform() -> bool {
    let Some(navigator) = web_sys::window().map(|window| window.navigator()) else {
        return false;
    };
    let platform = navigator.platform().unwrap_or_default();
    let agent = navigator.user_agent().unwrap_or_default();
    ["Mac", "iPhone", "iPad", "iPod"].iter().any(|name| platform.contains(name) || agent.contains(name))
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
