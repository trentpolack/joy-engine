// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

for(const scenario of ['clean', 'three-way', 'conflict', 'invalid delivery']) {
  test(`standalone npm patch workflow handles ${scenario} without changing staging or refs`, async t => {
    const { root, git, run, original, incoming, bundle, diffPath } = await createFixture(t, scenario);
    const source = join(root, 'sample.txt');
    const local = scenario === 'conflict'
      ? original.replace('line 3\n', 'local line 3\n')
      : original.replace('line 5\n', 'local line 5\n');
    if(scenario === 'three-way' || scenario === 'conflict') {
      await writeFile(source, local);
    }
    // A staged change unrelated to the incoming patch must survive fetching and applying.
    await writeFile(join(root, 'local.txt'), 'staged local work\n');
    git(root, 'add', 'local.txt');
    const index = git(root, 'ls-files', '--stage');
    const head = git(root, 'rev-parse', 'HEAD');
    const before = await readFile(source, 'utf8');

    const fetched = run('fetch-joy-engine-patch', '--diff', diffPath);
    assert.equal(git(root, 'ls-files', '--stage'), index);
    assert.equal(git(root, 'rev-parse', 'HEAD'), head);
    assert.equal(await readFile(source, 'utf8'), before);
    if(scenario === 'invalid delivery') {
      assert.notEqual(fetched.status, 0);
      assert.match(fetched.stderr, /manifest/i);
      await assert.rejects(readFile(join(root, '.patches/joy-engine.patch')), {code: 'ENOENT'});
      return;
    }
    assert.equal(fetched.status, 0, fetched.stderr);
    for(const [name, contents] of Object.entries(bundle)) {
      assert.equal(await readFile(join(root, '.patches', name), 'utf8'), contents);
    }
    assert.equal(git(root, 'check-ignore', '.patches/joy-engine.patch').trim(), '.patches/joy-engine.patch');

    const applied = run('apply-joy-engine-patch', '--no-mergetool');
    assert.equal(git(root, 'ls-files', '--stage'), index);
    assert.equal(git(root, 'rev-parse', 'HEAD'), head);
    if(scenario === 'conflict') {
      assert.notEqual(applied.status, 0);
      assert.equal(await readFile(source, 'utf8'), local);
      for(const [name, contents] of Object.entries(bundle)) {
        assert.equal(await readFile(join(root, '.patches', name), 'utf8'), contents);
      }
      return;
    }
    assert.equal(applied.status, 0, applied.stderr);
    const expected = scenario === 'three-way' ? incoming.replace('line 5\n', 'local line 5\n') : incoming;
    assert.equal(await readFile(source, 'utf8'), expected);
    assert.deepEqual(await readFile(join(root, 'new.bin')), Buffer.from([0, 255, 1]));
    await assert.rejects(readFile(join(root, 'deleted.txt')), {code: 'ENOENT'});
    assert.deepEqual(await readdir(join(root, '.patches')), []);
    const repeated = run('apply-joy-engine-patch', '--no-mergetool');
    assert.equal(repeated.status, 0, repeated.stderr);
    assert.match(repeated.stdout, /No pending Joy Engine patch/);
  });
}

/**
 * Copy only engine-owned tooling and its real npm scripts into a standalone repository.
 * All staging and history changes below belong to temporary fixtures, never the working checkout.
 */
async function createFixture(t, scenario) {
  const directory = await mkdtemp(join(tmpdir(), 'joy-standalone-patch-'));
  t.after(() => rm(directory, {recursive: true, force: true}));
  const root = join(directory, 'engine');
  const cloud = join(directory, 'cloud');
  await mkdir(root);
  await mkdir(join(cloud, '.patches'), {recursive: true});
  await cp(new URL('../scripts/', import.meta.url), join(root, 'scripts'), {recursive: true});
  await cp(new URL('../package.json', import.meta.url), join(root, 'package.json'));
  await writeFile(join(root, '.gitignore'), '.patches/\n');

  function git(repository, ...arguments_) {
    const result = spawnSync('git', ['-C', repository, ...arguments_], {encoding: 'utf8'});
    assert.equal(result.status, 0, result.stderr);
    return result.stdout;
  }

  for(const repository of [root, cloud]) {
    git(repository, 'init', '--quiet');
    git(repository, 'config', 'user.name', 'Patch Test');
    git(repository, 'config', 'user.email', 'patch-test@example.invalid');
    git(repository, 'config', 'commit.gpgsign', 'false');
  }
  const original = Array.from({length: 20}, (_, index) => `line ${index + 1}\n`).join('');
  const incoming = original.replace('line 3\n', 'incoming line 3\n');
  await writeFile(join(root, 'sample.txt'), original);
  await writeFile(join(root, 'deleted.txt'), 'old file\n');
  git(root, 'add', '--all');
  git(root, 'commit', '-qm', 'Created fixture base');
  const base = git(root, 'rev-parse', 'HEAD');
  await writeFile(join(root, 'sample.txt'), incoming);
  await writeFile(join(root, 'new.bin'), Buffer.from([0, 255, 1]));
  await rm(join(root, 'deleted.txt'));
  git(root, 'add', '--all');
  const bundle = {
    'joy-engine.patch': git(root, 'diff', '--cached', '--binary', '--full-index'),
    'joy-engine-base.txt': base,
    'joy-engine-manifest.txt': scenario === 'invalid delivery' ? 'sample.txt\n' : 'deleted.txt\nnew.bin\nsample.txt\n',
    'joy-engine-tests.txt': 'Fixture validation report\n'
  };
  git(root, 'reset', '--hard', base.trim());
  for(const [name, contents] of Object.entries(bundle)) {
    await writeFile(join(cloud, '.patches', name), contents);
  }
  git(cloud, 'add', '--all');
  const diffPath = join(directory, 'task.diff');
  await writeFile(diffPath, git(cloud, 'diff', '--cached', '--binary', '--full-index'));
  return {
    root, git, original, incoming, bundle, diffPath,
    run: (command, ...arguments_) => spawnSync('npm', ['run', command, '--', ...arguments_], {
      cwd: root,
      encoding: 'utf8',
      env: {...process.env, npm_config_cache: join(directory, 'npm-cache')}
    })
  };
}
