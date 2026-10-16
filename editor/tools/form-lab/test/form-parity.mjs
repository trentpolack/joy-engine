// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Run the complete original FORM corpus against recorded reference outputs without loading a retired runtime.
// Build joy-form-evaluate first, or pass --binary to select an existing evaluator.

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { gunzipSync } from 'node:zlib';
import { compareData } from './form-parity-comparison.mjs';

const engineRoot = fileURLToPath(new URL('../../../../', import.meta.url));
const { values } = parseArgs({ options: {
  binary: { type: 'string' },
  output: { type: 'string', default: 'artifacts/form-parity' }
} });
const metadata = values.binary ? null : JSON.parse(execFileSync('cargo', [
  'metadata', '--no-deps', '--format-version', '1', '--manifest-path', resolve(engineRoot, 'Cargo.toml')
], { encoding: 'utf8' }));
const binary = resolve(values.binary ?? resolve(metadata.target_directory, 'debug', `joy-form-evaluate${process.platform === 'win32' ? '.exe' : ''}`));
const recording = JSON.parse(gunzipSync(readFileSync(new URL('fixtures/form-reference.json.gz', import.meta.url))));
const results = [];

// Protect tolerance and exact topology semantics before accepting a comparison.
assert.equal(compareData({ positions: [1] }, { positions: [1 + 1e-9] }), null);
assert.ok(compareData({ positions: [1] }, { positions: [1 + 1e-6] }));
assert.ok(compareData({ faces: [[0, 1, 2]] }, { faces: [[0, 1, 2 + 1e-9]] }));

for(const item of recording.cases) {
  const request = { source: item.source, overrides: item.overrides, ...(item.materials === undefined ? {} : { materials: item.materials }) };
  try {
    const line = `${JSON.stringify(request)}\n`;
    const output = execFileSync(binary, {
      input: line + line, encoding: 'utf8', maxBuffer: 128*1024*1024, timeout: 30000
    });
    const responses = output.trim().split('\n').map(text => JSON.parse(text));
    assert.equal(responses.length, 2, 'Two requests produce two responses.');
    const [first, second] = responses.map(({ evaluation_ms, ...result }) => result);
    assert.deepEqual(first, second, 'Repeated evaluations are deterministic.');
    assert.equal(first.ok, item.result.ok, 'Reference acceptance matches.');
    if(first.ok) {
      assert.equal(compareData(item.result.data, first.data), null);
    } else {
      assert.ok(first.message?.length, 'Rejections include a diagnostic.');
      assert.equal(first.line, item.result.line);
      assert.equal(first.column, item.result.column);
    }
    results.push({ id: item.id, passed: true });
    console.log(`PASS ${item.id}`);
  } catch(error) {
    results.push({ id: item.id, passed: false, error: String(error.message).slice(0, 2000) });
    console.error(`FAIL ${item.id}: ${results.at(-1).error}`);
  }
}

const output = resolve(values.output);
mkdirSync(output, { recursive: true });
const failed = results.filter(result => !result.passed).length;
writeFileSync(resolve(output, 'results.json'), JSON.stringify({
  referenceRevision: recording.sourceRevision,
  total: results.length,
  failed,
  results
}, null, 2));
console.log(`FORM parity: ${results.length - failed}/${results.length} passed. Evidence: ${output}`);
process.exitCode = failed ? 1 : 0;
