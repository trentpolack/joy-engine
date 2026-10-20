// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { findWorkspaceRoot, isEditorEnabled, readEditorPolicy } from '../../tooling/editor-policy.mjs';

test('production policy preserves independent lab flags and hosted editor requirements', async t => {
  const root = await mkdtemp(join(tmpdir(), 'joy-editor-policy-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const config = join(root, 'joy.joyconfig');
  assert.deepEqual(await readEditorPolicy(root), { joyEditor: false, formLab: true, particleLab: true });

  await writeFile(config, JSON.stringify({ editors: { formLab: { productionEnabled: false } } }));
  const disabled = await readEditorPolicy(root);
  assert.equal(isEditorEnabled('form-lab', disabled), false);
  assert.equal(isEditorEnabled('particle-lab', disabled), true);
  assert.equal(isEditorEnabled('game', disabled), true);
  assert.deepEqual(await readEditorPolicy(root, true), { joyEditor: true, formLab: true, particleLab: true });

  await writeFile(config, JSON.stringify({ editors: {
    joyEditor: { productionEnabled: true },
    formLab: { productionEnabled: false },
    particleLab: { productionEnabled: false }
  } }));
  assert.deepEqual(await readEditorPolicy(root), { joyEditor: true, formLab: true, particleLab: true });

  for(const editors of [{ joyEdtor: {} }, { joyEditor: { productionEnabled: 'false' } }]) {
    await writeFile(config, JSON.stringify({ editors }));
    await assert.rejects(readEditorPolicy(root), /Unknown editor setting|must be a boolean/);
  }
  await writeFile(config, '{');
  await assert.rejects(readEditorPolicy(root), SyntaxError);
});

test('policy discovers the hosting workspace from nested packages', async t => {
  const root = await mkdtemp(join(tmpdir(), 'joy-editor-root-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const nested = join(root, 'engine', 'editor', 'tools');
  await mkdir(nested, { recursive: true });
  assert.equal(findWorkspaceRoot(nested), null);
  await writeFile(join(root, 'joy.joyconfig'), '{}');
  assert.equal(findWorkspaceRoot(nested), root);
});
