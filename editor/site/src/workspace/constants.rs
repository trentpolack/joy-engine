// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Workspace timing, storage keys, and layout bounds.

/// Storage application folder for editor preferences (`runtime::Storage`).
pub const STORAGE_APPLICATION: &str = "joy-editor";

/// Storage key of the restored session (TypeScript `joy-editor-workspace-v1`).
pub const SESSION_STORAGE_KEY: &str = "joy-editor.workspace.v1";

/// Seconds between disk revision checks of open documents.
pub const POLL_SECONDS: f64 = 1.5;

/// Seconds between automatic Content Browser rescans.
pub const REFRESH_SECONDS: f64 = 15.0;

/// Narrowest Content Browser, in logical pixels.
pub const CONTENT_BROWSER_MINIMUM_WIDTH: f32 = 160.0;

/// Widest stored Content Browser, in logical pixels; the displayed width is also limited to 30% of the window.
pub const CONTENT_BROWSER_MAXIMUM_WIDTH: f32 = 600.0;

/// Content Browser width before the author resizes it.
pub const CONTENT_BROWSER_DEFAULT_WIDTH: f32 = 220.0;

/// Logical pixels one arrow-key press resizes a panel divider.
pub const DIVIDER_KEY_STEP: f32 = 20.0;
