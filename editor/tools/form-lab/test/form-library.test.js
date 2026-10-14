// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import test from 'node:test';
import assert from 'node:assert/strict';
import {defaultMaterial} from 'joy-engine/form';
import {createFormLibrary, openFormLibraryAsset} from '../src/form-library.ts';

test('the FORM library opens independent script and document copies only from its authored asset roots', () => {
  const document = {
    version: 1, name: 'Stored take', source: 'point(0,0,0)', overrides: {height: 3.5},
    materials: [defaultMaterial()], variants: [{name: 'Tall', overrides: {height: 6}}]
  };
  const assets = createFormLibrary({
    'assets/form/structures/study.formlab': JSON.stringify(document),
    'assets/examples/nested/recipe.form': '# Authored source\npoint(1,2,3)\n',
    'assets/form/ignored.txt': 'Not a FORM asset.',
    'assets/meshes/ignored.form': 'Outside the requested library.',
    'assets/form/../private.form': 'Not a library path.'
  });
  assert.deepEqual(assets.map(asset => asset.path), ['assets/examples/nested/recipe.form', 'assets/form/structures/study.formlab']);
  assert.equal(assets[0].folder, 'assets/examples/nested');
  assert.equal(openFormLibraryAsset(assets[0]).source, '# Authored source\npoint(1,2,3)\n');
  const opened = openFormLibraryAsset(assets[1]);
  assert.deepEqual(opened, document);
  opened.overrides.height = 9;
  opened.materials[0].baseColor[0] = 0;
  opened.variants[0].overrides.height = 10;
  assert.deepEqual(openFormLibraryAsset(assets[1]), document, 'Opening a library asset again must not inherit authoring mutations.');
});
