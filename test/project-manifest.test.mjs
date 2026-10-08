// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import test from 'node:test';
import { parseProjectManifest } from '../src/project/project-manifest.ts';

test('project manifests reject unsafe paths and invalid schema metadata', () => {
  const minimal = {schemaVersion: 1, name: 'Test', preview: {root: 'dist', entry: 'index.html'}, capabilities: []};
  assert.equal(Object.hasOwn(parseProjectManifest(minimal), '$schema'), false);
  assert.throws(() => parseProjectManifest({...minimal, $schema: {type: 'object'}}), /schema.*string/i);
  assert.throws(() => parseProjectManifest({...minimal, $schema: ' '}), /schema.*non-empty/i);
  assert.throws(() => parseProjectManifest({
    schemaVersion: 1,
    name: 'Unsafe',
    preview: {root: '../outside', entry: 'index.html'},
    capabilities: []
  }), /stay inside/);
});
