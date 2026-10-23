// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Opening files in browsers for [`FileDialogs`](crate::FileDialogs): a detached `<input type="file">` clicked during the
//! author's gesture, read asynchronously with `Blob.text()` or `Blob.arrayBuffer()`, with the result left for the
//! next poll. Dismissing the picker reports a cancellation through the input's `cancel` event where browsers send it.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::{
    JsCast,
    JsValue,
};

use crate::file_dialogs::{
    FileDialogEvent,
    OpenedFile,
    decode_contents,
    too_large_message,
};

/// One running open request.
pub(crate) struct FileInputRequest {
    result: Rc<RefCell<Option<FileDialogEvent>>>,
    // Kept alive until the request finishes.
    _input: web_sys::HtmlInputElement,
}

impl FileInputRequest {
    /// Show the browser's file picker for files matching `accept` (for example `.joyfx,.json`).
    pub(crate) fn open(accept: &str, binary: bool, maximum_bytes: usize) -> Result<Self, String> {
        let describe = |error: JsValue| error.as_string().unwrap_or_else(|| format!("{error:?}"));
        let document = web_sys::window().and_then(|window| window.document()).ok_or("the page has no document")?;
        let input: web_sys::HtmlInputElement = document
            .create_element("input")
            .map_err(describe)?
            .dyn_into()
            .map_err(|_| "inputs are not input elements".to_owned())?;
        input.set_type("file");
        input.set_accept(accept);
        let result = Rc::new(RefCell::new(None));

        let change_result = result.clone();
        let change_input = input.clone();
        let on_change = wasm_bindgen::closure::Closure::once_into_js(move |_event: JsValue| {
            let Some(file) = change_input.files().and_then(|files| files.get(0)) else {
                *change_result.borrow_mut() = Some(FileDialogEvent::Cancelled);
                return;
            };
            let name = file.name();
            if file.size() > maximum_bytes as f64 {
                *change_result.borrow_mut() = Some(FileDialogEvent::Failed(too_large_message(&name, maximum_bytes)));
                return;
            }
            let promise = if binary { file.array_buffer() } else { file.text() };
            wasm_bindgen_futures::spawn_local(async move {
                let event = match wasm_bindgen_futures::JsFuture::from(promise).await {
                    Ok(value) => {
                        let bytes = if binary {
                            js_sys::Uint8Array::new(&value).to_vec()
                        } else {
                            value.as_string().unwrap_or_default().into_bytes()
                        };
                        match decode_contents(&name, bytes, binary) {
                            Ok(contents) => FileDialogEvent::Opened(OpenedFile {
                                name,
                                contents,
                                path: None,
                            }),
                            Err(message) => FileDialogEvent::Failed(message),
                        }
                    }
                    Err(error) => FileDialogEvent::Failed(format!("Cannot read {name}: {}", describe(error))),
                };
                *change_result.borrow_mut() = Some(event);
            });
        });
        input.add_event_listener_with_callback("change", on_change.unchecked_ref()).map_err(describe)?;

        let cancel_result = result.clone();
        let on_cancel = wasm_bindgen::closure::Closure::once_into_js(move |_event: JsValue| {
            let mut slot = cancel_result.borrow_mut();
            if slot.is_none() {
                *slot = Some(FileDialogEvent::Cancelled);
            }
        });
        input.add_event_listener_with_callback("cancel", on_cancel.unchecked_ref()).map_err(describe)?;
        input.click();
        Ok(Self {
            result,
            _input: input,
        })
    }

    /// The result once the file was read or the picker dismissed.
    pub(crate) fn poll(&mut self) -> Option<FileDialogEvent> {
        self.result.borrow_mut().take()
    }
}
