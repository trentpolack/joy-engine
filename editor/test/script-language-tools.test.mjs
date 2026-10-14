// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import { createScriptCompletionOptions, findScriptSymbol } from '../src/script-language-tools.ts';

const catalog = Object.freeze({
  id: 'test',
  symbols: Object.freeze([
    Object.freeze({name: 'let', kind: 'keyword', detail: 'Keyword', documentation: 'Declares a value.'}),
    Object.freeze({name: 'noise', kind: 'function', detail: 'noise(value)', documentation: 'Returns noise.', signatures: Object.freeze(['noise(value)'])})
  ])
});

test('shared script tooling creates documented keyword and function completions', () => {
  const options = createScriptCompletionOptions(catalog);
  assert.deepEqual(options.map(option => option.label), ['let', 'noise']);
  assert.equal(options[0].type, 'keyword');
  assert.equal(options[0].info, 'Declares a value.');
  assert.equal(options[1].type, 'function');
  assert.equal(typeof options[1].apply, 'function');
});

test('shared script tooling resolves exact catalog symbols for hover adapters', () => {
  assert.equal(findScriptSymbol(catalog, 'noise')?.detail, 'noise(value)');
  assert.equal(findScriptSymbol(catalog, 'no'), undefined);
});
