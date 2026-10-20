# Repository Audit
`npm run audit:repo` scans this checkout independently and writes `artifacts/audit/audit.html` and `audit.json`. It requires Python 3.10+ and Git; it does not require Rust, Node dependencies, a game checkout, or a hosting repository. npm is just a convenience wrapper:

```bash
python3 -B scripts/audit/audit.py
npm run audit:repo -- --fail-on-issues
npm run test:audit
```

## Schema Validation
Install the optional validator in your Python environment to enable JSON Schema checks and their fixture tests:

```bash
python3 -m pip install -r scripts/audit/requirements.txt
```

Without it, schema validation and schema-specific tests explicitly report a skip. `--skip-schema-validation` also skips validation. Other checks still run. Schemas and their references resolve offline from tracked local files and registered schema IDs; unknown external references require review. No dependency installation or network retrieval runs automatically.

## Checks and Interpretation
The tool checks tracked generated outputs, assets without literal filename references, legacy renderer dependencies, literal package script and export targets, local Markdown destinations, test declarations, and authoring schemas/documents.

Mechanical failures are `issue` findings. Small tests, similar test bodies, possible unused assets, and historical dependencies are `review` findings: inspect their consumers and protected behavior before removing anything. The audit never deletes files or executes repository builds, tests, package scripts, or Git hooks. It reads current tracked working-tree files; ignored and untracked files are excluded. Import caches are disabled, and only the selected report output is written. Running again replaces those report files.

Test value, configuration semantics, dynamic references, and parity between JSON schemas and Rust loaders still need human review. The engine's layer policy lives in the Cargo manifests and is enforced by `npm run check`, not by the audit. Node test counts represent source declarations, not expanded runtime cases.

## Options and Exit Codes
- `--root PATH`: checkout to scan; defaults to the engine containing this script, regardless of the current directory.
- `--output PATH`: report directory; defaults to the selected checkout's `artifacts/audit`.
- `--private-repo owner/name`: flag public Markdown URLs into this private GitHub repository. Repeat for multiple repositories. No private repository policy is embedded in the engine, and URL privacy is not queried from GitHub.
- `--json-stdout`: emit the JSON report without writing files; mutually exclusive with `--output`.
- `--skip-schema-validation`: explicitly skip the optional schema checks.
- `--fail-on-issues`: exit 1 for mechanical issues; review candidates do not fail the command.

Exit 0 means the scan completed, not that every check passed. Exit 2 means an argument or operational failure. Always inspect `schema_validation` and `skipped_paths`; `--fail-on-issues` does not convert skipped validation into a failure.

## Consumer Integration
The engine CLI scans exactly one engine repository. A consuming repository can execute it with `--json-stdout`, then use `scripts/audit/checks.py` to scan its own files and `report.py` to render combined findings. `Audit(repository, schema_repository=..., path_mappings=...)` supplies shared schemas and explicit local path mappings without adding engine findings to the consumer scan. Engine schemas are counted by the engine audit; consumer documents are counted by the consumer audit. Asset and similar-test comparisons stay within each owning repository.

JSON reports have `format_version: 2`. Consumers should check it before combining results. Keep consumer policy, entry points, and integration fixtures in the consumer repository. The engine's `tests/tooling/test_audit.py` suite creates disposable repositories and can run without any consumer source. Audit tests have their own `test:audit` command so optional Python tooling does not become a prerequisite for the Rust/Node suites.
