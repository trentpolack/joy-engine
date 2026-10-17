#!/usr/bin/env python3
# Copyright (c) 2026 Trent Polack. All Rights Reserved.
# Licensed under the MIT License.

"""Audit a standalone engine checkout without builds, cleanup, or network access.

Requires Python 3.10+ and Git. Optional jsonschema>=4.18 enables offline schema
validation. Scans tracked working-tree files; review suggestions need judgment.
Exit 0: scan complete; 1: --fail-on-issues found issues; 2: operational failure.
"""

import argparse
import json
from pathlib import Path
import subprocess
import sys

# A read-only scan must not leave import caches in the engine checkout.
sys.dont_write_bytecode = True
from checks import Audit, Repository
from report import exit_status, write_report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[2], help='Engine checkout root')
    output = parser.add_mutually_exclusive_group()
    output.add_argument('--output', type=Path, help='Report directory; defaults to ROOT/artifacts/audit')
    output.add_argument('--json-stdout', action='store_true', help='Emit JSON only, without writing files (for consumers)')
    parser.add_argument('--private-repo', action='append', default=[], help='Private GitHub owner/repo to flag; repeatable')
    parser.add_argument('--skip-schema-validation', action='store_true')
    parser.add_argument('--fail-on-issues', action='store_true')
    args = parser.parse_args()
    try:
        repository = Repository('joy-engine', args.root)
        report = Audit(repository, private_repositories=args.private_repo, architecture=True).scan(args.skip_schema_validation)
        if args.json_stdout:
            print(json.dumps(report))
        else:
            write_report(report, args.output or repository.root / 'artifacts/audit')
        return exit_status(report, args.fail_on_issues)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f'Audit failed: {error}', file=sys.stderr)
        return 2


if __name__ == '__main__':
    raise SystemExit(main())
