# Direct Taffy Layout
Joy Engine uses Taffy 0.15 for layout while egui owns widgets, input, fonts, focus, accessibility output, and painting. Consumers reach both through `joy_engine`, with Taffy under `runtime::ui_layout::taffy`. The engine adds geometry conversion, text measurement, and a clipped egui region; it does not replace Taffy's style or tree APIs.

## Ownership and Coordinates
Keep the `TaffyTree` and its node IDs in the view that owns the layout. Update native `Style` values and child lists when they change, then compute layout before drawing. Existing nodes retain Taffy's caches. Node IDs are layout handles, not widget IDs: pass stable semantic identifiers to `show_region` and the widgets themselves. Removing or hiding another region must not change an edited field's identity.

All dimensions are logical egui points. Pass the host rectangle's size to Taffy and its origin to `node_rect`; the helper accumulates nested ancestor offsets. Disable Taffy's integer rounding for fractional logical coordinates and let egui handle pixel alignment at the current display scale. This works for standalone windows, browser canvases, and hosted editor tabs.

```rust,ignore
use joy_engine::runtime::ui_layout::{self, taffy::prelude::*};

// Create once in the owning view; keep these node IDs with the tree.
let mut tree: TaffyTree = TaffyTree::new();
tree.disable_rounding();
let inspector = tree.new_leaf(Style {
    size: Size { width: length(260.0), height: percent(1.0) },
    min_size: Size { width: length(0.0), height: length(0.0) },
    ..Default::default()
})?;
let preview = tree.new_leaf(Style {
    flex_grow: 1.0,
    min_size: Size { width: length(0.0), height: length(0.0) },
    ..Default::default()
})?;
let root = tree.new_with_children(Style::default(), &[preview, inspector])?;

// During the egui pass, compute against the host's current size.
let area = ui.available_rect_before_wrap();
tree.set_style(root, Style {
    size: Size { width: length(area.width()), height: length(area.height()) },
    ..Default::default()
})?;
tree.compute_layout(root, Size::MAX_CONTENT)?;
let rect = ui_layout::node_rect(&tree, root, inspector, area.min)?;
ui_layout::show_region(ui, "inspector", rect, |ui| {
    ui.label("Properties");
    // Existing egui controls, with their usual state and event handling.
});
```

`show_region` clips painting and hit tests to the region and parent, and contributes the actual content extent to the parent's layout. Use normal egui popup/window APIs for overlays and `ScrollArea` inside a bounded region for long content; Taffy does not implement scrolling, docking, splitter gestures, or widget events. Separate the scroll viewport's layout constraints from its content's intrinsic size.

## Measured Content
Taffy does not know font metrics. Text leaves can hold their text/font context and use `ui_layout::measure_text` inside `taffy::compute_leaf_layout` in a `compute_layout_with_measure` callback. The callback receives known dimensions and available space; definite widths wrap using egui's active fonts, while max-content measures unwrapped text. Min-content requests egui's narrowest wrapping rather than a separate browser text-layout implementation.

Measurement runs inside an active egui pass, but must not draw interactive widgets or mutate document state. Taffy may measure a node more than once. Mark a measured leaf dirty when its text, font, or font definitions change; resizing invalidates the affected constraints through the normal Taffy computation. egui remains responsible for measuring validation messages and other dynamic content inside the resulting regions.

## Editor Integration
`PanelLayout` retains a four-node Flexbox tree: root, left pane, preview, and right pane. It owns saved width preferences, visibility, and divider behavior separately from geometry. Its `panes(&mut self, area)` updates cached geometry without changing document data or saved preferences. FORM, JoyFX, and Joy Editor all consume it.

`ComponentGroup` uses native Grid tracks for XYZ/RGBA columns and wraps to additional rows when necessary. egui measures row height, including invalid-draft messages. Only solved track geometry is cached in egui's shared memory: Taffy trees are not `Send + Sync`, so they stay local to the UI owner or computation, with no unsafe trait implementations. Each component's egui ID stays stable across row changes.

Palette, typography, spacing, and control styling continue to come from the shared [UI assets](../assets/ui/README.md). These remain build-time assets. Taffy adds neither CSS stylesheet parsing nor theme hot reload; those are separate authoring-workflow capabilities.
