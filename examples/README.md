# Joy Engine examples
Small feature demonstrations live beside the engine and consume its public `joy-engine` package. Each owns its assets, controls, checks, and cleanup. They remain npm workspaces for standalone development and root checks/tests, while the JoyGames platform presents them in a dedicated **Joy Engine Examples** section.

| Example | Demonstrates | Platform route |
| --- | --- | --- |
| [Terrain](terrain/README.md) | Resident indexed CLOD terrain and fly/orbit/pan controls | `/examples/terrain/` |
| [Scene Effects](scene-effects/README.md) | Lighting, shadows, and post-processing inspection | `/examples/scene-effects/` |
| [FORM Building](form-building/README.md) | Editable procedural recipes and baked building assets | `/examples/form-building/` |
| [FORM Destruction](form-destruction/README.md) | Surface-projected impacts and synchronized chunk removal | `/examples/form-destruction/` |

## Run
From the JoyGames root, replace `form-destruction` with any example folder name:
```bash
npm run dev:example:form-destruction
npm run build:example:form-destruction
npm run capture:example:form-destruction
npm run capture:example:form-destruction -- --built
```
From the standalone engine repository, use the workspace command, for example `npm run dev --workspace @joy-engine/example-form-destruction`.

## Rust Versions
Scene Effects, FORM Building, and FORM Destruction also have Rust versions on the Rust engine: a `main.rs` composition root at the example root and Rust modules beside the TypeScript modules in `src/`, registered as `[[example]]` targets in `Cargo.toml`. Run one with `cargo run --example scene-effects` or build it for browsers with `node scripts/rust/build-web.mjs --example scene-effects`; each README's Rust Version section lists differences. Their headless tests in `test/rust/` include the same module trees.

## Hosting and adding examples
Add examples under `examples/<feature>/` with a private npm package, a README, focused checks, explicit cleanup, and a browser capture command. To publish one, add `gameSite` metadata with `category: "example"`, a title, description, status, start date, and optional preview filename. JoyGames discovers that metadata for both development and static export at `/examples/<feature>/`. Published examples join the host's automatic builds and release captures; examples without site metadata remain local.

Capture commands accept an optional `--preview-directory <absolute-path>` supplied by the hosting workspace. After a successful production capture (`--built`), they copy the representative image there using the metadata's screenshot filename. Without that option, captures only produce local evidence. The engine does not import hosting-workspace scripts or depend on its directory layout for runtime behavior.

Promote stable mechanisms into `src/` when at least two consumers need them. Engine runtime modules must never import example code.
