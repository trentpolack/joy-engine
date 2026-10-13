# PARTICLE LAB
This workspace contains the playable project, its project-owned content, and its build configuration.

See the [project documentation](https://github.com/trentpolack/JoyGames/blob/develop/docs/projects/particle-lab/README.md) for the application overview, controls, architecture, development workflow, and validation guidance.

### Live preview application
Live applies valid definition edits after the existing debounce. Turning it off cancels queued automatic work and prevents an automatic preparation already in flight from replacing the preview. Explicit Apply remains available with Live off, and enabling Live applies pending edits. Invalid drafts stay separate from the current valid definition and accepted preview; repair them before Apply or export. Run `npm run test:live-browser --workspace=@joy-games/particle-lab` for the queued/in-flight/manual application regression, using `CAPTURE_EXECUTABLE` for a locally installed Chrome.

### Parameter Editing
Bounded scalars pair a slider with an exact numeric field. XYZ and RGBA components stay together in compact groups; Bounds remains available beneath each parameter. Color swatches display linear RGB in sRGB, and the native picker converts an explicit RGB selection back to linear within existing bounds while preserving alpha. Exact fields preserve their full authored precision. Property Undo/Redo includes picker choices, and Shift/Alt + arrow keys retain fine/coarse editing. A rejected numeric draft must be repaired before a picker choice or export can replace accepted values.
