# Changelog

This changelog summarizes meaningful work on Joy Engine, Joy Editor, FORM LAB, PARTICLE LAB and the engine examples. Entries are grouped by day, newest first, using America/Detroit dates. Work before 10-08-2026 happened inside [JoyGames](https://github.com/trentpolack/JoyGames); its [CHANGELOG](https://github.com/trentpolack/JoyGames/blob/develop/CHANGELOG.md) is the historical record for that period.

## 10-08-2026
### Repository
- Clarified the agent workflow for inclusion in a larger workspace: complete file edits and validation together, leave submodule Git state to the user, and keep agent instructions independent of private hosting repositories.
- Established this repository from the Joy Engine and Joy Editor history in JoyGames (`joy-engine/` became the root, `joy-editor/` became `editor/`, the engine guide moved to `docs/`). Joy Engine 0.12.0 and Joy Editor 0.6.0 are the starting versions.
- Made the root package the npm workspace root for Joy Editor, FORM LAB, PARTICLE LAB and the examples, with its own lockfile, `.gitignore`, `AGENTS.md` house rules and coding standards. JoyGames consumes the repository as a git submodule at `joy-engine/`.
