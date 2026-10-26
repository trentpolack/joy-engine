// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The terrain failure vocabulary shared by assets, tile pyramids, streaming, selection, and the renderer's terrain
//! frame preparation. Messages are stable diagnostics.

/// Failure decoding terrain data, streaming tiles, editing a surface, or preparing a terrain frame.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum TerrainError {
    /// The manifest, package, or a tile file could not be read.
    #[error("Terrain file could not be read: {0}")]
    Io(String),
    /// The manifest is not valid JSON for its schema.
    #[error("Invalid terrain manifest: {0}")]
    InvalidManifest(String),
    /// Dimensions, version, orientation, scales, or the binary length are invalid.
    #[error("Invalid terrain dimensions, orientation, scales or height payload.")]
    InvalidDimensions,
    /// The patch list is not a complete row-major leaf grid.
    #[error("Invalid terrain patch count.")]
    InvalidPatchCount,
    /// A patch is out of place, has invalid bounds, or a non-monotone error hierarchy.
    #[error("Invalid terrain patch bounds or monotone error hierarchy.")]
    InvalidPatch,
    /// A tile pyramid layout is out of range; the message names the field.
    #[error("Invalid terrain layout: {0}.")]
    InvalidLayout(&'static str),
    /// A tile does not belong to its layout or its payload is malformed; the message names the problem.
    #[error("Invalid terrain tile: {0}.")]
    InvalidTile(&'static str),
    /// Detail synthesis settings are out of range; the message names the field.
    #[error("Invalid terrain detail: {0}.")]
    InvalidDetail(&'static str),
    /// A crater is non-finite or outside its accepted range; the message names the field.
    #[error("Invalid terrain crater: {0}.")]
    InvalidCrater(&'static str),
    /// Streaming budgets are out of range; the message names the field.
    #[error("Invalid terrain streaming budget: {0}.")]
    InvalidBudget(&'static str),
    /// A view vector or scalar is non-finite or out of range.
    #[error("Invalid terrain view.")]
    InvalidView,
    /// The view-projection matrix is not finite.
    #[error("Invalid terrain matrix.")]
    InvalidMatrix,
    /// The world transform is not a finite affine transform.
    #[error("Terrain requires a finite affine world transform.")]
    InvalidTransform,
    /// A scene terrain instance has an invalid tolerance or target radius.
    #[error("{0}")]
    InvalidInstance(&'static str),
    /// The device cannot hold this terrain (for example, WebGL 2 texture limits).
    #[error("Terrain is unsupported on this device: {0}")]
    Unsupported(String),
}
