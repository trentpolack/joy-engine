// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Direct Taffy layout with egui content. Keep a [`taffy::TaffyTree`] in the owning view, update its native styles and
//! children, and compute before drawing widgets. Taffy caches geometry; egui owns widget state, input, and painting.
//! All coordinates and measurements are logical egui points, not physical pixels.
//!
//! Measurement callbacks must measure content only: never run interactive widgets in Taffy's measurement pass.
//! Mark measured nodes dirty when their text or font changes. [`show_region`] runs its contents once per egui pass.

use egui::{
    Context,
    FontId,
    InnerResponse,
    Pos2,
    Rect,
    Ui,
    UiBuilder,
    vec2,
};
use taffy::{
    AvailableSpace,
    NodeId,
    Size,
    TaffyTree,
};

/// Native layout styles, tree operations, and algorithms. Consumers need no separate backend dependency.
pub use taffy;

/// A node could not be mapped into the requested layout root.
#[derive(Debug, thiserror::Error)]
pub enum LayoutError {
    /// Taffy rejected a node handle.
    #[error(transparent)]
    Tree(#[from] taffy::TaffyError),
    /// The node is not the root or one of its descendants.
    #[error("layout node is outside the requested root")]
    OutsideRoot,
}

/// Map a computed node's border box into the host's logical coordinates.
///
/// `origin` is the position at which the root was laid out. Ancestor offsets, including root margins, are included.
/// Call after computing `root`; retained node handles belong to `tree` and must not come from another tree.
pub fn node_rect<C>(tree: &TaffyTree<C>, root: NodeId, node: NodeId, origin: Pos2) -> Result<Rect, LayoutError> {
    let layout = tree.layout(node)?;
    let size = vec2(layout.size.width, layout.size.height);
    let mut position = origin;
    let mut current = node;
    loop {
        let layout = tree.layout(current)?;
        position += vec2(layout.location.x, layout.location.y);
        if current == root {
            return Ok(Rect::from_min_size(position, size));
        }
        current = tree.parent(current).ok_or(LayoutError::OutsideRoot)?;
    }
}

/// Draw egui content inside a layout rectangle, clipping paint and hit tests to the parent and region.
///
/// `id_salt` must identify the region uniquely within its parent, independent of its position or visible siblings.
/// The child inherits style and enabled state, uses top-down layout, and contributes its content size to the parent
/// (including scroll extents). Popups should use egui's popup/window layers, outside this clipped child.
pub fn show_region<R>(
    ui: &mut Ui,
    id_salt: impl egui::AsIdSalt,
    rect: Rect,
    contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    // An explicit parent-scoped ID also stabilizes auto-generated widget IDs when another pane is hidden.
    let id = ui.id().with(id_salt);
    ui.scope_builder(UiBuilder::new().id(id).max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)), |child| {
        child.set_clip_rect(child.clip_rect().intersect(rect));
        contents(child)
    })
}

/// Measure wrapped text for a Taffy leaf using the same fonts as egui's renderer.
///
/// Use inside an active egui pass and Taffy's `compute_leaf_layout` measurement callback. Known dimensions take
/// precedence; otherwise definite width wraps, min-content requests egui's narrowest wrapping, and max-content
/// measures an unwrapped line. Invalidate the Taffy leaf after text, font, or font-definition changes.
pub fn measure_text(
    context: &Context,
    font: &FontId,
    text: &str,
    known: Size<Option<f32>>,
    available: Size<AvailableSpace>,
) -> Size<f32> {
    let wrap_width = known.width.unwrap_or(match available.width {
        AvailableSpace::Definite(width) => width.max(0.0),
        AvailableSpace::MinContent => 0.0,
        AvailableSpace::MaxContent => f32::INFINITY,
    });
    let size =
        context.fonts_mut(|fonts| fonts.layout(text.to_owned(), font.clone(), egui::Color32::WHITE, wrap_width).size());
    Size {
        width: known.width.unwrap_or(size.x),
        height: known.height.unwrap_or(size.y),
    }
}

#[cfg(test)]
#[path = "ui_layout.test.rs"]
mod tests;
