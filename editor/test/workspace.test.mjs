// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, readFile, symlink, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { ProjectFiles } from '../server/project-files.mjs';
import { WorkspaceDocument } from '../site/src/workspace/documents.ts';

test('project saves preserve bytes, reject stale/concurrent writes and confine access', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'joy-workspace-'));
  try {
    await mkdir(path.join(root, 'projects/demo/assets/form'), {recursive: true});
    await writeFile(path.join(root, 'projects/demo/package.json'), JSON.stringify({name: 'demo'}));
    const assetPath = path.join(root, 'projects/demo/assets/form/tree.form');
    await writeFile(assetPath, 'box(1, 2, 3);\n');
    const files = new ProjectFiles(root);
    assert.equal((await files.listProjects())[0].id, 'demo');
    assert.equal((await files.listFiles('demo'))[0].path, 'assets/form/tree.form');
    // A hosting workspace with another layout names its projects directory explicitly.
    await mkdir(path.join(root, 'games/other/assets'), {recursive: true});
    await writeFile(path.join(root, 'games/other/package.json'), JSON.stringify({name: 'other'}));
    const relocated = new ProjectFiles(root, {projectsDirectory: 'games'});
    assert.deepEqual((await relocated.listProjects()).map(project => project.id), ['other']);
    await assert.rejects(relocated.listFiles('demo'));
    const original = await files.read('demo', 'assets/form/tree.form');
    const outcomes = await Promise.allSettled([
      files.save('demo', 'assets/form/tree.form', 'first\n', original.revision),
      files.save('demo', 'assets/form/tree.form', 'second\n', original.revision)
    ]);
    assert.equal(outcomes.filter(result => result.status === 'fulfilled').length, 1);
    assert.equal(await readFile(assetPath, 'utf8'), 'first\n');
    const baseline = await files.read('demo', 'assets/form/tree.form');
    const read = files.read.bind(files);
    let checks = 0;
    files.read = async (...args) => {
      const disk = await read(...args);
      if(++checks === 2) {
        await writeFile(assetPath, 'external during save');
      }
      return disk;
    };
    await assert.rejects(files.save('demo', 'assets/form/tree.form', 'workspace', baseline.revision), /changed/i);
    assert.equal(await readFile(assetPath, 'utf8'), 'external during save');
    files.read = read;
    await assert.rejects(files.save('demo', 'assets/form/tree.form', 'old', original.revision), /changed/i);
    await assert.rejects(files.read('demo', '../package.json'));
    await assert.rejects(files.read('../demo', 'assets/form/tree.form'));
    await writeFile(path.join(root, 'secret.form'), 'private');
    await symlink(path.join(root, 'secret.form'), path.join(root, 'projects/demo/assets/form/link.form'));
    await assert.rejects(files.read('demo', 'assets/form/link.form'));
    await assert.rejects(files.save('demo', 'assets/form/tree.form', 'x'.repeat(6_000_001), original.revision));
    assert.deepEqual((await files.listFiles('demo')).map(file => file.path), ['assets/form/tree.form']);
  } finally {
    await rm(root, {recursive: true, force: true});
  }
});

test('manifests, asset lifecycle and recovery copies stay project-owned', async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), 'joy-workspace-lifecycle-'));
  const external = await mkdtemp(path.join(os.tmpdir(), 'joy-external-'));
  const manifest = {
    schemaVersion: 1,
    name: 'External Demo',
    preview: {root: 'dist', entry: 'index.html'},
    capabilities: [{id: 'notes', label: 'Notes', extensions: ['.note'], assetRoot: 'assets', renderer: 'text', features: {}}]
  };
  try {
    await mkdir(path.join(external, 'assets'), {recursive: true});
    await writeFile(path.join(external, 'project.joyproject'), JSON.stringify(manifest));
    await writeFile(path.join(external, 'assets/example.note'), 'first');
    const files = new ProjectFiles(root);
    const project = await files.registerProject(external);
    assert.equal(project.name, 'External Demo');
    assert.equal(project.external, true);
    assert.deepEqual(await files.listFiles(project.id), [{path: 'assets/example.note', kind: 'note'}]);

    const original = await files.read(project.id, 'assets/example.note');
    await files.save(project.id, 'assets/example.note', 'second', original.revision);
    assert.equal((await files.listRecovery(project.id)).length, 1);
    await files.move(project.id, 'assets/example.note', 'assets/renamed.note');
    await files.create(project.id, 'assets/created.note', 'created');
    assert.deepEqual((await files.listFiles(project.id)).map(item => item.path), ['assets/created.note', 'assets/renamed.note']);
    await files.remove(project.id, 'assets/created.note');
    assert.equal((await files.removeRecovery(project.id)).removed, 1);
    assert.deepEqual(await files.listRecovery(project.id), []);
  } finally {
    await rm(root, {recursive: true, force: true});
    await rm(external, {recursive: true, force: true});
  }
});

test('document conflicts and edits during save retain every unsaved change', () => {
  const doc = new WorkspaceDocument('demo', 'tree.form', {text: 'original', revision: 'a'});
  doc.edit('first');
  const snapshot = doc.snapshot();
  doc.edit('second');
  doc.saved(snapshot, {text: 'first', revision: 'b'});
  assert.equal(doc.text, 'second');
  assert.equal(doc.dirty, true);
  doc.external({text: 'external', revision: 'c'});
  assert.equal(doc.text, 'second');
  assert.equal(doc.conflict?.text, 'external');
  doc.keepLocal();
  assert.equal(doc.revision, 'c');
  assert.equal(doc.text, 'second');
  doc.saved(doc.snapshot(), {text: 'second', revision: 'd'});
  assert.equal(doc.dirty, false);
  doc.external({text: 'VS Code', revision: 'e'});
  assert.equal(doc.text, 'VS Code');
  doc.edit('local');
  doc.external({text: 'disk', revision: 'f'});
  doc.reloadDisk();
  assert.equal(doc.text, 'disk');
  assert.equal(doc.dirty, false);
});
