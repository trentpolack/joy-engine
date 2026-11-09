// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Retained Taffy nodes for the editor's three panes. Preferences and interactions stay in `PanelLayout`.

use joy_ui::ui_layout::{
    node_rect,
    taffy::prelude::*,
};

/// Geometry cache; nodes retain their identities across resizes and visibility changes.
#[derive(Clone, Debug)]
pub(super) struct PaneGeometry {
    tree: TaffyTree,
    root: NodeId,
    panes: [NodeId; 3],
}

impl PaneGeometry {
    pub(super) fn new() -> Self {
        let mut tree = TaffyTree::with_capacity(4);
        // egui handles physical pixel rounding after scaling; Taffy works in logical points.
        tree.disable_rounding();
        let panes = std::array::from_fn(|_| tree.new_leaf(Style::default()).expect("new pane node"));
        let root = tree.new_with_children(Style::default(), &panes).expect("new pane root");
        Self {
            tree,
            root,
            panes,
        }
    }

    pub(super) fn compute(&mut self, area: egui::Rect, widths: [f32; 2], hidden: [bool; 3]) -> [Option<egui::Rect>; 3] {
        let size = Size {
            width: area.width().max(0.0),
            height: area.height().max(0.0),
        };
        self.set_style(
            self.root,
            Style {
                size: Size {
                    width: length(size.width),
                    height: length(size.height),
                },
                ..Default::default()
            },
        );
        for (index, preferred) in [widths[0], 0.0, widths[1]].into_iter().enumerate() {
            self.set_style(
                self.panes[index],
                Style {
                    display: if hidden[index] { Display::None } else { Display::Flex },
                    flex_basis: length(if hidden[1] { 0.0 } else { preferred }),
                    flex_grow: if hidden[1] {
                        preferred
                    } else if index == 1 {
                        1.0
                    } else {
                        0.0
                    },
                    // Under extreme constraints the sides shrink instead of extending beyond their host.
                    min_size: Size {
                        width: length(0.0),
                        height: length(0.0),
                    },
                    size: Size {
                        width: auto(),
                        height: percent(1.0),
                    },
                    ..Default::default()
                },
            );
        }
        self.tree.compute_layout(self.root, size.map(AvailableSpace::Definite)).expect("retained pane root");
        std::array::from_fn(|index| {
            (!hidden[index])
                .then(|| node_rect(&self.tree, self.root, self.panes[index], area.min).expect("pane belongs to root"))
        })
    }

    fn set_style(&mut self, node: NodeId, style: Style) {
        if self.tree.style(node).expect("retained pane node") != &style {
            self.tree.set_style(node, style).expect("retained pane node");
        }
    }
}
