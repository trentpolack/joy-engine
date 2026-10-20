# Patch Delivery
The engine owns its patch tooling and can fetch and apply cloud bundles from a standalone checkout. Run these commands from the engine repository root; they use its ignored `.patches/` directory:

```bash
npm run fetch-joy-engine-patch -- <task-id>
npm run apply-joy-engine-patch
```

Fetching a task requires an authenticated Codex CLI. To select an attempt or use a previously saved task diff:

```bash
npm run fetch-joy-engine-patch -- <task-id> --attempt 2
npm run fetch-joy-engine-patch -- --diff /path/to/task.diff
npm run apply-joy-engine-patch -- --no-mergetool
```

The saved diff must contain the exported bundle, not just ordinary source changes. `--no-mergetool` makes conflicts fail without opening an editor. Otherwise, application uses the repository's configured Git merge tool in a disposable checkout. An unresolved merge retains that checkout and prints its path.

## Bundle Contract
A delivery contains four files:

| File | Contents |
| --- | --- |
| `joy-engine.patch` | Git patch with engine-relative paths, including binary changes and new/deleted authored files. |
| `joy-engine-base.txt` | Full engine commit SHA recorded at the start of the task. |
| `joy-engine-manifest.txt` | One engine-relative changed path per line, matching the patch. |
| `joy-engine-tests.txt` | Validation results and any limitations. |

The patch includes all task changes relative to the recorded base, including changes already committed in the cloud. Exclude dependencies, generated build output, secrets, transport files, and submodule pointer changes. The [agent delivery rules](../AGENTS.md#joy-engine-cloud-patch-delivery) describe cloud preparation and transport. Keep bundles ignored locally and out of source history.

Fetch extracts only these four files, checks their completeness and manifest, and verifies application against the recorded base in a disposable clone. The base commit must already exist locally; fetch does not download Git history. A different pending patch is preserved, while stale reports without a pending patch can be replaced.

Apply preserves local staging and refs. Compatible edits are reconciled in a disposable snapshot before a complete conflict-free delta is applied to working files. Never use `git apply --3way` in the real checkout: that operation also changes the index. Successful or already-applied deliveries remove all four bundle files; failures retain them. Other deliveries remain untouched.

## Hosting Workspaces
JoyGames uses the same engine-owned implementation through the Node-only `joy-engine/development/patches` export. A host supplies its repository paths, delivery root, target name, and protected paths; the engine tooling imports no host-specific code.

| Command Location | Patched Repository | Bundle Location |
| --- | --- | --- |
| Engine root, standalone or nested | That engine repository | Engine root's `.patches/` |
| JoyGames root (apply) | `joy-engine/` | JoyGames root's `.patches/` and the engine root's `.patches/` |

JoyGames keeps its existing `fetch-joy-engine-patch`, `apply-joy-engine-patch`, and `apply-patches` commands. Applying from a host checks its configured delivery directory and the source repository's `.patches/`. Duplicate patches produce a warning listing both paths. Byte-identical copies apply once, and all four bundle files in each location are removed only after success or already-applied detection. Different patches fail before changing source or bundles; keep the intended bundle in one location before retrying. Conflicts retain every copy, and unrelated files remain untouched. Running a command inside the nested engine still uses only the engine's own delivery directory; there is no ancestor-directory search.
