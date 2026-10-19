#!/usr/bin/env python3
# Copyright (c) 2026 Trent Polack. All Rights Reserved.
# Licensed under the MIT License.

"""Behavior checks for the standalone audit CLI; all repositories are disposable."""

import importlib.util
import json
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[2] / 'scripts/audit/audit.py'


class AuditCliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='joy audit fixtures ')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.engine = self.root / 'standalone-engine'
        self.engine.mkdir()
        subprocess.run(['git', 'init', '-q', str(self.engine)], check=True)

    def write(self, root, name, value):
        path = root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value) if isinstance(value, dict) else value)

    def stage(self):
        subprocess.run(['git', '-C', str(self.engine), 'add', '.'], check=True)

    def run_audit(self, *extra):
        self.assertTrue(SCRIPT.is_file(), 'The reusable audit CLI has not been implemented yet.')
        result = subprocess.run([
            sys.executable, '-B', str(SCRIPT), '--root', str(self.engine),
            '--private-repo', 'example/private-host', '--output', str(self.root / 'reports'), *extra,
        ], text=True, capture_output=True)
        self.assertIn(result.returncode, (0, 1), result.stderr)
        report_path = self.root / 'reports' / 'audit.json'
        self.assertTrue(report_path.exists(), result.stderr)
        report = json.loads(report_path.read_text())
        return result, report

    def test_live_checks_clear_after_fixes_without_modifying_sources(self):
        # Copying old findings or executing repository commands would fail this workflow.
        self.write(self.engine, 'package.json', {'scripts': {'fetch': 'node scripts/retired/fetch.mjs'}})
        self.write(self.engine, 'projects/demo/dist-rust/demo.js', 'old generated output')
        self.write(self.engine, 'README.md', '[Private](https://github.com/example/private-host/blob/main/docs/guide.md)\n[Missing](absent.md)\n')
        self.write(self.engine, 'scripts/patches/fetch.mjs', 'throw new Error("audit must not execute me");')
        self.stage()
        snapshots = {p: p.read_bytes() for p in self.engine.rglob('*') if p.is_file()}
        result, report = self.run_audit('--skip-schema-validation', '--fail-on-issues')
        self.assertEqual(result.returncode, 1)
        rules = {f['rule'] for f in report['findings']}
        self.assertTrue({'missing_script_target', 'private_documentation_link', 'broken_doc_link', 'tracked_generated_output'} <= rules)
        self.assertEqual([r['name'] for r in report['repositories']], ['joy-engine'])
        for path, content in snapshots.items():
            self.assertEqual(path.read_bytes(), content, str(path))
        self.write(self.engine, 'package.json', {'scripts': {'fetch': 'node scripts/patches/fetch.mjs'}})
        self.write(self.engine, 'README.md', 'Public documentation.\n')
        _, second = self.run_audit('--skip-schema-validation')
        self.assertFalse({'missing_script_target', 'private_documentation_link', 'broken_doc_link'} & {f['rule'] for f in second['findings']})
        self.assertEqual(second['schema_validation']['status'], 'skipped')

    @unittest.skipUnless(importlib.util.find_spec('jsonschema'), 'Optional jsonschema dependency is not installed')
    def test_schema_validation_resolves_local_refs_and_reports_invalid_data(self):
        self.write(self.engine, 'schemas/joylevel.schema.json', {
            '$schema': 'http://json-schema.org/draft-07/schema#', 'type': 'object',
            'required': ['values'], 'properties': {'values': {'$ref': 'vector.schema.json'}},
        })
        self.write(self.engine, 'schemas/vector.schema.json', {
            '$schema': 'http://json-schema.org/draft-07/schema#', 'type': 'array', 'minItems': 3,
        })
        self.write(self.engine, 'projects/demo/assets/main.joylevel', {'values': [1]})
        self.stage()
        _, report = self.run_audit()
        self.assertEqual(report['schema_validation']['status'], 'completed')
        self.assertEqual(report['schema_validation']['documents_checked'], 1)
        self.assertEqual(report['schema_validation']['documents_failed'], 1)
        self.assertTrue(any(f['rule'] == 'invalid_document' for f in report['findings']))
        self.write(self.engine, 'projects/demo/assets/main.joylevel', {'values': [1, 2, 3]})
        _, second = self.run_audit()
        self.assertEqual(second['schema_validation']['documents_failed'], 0)
        self.assertFalse(any(f['rule'] == 'invalid_document' for f in second['findings']))
        self.write(self.engine, 'projects/demo/assets/main.joylevel', 'null')
        _, third = self.run_audit()
        self.assertEqual(third['schema_validation']['documents_failed'], 1)
        # Retrieval paths and canonical schema IDs can differ. Both must retain
        # the same base when checking references and validating documents.
        self.write(self.engine, 'projects/demo/assets/main.joylevel', {'values': [1]})
        (self.engine / 'schemas/vector.schema.json').unlink()
        for root_id, child_id in (
            ('https://schema.invalid/base/root.json', 'https://schema.invalid/base/vector.schema.json'),
            ('nested/root.json', '../nested/vector.schema.json'),
        ):
            self.write(self.engine, 'schemas/joylevel.schema.json', {
                '$schema': 'http://json-schema.org/draft-07/schema#', '$id': root_id,
                'type': 'object', 'properties': {'values': {'$ref': 'vector.schema.json'}},
            })
            self.write(self.engine, 'schemas/resources/vector.schema.json', {
                '$schema': 'http://json-schema.org/draft-07/schema#', '$id': child_id,
                'type': 'array', 'minItems': 3,
            })
            self.stage()
            _, canonical = self.run_audit()
            self.assertEqual(canonical['schema_validation']['documents_failed'], 1)
            self.assertEqual(canonical['schema_validation']['documents_unvalidated'], 0)
        self.write(self.engine, 'schemas/joylevel.schema.json', {
            '$schema': 'http://json-schema.org/draft-07/schema#',
            '$ref': 'resources/vector.schema.json',
        })
        self.write(self.engine, 'schemas/resources/vector.schema.json', {
            '$schema': 'http://json-schema.org/draft-07/schema#',
            '$ref': 'https://example.invalid/external.schema.json',
        })
        _, external = self.run_audit()
        references = [f for f in external['findings'] if f['rule'] == 'unresolved_schema_reference']
        self.assertTrue(references)
        self.assertTrue(all(f['kind'] == 'review' for f in references))

    def test_repo_hooks_and_ancestor_symlinks_are_never_executed_or_read(self):
        self.write(self.engine, 'docs/guide.md', 'Safe tracked content')
        self.stage()
        outside = self.root / 'outside'
        outside.mkdir()
        self.write(outside, 'guide.md', '[Outside](https://github.com/example/private-host/blob/main/private.md)')
        shutil.rmtree(self.engine / 'docs')
        (self.engine / 'docs').symlink_to(outside, target_is_directory=True)
        marker = self.root / 'hook-executed'
        hook = self.root / 'fsmonitor.py'
        hook.write_text(f'#!{sys.executable}\nfrom pathlib import Path\nPath({str(marker)!r}).write_text("ran")\nprint("\\0")\n')
        hook.chmod(0o755)
        subprocess.run(['git', '-C', str(self.engine), 'config', 'core.fsmonitor', shlex.quote(str(hook))], check=True)
        _, report = self.run_audit('--skip-schema-validation')
        self.assertFalse(marker.exists(), 'Git query executed a repository-configured hook')
        self.assertFalse(any(f['rule'] == 'private_documentation_link' for f in report['findings']))
        self.assertTrue(any(p['path'] == 'docs/guide.md' for r in report['repositories'] for p in r['skipped_paths']))

    def test_unsupported_shell_context_and_malformed_config_do_not_abort_audit(self):
        self.write(self.engine, 'package.json', {'scripts': {'build': 'cd sub && node task.mjs'}})
        self.write(self.engine, 'sub/task.mjs', 'throw new Error("must not execute");')
        self.write(self.engine, 'tools/package.json', {'scripts': None, 'dependencies': None})
        self.write(self.engine, 'schemas/broken.schema.json', {'$schema': 123, 'type': 'object'})
        self.stage()
        _, report = self.run_audit()
        self.assertFalse(any(f['rule'] == 'missing_script_target' for f in report['findings']))
        self.assertTrue(any(f['rule'] == 'script_target_review' for f in report['findings']))
        if report['schema_validation']['status'] == 'completed':
            self.assertTrue(any(f['rule'] == 'invalid_schema' for f in report['findings']))

    def test_test_candidates_are_heuristics_and_resource_checks_are_preserved(self):
        self.write(self.engine, 'src/core/mod.rs', '''
#[test]
fn title_is_capitalized() { assert_eq!(title("hi"), "Hi"); }
#[test]
fn failed_save_preserves_document() { assert_eq!(save_after_failure(), original()); }
#[test]
fn labels_with_braces() { assert_eq!(label(r#"}"#), "{"); }
''')
        self.stage()
        _, report = self.run_audit('--skip-schema-validation')
        self.assertEqual(len(report['tests']), 3)
        candidates = [f for f in report['findings'] if f['rule'] == 'small_test_review']
        self.assertEqual({f['test'] for f in candidates}, {'title_is_capitalized', 'labels_with_braces'})
        self.assertTrue(all(f['kind'] == 'review' for f in candidates))
        self.assertIn('manual_review', report)
        self.assertNotIn('<script', (self.root / 'reports/audit.html').read_text())


if __name__ == '__main__':
    unittest.main()
