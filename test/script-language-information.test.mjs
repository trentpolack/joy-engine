// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import test from 'node:test';
import { FORM_COMMAND, FORM_KEYWORD } from '../src/form/constants.ts';
import { FORM_LANGUAGE_CATALOG } from '../src/form/language-information.ts';
import { PARTICLE_LANGUAGE_CATALOG } from '../src/particles/language-information.ts';
import { PARTICLE_FIELDS, PARTICLE_FUNCTION_ARITIES, PARTICLE_INPUTS } from '../src/particles/script.ts';

test('FORM language information covers every built-in command and keyword', () => {
  const names = new Set(FORM_LANGUAGE_CATALOG.symbols.map(symbol => symbol.name));
  assert.equal(names.size, FORM_LANGUAGE_CATALOG.symbols.length, 'FORM symbols should be unique');
  for(const name of [...Object.values(FORM_COMMAND), ...Object.values(FORM_KEYWORD)]) {
    assert.equal(names.has(name), true, `missing FORM language symbol ${name}`);
  }
});

test('particle language information describes every runtime function, field, and input', () => {
  const symbols = new Map(PARTICLE_LANGUAGE_CATALOG.symbols.map(symbol => [symbol.name, symbol]));
  for(const name of Object.keys(PARTICLE_FUNCTION_ARITIES)) {
    assert.equal(symbols.get(name)?.kind, 'function', `missing particle function ${name}`);
  }
  for(const name of PARTICLE_FIELDS) {
    assert.equal(symbols.get(name)?.writable, true, `particle field ${name} should be writable`);
  }
  for(const name of PARTICLE_INPUTS) {
    assert.equal(symbols.get(name)?.writable, false, `particle input ${name} should be read-only`);
  }
});
