// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rendering errors returned at the renderer's public boundary.

/// Failure creating GPU resources, validating geometry, or submitting a frame.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// No adapter matched the requested backends (for example, no GPU and no software fallback).
    #[error("No compatible GPU adapter is available: {0}")]
    NoAdapter(String),
    /// The adapter rejected the device request.
    #[error("GPU device creation failed: {0}")]
    DeviceCreation(String),
    /// A presentation surface could not be created or configured.
    #[error("GPU surface setup failed: {0}")]
    Surface(String),
    /// Mesh data was malformed (mismatched channels, out-of-range indices, or non-finite values).
    #[error("Invalid mesh data: {0}")]
    InvalidMesh(String),
    /// A frame request was invalid (for example, a zero-sized target).
    #[error("Invalid frame: {0}")]
    InvalidFrame(String),
    /// FORM source failed to parse or evaluate; the diagnostic carries the source location.
    #[error("FORM evaluation failed: {0}")]
    Form(joy_form::Diagnostic),
    /// Reading back a captured frame failed.
    #[error("Frame capture failed: {0}")]
    Capture(String),
}
