# Copyright (c) 2026 Trent Polack. All Rights Reserved.
# Licensed under the MIT License.

"""JSON and HTML presentation for standalone or combined audit reports."""

from collections import Counter
import html
import json


def render_html(report):
    escape = lambda value: html.escape(str(value))
    counts = Counter(f['kind'] for f in report['findings'])
    body = '<h1>Repository Maintenance Audit</h1><p>' + escape(report['generated_at']) + '</p>'
    body += '<p>' + escape(report['scope']) + '</p>'
    body += f'<p><strong>{counts["issue"]} issues · {counts["review"]} review candidates</strong></p>'
    body += '<p>Issues are mechanical check failures. Review candidates need context; this tool never recommends automatic deletion.</p>'
    body += '<h2>Repositories</h2><ul>'
    for repo in report['repositories']:
        body += '<li><strong>' + escape(repo['name']) + '</strong>: ' + escape(repo['root']) + ' · ' + escape(repo['head'] or 'no commit') + f' · {repo["tracked_entries"]} tracked entries</li>'
    body += '</ul><h2>Schema Validation</h2><pre>' + escape(json.dumps(report['schema_validation'], indent=2)) + '</pre>'
    body += '<h2>Findings</h2><table><thead><tr><th>Kind / Rule</th><th>Source</th><th>Finding</th></tr></thead><tbody>'
    for finding in report['findings']:
        location = finding['repository'] + '/' + finding['path'] + (f':{finding["line"]}' if finding.get('line') else '')
        body += '<tr><td>' + escape(finding['kind']) + '<br><code>' + escape(finding['rule']) + '</code></td><td><code>' + escape(location) + '</code></td><td>' + escape(finding['message'])
        more = {k: v for k, v in finding.items() if k not in {'kind', 'rule', 'repository', 'path', 'line', 'message'}}
        if more:
            body += '<details><summary>Evidence</summary><pre>' + escape(json.dumps(more, indent=2)) + '</pre></details>'
        body += '</td></tr>'
    body += '</tbody></table><h2>Manual Review Still Required</h2><ul>'
    body += ''.join('<li>' + escape(item) + '</li>' for item in report['manual_review']) + '</ul>'
    body += '<h2>Coverage Limits</h2><ul>' + ''.join('<li>' + escape(item) + '</li>' for item in report['limits']) + '</ul>'
    body += '<details><summary>Test inventory: ' + str(len(report['tests'])) + ' source declarations</summary><pre>' + escape(json.dumps(report['tests'], indent=2)) + '</pre></details>'
    body += '<details><summary>Scan metadata and skipped paths</summary><pre>' + escape(json.dumps({'repositories': report['repositories'], 'statistics': report['statistics']}, indent=2)) + '</pre></details>'
    style = 'body{font:15px/1.6 system-ui;color:#182a36;max-width:1200px;margin:auto;padding:32px}h1{font-size:36px}table{border-collapse:collapse;width:100%}th,td{border:1px solid #d6dfe4;padding:12px;text-align:left;vertical-align:top}th{background:#edf3f5}code,pre{font:12px/1.6 monospace;overflow-wrap:anywhere}pre{white-space:pre-wrap;background:#f5f7f8;padding:12px}details{margin:12px 0}summary{cursor:pointer}li{margin:8px 0}'
    return '<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Repository Maintenance Audit</title><style>' + style + '</style></head><body>' + body + '</body></html>'


def write_report(report, output):
    """Write reports only after all requested scans have completed successfully."""
    output.mkdir(parents=True, exist_ok=True)
    (output / 'audit.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    (output / 'audit.html').write_text(render_html(report), encoding='utf-8')
    counts = Counter(f['kind'] for f in report['findings'])
    print(f'{counts["issue"]} issues; {counts["review"]} review candidates; {len(report["tests"])} test declarations.')
    print(f'Schema validation: {report["schema_validation"]["status"]}')
    print(f'HTML: {(output / "audit.html").resolve()}')
    print(f'JSON: {(output / "audit.json").resolve()}')


def exit_status(report, fail_on_issues):
    return int(fail_on_issues and any(f['kind'] == 'issue' for f in report['findings']))
