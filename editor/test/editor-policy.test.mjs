// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { build, createServer, preview } from 'vite';
import { createEditorPolicyPlugin } from '../tooling/editor-policy-plugin.mjs';
import { findWorkspaceRoot } from '../tooling/editor-policy.mjs';

test('editor policy gates direct builds and stale previews while development stays enabled', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'joy-editor-build-'));
  const configPath = path.join(root, 'joy.joyconfig');
  const config = slug => ({
    configFile: false,
    root,
    logLevel: 'silent',
    plugins: [createEditorPolicyPlugin(slug, root)]
  });
  try {
    await writeFile(path.join(root, 'entry.js'), `export default {
      form: import.meta.env.JOY_FORM_LAB_ENABLED,
      particle: import.meta.env.JOY_PARTICLE_LAB_ENABLED
    };`);
    const buildTool = slug => build({
      ...config(slug),
      build: {write: false, lib: {entry: path.join(root, 'entry.js'), formats: ['es']}}
    });
    await assert.rejects(buildTool('joy-editor'), /production build is disabled/);
    for(const scenario of [
      {editors: {formLab: {productionEnabled: false}}, disabled: 'form-lab', active: 'particle-lab', want: {form: false, particle: true}},
      {editors: {particleLab: {productionEnabled: false}}, disabled: 'particle-lab', active: 'form-lab', want: {form: true, particle: false}},
      {editors: {joyEditor: {productionEnabled: true}, formLab: {productionEnabled: false}, particleLab: {productionEnabled: false}}, active: 'joy-editor', want: {form: true, particle: true}}
    ]) {
      await writeFile(configPath, JSON.stringify({editors: scenario.editors}));
      if(scenario.disabled) {
        await assert.rejects(buildTool(scenario.disabled), /production build is disabled/);
      }
      const result = await buildTool(scenario.active);
      const bundle = result[0].output.find(output => output.type === 'chunk');
      const compiled = await import(`data:text/javascript;base64,${Buffer.from(bundle.code).toString('base64')}`);
      assert.deepEqual(compiled.default, scenario.want);
    }
    await writeFile(configPath, JSON.stringify({editors: {joyEditor: {productionEnabled: false}}}));
    const development = await createServer({...config('joy-editor'), mode: 'production', server: {middlewareMode: true}});
    try {
      assert.equal(development.config.define['import.meta.env.JOY_FORM_LAB_ENABLED'], 'true');
    } finally {
      await development.close();
    }
    await mkdir(path.join(root, 'dist'));
    await writeFile(path.join(root, 'dist', 'index.html'), 'stale editor');
    const stalePreview = await preview({...config('joy-editor'), preview: {host: '127.0.0.1', port: 0}});
    try {
      const response = await fetch(stalePreview.resolvedUrls.local[0]);
      assert.equal(response.status, 404);
      assert.doesNotMatch(await response.text(), /stale editor/);
    } finally {
      await new Promise((resolve, reject) => stalePreview.httpServer.close(error => error ? reject(error) : resolve()));
    }
    for(const editors of [{joyEditor: {productionEnabled: 'false'}}, {joyEdtor: {productionEnabled: true}}]) {
      await writeFile(configPath, JSON.stringify({editors}));
      await assert.rejects(buildTool('joy-editor'), /must be a boolean|Unknown editor setting/);
    }
  } finally {
    await rm(root, {recursive: true, force: true});
  }
});

test('editor policy locates the hosting workspace from nested editor directories', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'joy-editor-workspace-'));
  try {
    const nested = path.join(root, 'joy-editor', 'tools', 'form-lab', 'site');
    await mkdir(nested, {recursive: true});
    assert.equal(findWorkspaceRoot(nested), null);
    await writeFile(path.join(root, 'joy.joyconfig'), '{}');
    assert.equal(findWorkspaceRoot(nested), root);
  } finally {
    await rm(root, {recursive: true, force: true});
  }
});
