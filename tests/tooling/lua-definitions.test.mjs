// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, readFile, rm, stat, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import path from 'node:path';
import {DefinitionGenerator, updateDefinition} from '../../scripts/lua-definitions.mjs';

test('definition checks reject missing and stale files without changing them; generation preserves unchanged files', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'joy-lua-definitions-'));
  const output = path.join(directory, 'library', 'engine.d.lua');
  try {
    await assert.rejects(updateDefinition(output, 'first\n', true), /Missing/);
    await updateDefinition(output, 'first\n', false);
    const before = (await stat(output)).mtimeMs;
    await updateDefinition(output, 'first\n', false);
    assert.equal((await stat(output)).mtimeMs, before);
    await assert.rejects(updateDefinition(output, 'second\n', true), /Stale/);
    assert.equal(await readFile(output, 'utf8'), 'first\n');
    await updateDefinition(output, 'second\n', false);
    await updateDefinition(output, 'second\n', true);
    await writeFile(output, 'manual edit\n');
    await assert.rejects(updateDefinition(output, 'second\n', true), /Stale/);
  } finally {
    await rm(directory, {recursive: true, force: true});
  }
});


test('an edit queued during a failed export regenerates the newest definitions', async () => {
  const directory = await mkdtemp(path.join(tmpdir(), 'joy-lua-retry-'));
  const output = path.join(directory, 'engine.d.lua');
  let rejectFirst;
  const first = new Promise((_resolve, reject) => { rejectFirst = reject; });
  class ControlledExporter extends DefinitionGenerator {
    async export() {
      if(this.first === undefined) {
        this.first = false;
        return first;
      }
      return '---@meta\n-- newest API\n';
    }
  }
  const generator = new ControlledExporter([{output}], false);
  try {
    const running = generator.generate();
    await generator.generate();
    rejectFirst(new Error('source failed before the queued fix'));
    await running;
    assert.equal(await readFile(output, 'utf8'), '---@meta\n-- newest API\n');
  } finally {
    generator.close();
    await rm(directory, {recursive: true, force: true});
  }
});
