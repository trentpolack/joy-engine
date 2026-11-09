// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Grid tracks for inspector component rows. egui measures their actual height, including validation messages.

use joy_ui::ui_layout::taffy::prelude::*;

/// Minimum useful component width, in logical points, before a group wraps onto another row.
pub(super) const MINIMUM_COMPONENT_WIDTH: f32 = 48.0;

// egui's shared memory requires Send + Sync; Taffy trees intentionally stay on the UI thread.
// Cache only the solved tracks here. Small trees are rebuilt when their constraints change.
#[derive(Clone, Default)]
struct ComponentTracks {
    width: f32,
    gap: f32,
    columns: usize,
    tracks: Vec<(f32, f32)>,
}

/// Compute equal Grid tracks for one component row. The group's egui ID owns its transient geometry cache.
pub(super) fn component_tracks(ui: &egui::Ui, count: usize) -> Vec<(f32, f32)> {
    let width = ui.available_width().max(0.0);
    let gap = ui.spacing().item_spacing.x;
    let columns = (((width + gap) / (MINIMUM_COMPONENT_WIDTH + gap)).floor() as usize).max(1).min(count.max(1));
    let cache_id = ui.id().with("component-grid-layout");
    ui.data_mut(|data| {
        let cache = data.get_temp_mut_or_default::<ComponentTracks>(cache_id);
        if cache.columns != columns || cache.width != width || cache.gap != gap {
            *cache = ComponentTracks {
                width,
                gap,
                columns,
                tracks: compute_tracks(columns, width, gap),
            };
        }
        cache.tracks.clone()
    })
}

fn compute_tracks(count: usize, width: f32, gap: f32) -> Vec<(f32, f32)> {
    let mut tree: TaffyTree = TaffyTree::with_capacity(count + 1);
    tree.disable_rounding();
    let columns: Vec<_> = (0..count).map(|_| tree.new_leaf(Style::default()).expect("new component column")).collect();
    let root = tree
        .new_with_children(
            Style {
                display: Display::Grid,
                size: Size {
                    width: length(width),
                    height: length(0.0),
                },
                grid_template_columns: vec![minmax(length(0.0), fr(1.0)); count],
                gap: Size {
                    width: length(gap),
                    height: length(0.0),
                },
                ..Default::default()
            },
            &columns,
        )
        .expect("new component grid");
    tree.compute_layout(root, Size::MAX_CONTENT).expect("component grid root");
    columns
        .iter()
        .map(|node| {
            let layout = tree.layout(*node).expect("component column");
            (layout.location.x, layout.size.width)
        })
        .collect()
}
