// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Open and save files through the platform's dialogs, without blocking a frame: native builds use the system file
//! dialog (rfd: the desktop portal or zenity on Linux, the common dialogs on Windows, and panels on macOS), and
//! browsers use a file input for opening and a download for saving ([`save_text_file`](joy_storage::save_text_file) and
//! [`save_binary_file`](joy_storage::save_binary_file)).
//!
//! The API is poll-based so it works the same in WebAssembly, where nothing may block: a tool starts one request with
//! [`FileDialogs::open`] or [`FileDialogs::save`] from its UI phase and calls [`FileDialogs::poll`] every frame until
//! an event arrives. Only one request runs at a time. Hosts and tests can [`deliver`](FileDialogs::deliver) results
//! directly.

use std::collections::VecDeque;

/// Default largest file [`FileDialogs::open`] reads.
pub const DEFAULT_MAXIMUM_OPEN_BYTES: usize = 16 * 1024 * 1024;

/// A named set of extensions for a dialog's type filter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    /// Label shown in the dialog, such as `Particle effect`.
    pub name: String,
    /// Extensions without dots, such as `["joyfx", "json"]`.
    pub extensions: Vec<String>,
}

impl FileFilter {
    /// A filter named `name` for `extensions` (without dots).
    pub fn new(name: impl Into<String>, extensions: &[&str]) -> Self {
        Self {
            name: name.into(),
            extensions: extensions.iter().map(|extension| (*extension).to_owned()).collect(),
        }
    }

    /// Whether `file_name` has one of the filter's extensions (case-insensitive).
    pub fn accepts(&self, file_name: &str) -> bool {
        let lower = file_name.to_lowercase();
        self.extensions.iter().any(|extension| lower.ends_with(&format!(".{}", extension.to_lowercase())))
    }
}

/// File contents read or written by a dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileContents {
    /// UTF-8 text.
    Text(String),
    /// Raw bytes (images, GLB).
    Binary(Vec<u8>),
}

impl FileContents {
    /// The contents as bytes.
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            Self::Text(text) => text.as_bytes(),
            Self::Binary(bytes) => bytes,
        }
    }

    /// The text, for text contents.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Binary(_) => None,
        }
    }
}

/// A file the author opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedFile {
    /// File name with extension, without folders.
    pub name: String,
    /// The file's contents.
    pub contents: FileContents,
    /// Where the file lives natively, so tools can read files beside it (such as a FORM instance's script); `None`
    /// in browsers, which do not reveal locations.
    pub path: Option<std::path::PathBuf>,
}

/// What a dialog request produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileDialogEvent {
    /// The author chose a file and it was read.
    Opened(OpenedFile),
    /// The file was written (natively) or downloaded (browsers).
    Saved {
        /// The saved file's name.
        name: String,
        /// Where it went: the written path natively, or the downloaded name in browsers.
        location: String,
    },
    /// The author dismissed the dialog.
    Cancelled,
    /// Reading or writing failed; the message says why.
    Failed(String),
}

/// A request to open one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenRequest {
    /// Dialog title (native dialogs).
    pub title: String,
    /// Type filters; the first is the default.
    pub filters: Vec<FileFilter>,
    /// Read as bytes instead of UTF-8 text.
    pub binary: bool,
    /// Largest accepted file; bigger files fail without being read.
    pub maximum_bytes: usize,
}

impl OpenRequest {
    /// Open a text file matching `filter`.
    pub fn text(title: impl Into<String>, filter: FileFilter) -> Self {
        Self {
            title: title.into(),
            filters: vec![filter],
            binary: false,
            maximum_bytes: DEFAULT_MAXIMUM_OPEN_BYTES,
        }
    }

    /// Open a binary file matching `filter`.
    pub fn binary(title: impl Into<String>, filter: FileFilter) -> Self {
        Self {
            binary: true,
            ..Self::text(title, filter)
        }
    }

    /// Refuse files larger than `bytes`.
    pub fn maximum_bytes(mut self, bytes: usize) -> Self {
        self.maximum_bytes = bytes;
        self
    }
}

/// A request to save one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveRequest {
    /// Dialog title (native dialogs).
    pub title: String,
    /// Suggested file name with extension; browsers download under this name.
    pub suggested_name: String,
    /// Type filters for the native dialog.
    pub filters: Vec<FileFilter>,
    /// What to write.
    pub contents: FileContents,
}

impl SaveRequest {
    /// Save `text` under a suggested name.
    pub fn text(title: impl Into<String>, suggested_name: impl Into<String>, filter: FileFilter, text: String) -> Self {
        Self {
            title: title.into(),
            suggested_name: suggested_name.into(),
            filters: vec![filter],
            contents: FileContents::Text(text),
        }
    }

    /// Save `bytes` (for example a GLB export) under a suggested name.
    pub fn binary(
        title: impl Into<String>,
        suggested_name: impl Into<String>,
        filter: FileFilter,
        bytes: Vec<u8>,
    ) -> Self {
        Self {
            title: title.into(),
            suggested_name: suggested_name.into(),
            filters: vec![filter],
            contents: FileContents::Binary(bytes),
        }
    }
}

