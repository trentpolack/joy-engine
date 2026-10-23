// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// The FORM guide's keyword list and built-in table must match the engine's language catalog, which also drives the
// evaluator's built-in names, completion, and hover text. A stale guide documents functions the evaluator refuses.

import assert from 'node:assert/strict';
import test from 'node:test';
import { generatedGuide } from '../../scripts/form-reference.mjs';

test('the FORM reference lists every keyword and built-in function of the language catalog', () => {
  const { current, expected } = generatedGuide();
  assert.equal(current, expected, 'docs/rust-form.md is stale; run node scripts/form-reference.mjs --write');
});
