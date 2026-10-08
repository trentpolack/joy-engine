# PARTICLE LAB
This workspace contains the playable project, its project-owned content, and its build configuration.

See the [project documentation](https://github.com/trentpolack/JoyGames/blob/develop/docs/projects/particle-lab/README.md) for the application overview, controls, architecture, development workflow, and validation guidance.

### Live preview application
Live applies valid definition edits after the existing debounce. Turning it off cancels queued automatic work and prevents an automatic preparation already in flight from replacing the preview. Explicit Apply remains available with Live off, and enabling Live applies pending edits. Invalid drafts stay separate from the current valid definition and accepted preview; repair them before Apply or export. Run `npm run test:live-browser --workspace=@joy-games/particle-lab` for the queued/in-flight/manual application regression, using `CAPTURE_EXECUTABLE` for a locally installed Chrome.
