// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The Documentation panel's guide (the TypeScript workspace's help dialog), as titled sections of paragraphs.

/// Guide sections: a title and its paragraphs.
pub const SECTIONS: [(&str, &[&str]); 5] = [
    (
        "New and Import",
        &[
            "New creates a starter asset in this project. Choose FORM LAB document (.formlab) to keep the script, parameter overrides, materials and variants together. Level, Object and Particle effect create their respective documents.",
            "Import copies an existing .form, .formlab, .joyfx, .joylevel or .joyobject file into the project without replacing project content. A raw .form keeps its script format: parameter sliders affect its preview; edit script defaults to persist values. Saved Inspector material edits and variants require .formlab.",
            "GLB geometry can be loaded into a FORM mesh parameter. That geometry-only import does not preserve a full scene or its materials; workspace Import accepts the editable formats listed above.",
        ],
    ),
    (
        "Preview and Save",
        &[
            "A preview accepting your changes means the tool can display them. Save File (Ctrl/Command+S) writes them to the project file. Save preserves the document format; it does not export or convert it. Save source assets to refresh dependent previews.",
            "External edits refresh clean tabs. When a tab has unsaved changes, Use Disk Version discards them and Keep My Edits keeps them for an explicit save. Replaced files are kept in a .joy-editor-recovery folder beside the asset.",
        ],
    ),
    (
        "Content Browser",
        &[
            "Single-click or Enter opens an asset. Rename through the row actions menu, secondary click, or F2. Rename and delete leave closed files closed; the trash action confirms deletion. Save or discard edits before renaming or deleting an open document.",
            "The Filter button combines checked asset types; All types clears restrictions. Refresh rescans the list immediately; Cmd/Ctrl+P reveals and focuses search, including from a tool.",
        ],
    ),
    (
        "Level authoring",
        &[
            "Add boxes, spheres, lights and empty entities, then select them in the viewport or the outliner. MOVE shows a gizmo: drag an axis or the plane handle, or click the ground. Transform fields use meters and degrees; rotations are stored as radians.",
            "Components come from the engine's component schemas: + ADD lists presets by category, + FIELD adds an omitted field at its runtime default, and advisory issues appear inside the component. Invalid input blocks saving and selection changes; Advanced Source remains available for unusual data and repair.",
        ],
    ),
    (
        "Canvas focus and cameras",
        &[
            "Click the viewport before using camera shortcuts. Drag to orbit; Shift-drag, right-drag or middle-drag to pan; scroll to zoom. With the viewport focused, F frames the content and 0 resets the camera.",
        ],
    ),
];
