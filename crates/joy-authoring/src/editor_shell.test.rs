// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

use super::*;

#[test]
fn hidden_rows_give_their_height_to_the_body() {
    let area = Rect::from_min_size(pos2(10.0, 20.0), vec2(1200.0, 800.0));
    let mut shell = ShellLayout::new();

    let full = shell.rows(area, true, true);
    assert_eq!(full.header.min, area.min);
    assert_eq!(full.status.max, area.max);
    let toolbar = full.toolbar.expect("toolbar shown");
    assert_eq!(full.body.min.y, toolbar.max.y);
    assert_eq!(full.body.height(), 800.0 - HEADER_HEIGHT - TABS_HEIGHT - TOOLBAR_HEIGHT - STATUS_HEIGHT);

    // With no documents open, the tab strip and toolbar collapse and the body starts under the header.
    let empty = shell.rows(area, false, false);
    assert!(empty.tabs.is_none() && empty.toolbar.is_none());
    assert_eq!(empty.body.min.y, empty.header.max.y);
    assert_eq!(empty.body.max.y, empty.status.min.y);
}