/// Why a request could not start.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FileDialogError {
    /// Another request is still running.
    #[error("A file dialog is already open.")]
    Busy,
    /// The suggested name was empty or contained folders.
    #[error("Saved files need a plain file name without folders: \"{0}\".")]
    InvalidName(String),
}

/// The running platform request.
enum Pending {
    #[cfg(not(target_arch = "wasm32"))]
    Open { future: native::DialogFuture, request: OpenRequest },
    #[cfg(not(target_arch = "wasm32"))]
    Save { future: native::DialogFuture, contents: FileContents },
    #[cfg(target_arch = "wasm32")]
    Browser(crate::web_file_input::FileInputRequest),
}

/// One tool's file dialogs.
#[derive(Default)]
pub struct FileDialogs {
    pending: Option<Pending>,
    events: VecDeque<FileDialogEvent>,
}

impl std::fmt::Debug for FileDialogs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("FileDialogs").field("busy", &self.is_busy()).field("events", &self.events).finish()
    }
}

impl FileDialogs {
    /// Dialogs with nothing pending.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a request is running.
    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Ask the author for a file to open. Call from a UI phase that handled the author's click (browsers only open
    /// file pickers during a user gesture).
    pub fn open(&mut self, request: OpenRequest) -> Result<(), FileDialogError> {
        if self.is_busy() {
            return Err(FileDialogError::Busy);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.pending = Some(Pending::Open {
                future: native::pick_file(&request),
                request,
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let accept = accept_attribute(&request.filters);
            match crate::web_file_input::FileInputRequest::open(&accept, request.binary, request.maximum_bytes) {
                Ok(pending) => self.pending = Some(Pending::Browser(pending)),
                Err(message) => self.events.push_back(FileDialogEvent::Failed(message)),
            }
        }
        Ok(())
    }

    /// Save a file: natively the author chooses where; browsers download it under the suggested name.
    pub fn save(&mut self, request: SaveRequest) -> Result<(), FileDialogError> {
        if self.is_busy() {
            return Err(FileDialogError::Busy);
        }
        let name = request.suggested_name.clone();
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
            return Err(FileDialogError::InvalidName(name));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let future = native::save_file(&request);
            self.pending = Some(Pending::Save {
                future,
                contents: request.contents,
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let saved = match &request.contents {
                FileContents::Text(text) => joy_storage::save_text_file(&name, text),
                FileContents::Binary(bytes) => joy_storage::save_binary_file(&name, bytes),
            };
            self.events.push_back(match saved {
                Ok(location) => FileDialogEvent::Saved {
                    name,
                    location,
                },
                Err(error) => FileDialogEvent::Failed(error.to_string()),
            });
        }
        Ok(())
    }

    /// The next result, if one is ready. Call every frame while [`is_busy`](Self::is_busy).
    pub fn poll(&mut self) -> Option<FileDialogEvent> {
        if let Some(event) = self.events.pop_front() {
            return Some(event);
        }
        let event = match self.pending.as_mut()? {
            #[cfg(not(target_arch = "wasm32"))]
            Pending::Open {
                future,
                request,
            } => {
                let chosen = native::poll(future)?;
                Some(match chosen {
                    None => FileDialogEvent::Cancelled,
                    Some(path) => read_file(&path, request.binary, request.maximum_bytes),
                })
            }
            #[cfg(not(target_arch = "wasm32"))]
            Pending::Save {
                future,
                contents,
            } => {
                let chosen = native::poll(future)?;
                Some(match chosen {
                    None => FileDialogEvent::Cancelled,
                    Some(path) => write_file(&path, contents),
                })
            }
            #[cfg(target_arch = "wasm32")]
            Pending::Browser(request) => request.poll(),
        };
        if event.is_some() {
            self.pending = None;
        }
        event
    }

    /// Queue an event as if a dialog produced it (hosts that supply files themselves, and tests).
    pub fn deliver(&mut self, event: FileDialogEvent) {
        self.events.push_back(event);
    }
}

/// The HTML `accept` attribute for filters: `.joyfx,.json`.
pub fn accept_attribute(filters: &[FileFilter]) -> String {
    filters
        .iter()
        .flat_map(|filter| filter.extensions.iter().map(|extension| format!(".{extension}")))
        .collect::<Vec<_>>()
        .join(",")
}

/// Decode opened bytes as the request asked: UTF-8 text or raw bytes.
pub fn decode_contents(name: &str, bytes: Vec<u8>, binary: bool) -> Result<FileContents, String> {
    if binary {
        return Ok(FileContents::Binary(bytes));
    }
    String::from_utf8(bytes).map(FileContents::Text).map_err(|_| format!("{name} is not UTF-8 text."))
}

/// The message for files over the size limit.
pub fn too_large_message(name: &str, maximum_bytes: usize) -> String {
    format!("{name} is larger than {} bytes.", group_thousands(maximum_bytes))
}

/// `6000000` as `6,000,000`.
fn group_thousands(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[cfg(not(target_arch = "wasm32"))]
fn read_file(path: &std::path::Path, binary: bool, maximum_bytes: usize) -> FileDialogEvent {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return FileDialogEvent::Failed(format!("Cannot read {name}: {error}")),
    };
    if metadata.len() > maximum_bytes as u64 {
        return FileDialogEvent::Failed(too_large_message(&name, maximum_bytes));
    }
    match std::fs::read(path)
        .map_err(|error| format!("Cannot read {name}: {error}"))
        .and_then(|bytes| decode_contents(&name, bytes, binary))
    {
        Ok(contents) => FileDialogEvent::Opened(OpenedFile {
            name,
            contents,
            path: Some(path.to_path_buf()),
        }),
        Err(message) => FileDialogEvent::Failed(message),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_file(path: &std::path::Path, contents: &FileContents) -> FileDialogEvent {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    match std::fs::write(path, contents.as_bytes()) {
        Ok(()) => FileDialogEvent::Saved {
            name,
            location: path.display().to_string(),
        },
        Err(error) => FileDialogEvent::Failed(format!("Could not save {name}: {error}")),
    }
}

/// Native dialogs through rfd's asynchronous API, polled each frame with a no-op waker (the dialog completes on its
/// own thread, or modally on macOS's main thread).
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::future::Future;
    use std::path::PathBuf;
    use std::pin::Pin;
    use std::task::{
        Context,
        Poll,
        Waker,
    };

    use super::{
        OpenRequest,
        SaveRequest,
    };

    /// A running dialog.
    pub(super) type DialogFuture = Pin<Box<dyn Future<Output = Option<rfd::FileHandle>>>>;

    pub(super) fn pick_file(request: &OpenRequest) -> DialogFuture {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(&request.title);
        for filter in &request.filters {
            dialog = dialog.add_filter(&filter.name, &filter.extensions);
        }
        Box::pin(dialog.pick_file())
    }

    pub(super) fn save_file(request: &SaveRequest) -> DialogFuture {
        let mut dialog = rfd::AsyncFileDialog::new().set_title(&request.title).set_file_name(&request.suggested_name);
        for filter in &request.filters {
            dialog = dialog.add_filter(&filter.name, &filter.extensions);
        }
        Box::pin(dialog.save_file())
    }

    /// `Some(choice)` once the dialog closed (`None` inside when dismissed).
    pub(super) fn poll(future: &mut DialogFuture) -> Option<Option<PathBuf>> {
        match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(handle) => Some(handle.map(|handle| handle.path().to_path_buf())),
            Poll::Pending => None,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn filters_accept_attributes_and_decoding() {
        let filter = FileFilter::new("Particle effect", &["joyfx", "json"]);
        assert!(filter.accepts("Ember.JOYFX") && filter.accepts("a.json") && !filter.accepts("a.png"));
        assert_eq!(accept_attribute(&[filter, FileFilter::new("PNG", &["png"])]), ".joyfx,.json,.png");
        assert_eq!(decode_contents("a.joyfx", b"{}".to_vec(), false), Ok(FileContents::Text("{}".to_owned())));
        assert!(decode_contents("a.joyfx", vec![0xff, 0xfe], false).is_err());
        assert_eq!(decode_contents("a.png", vec![0xff], true), Ok(FileContents::Binary(vec![0xff])));
        assert_eq!(too_large_message("big.joyfx", 6_000_000), "big.joyfx is larger than 6,000,000 bytes.");
    }

    #[test]
    fn files_read_and_write_with_limits_and_events_queue_in_order() {
        let directory = std::env::temp_dir().join(format!("joy-file-dialogs-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("effect.joyfx");
        assert_eq!(
            write_file(&path, &FileContents::Text("{\"version\": 1}\n".to_owned())),
            FileDialogEvent::Saved {
                name: "effect.joyfx".to_owned(),
                location: path.display().to_string()
            }
        );
        let opened = read_file(&path, false, 1024);
        assert_eq!(
            opened,
            FileDialogEvent::Opened(OpenedFile {
                name: "effect.joyfx".to_owned(),
                contents: FileContents::Text("{\"version\": 1}\n".to_owned()),
                path: Some(path.clone()),
            })
        );
        assert!(
            matches!(read_file(&path, false, 4), FileDialogEvent::Failed(message) if message.contains("larger than 4 bytes"))
        );
        assert!(matches!(read_file(&directory.join("missing.joyfx"), false, 1024), FileDialogEvent::Failed(_)));
        let _ = std::fs::remove_dir_all(&directory);

        let mut dialogs = FileDialogs::new();
        assert!(!dialogs.is_busy());
        dialogs.deliver(FileDialogEvent::Cancelled);
        dialogs.deliver(FileDialogEvent::Failed("no".to_owned()));
        assert_eq!(dialogs.poll(), Some(FileDialogEvent::Cancelled));
        assert_eq!(dialogs.poll(), Some(FileDialogEvent::Failed("no".to_owned())));
        assert_eq!(dialogs.poll(), None);
        let invalid =
            SaveRequest::text("Save", "folder/effect.joyfx", FileFilter::new("Effect", &["joyfx"]), String::new());
        assert_eq!(dialogs.save(invalid), Err(FileDialogError::InvalidName("folder/effect.joyfx".to_owned())));
    }
}
