import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtemp, realpath, readFile, writeFile, rm, symlink } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { createServer } from 'vite';
import { createPostProcessingPlugin } from '../src/development/post-processing-plugin.ts';

test('development profile writes validate revisions, origins, and fixed paths', async () => {
  const directory = await realpath(await mkdtemp(path.join(os.tmpdir(), 'joy-post-')));
  const file = path.join(directory, 'main.joylevel');
  const profile = { version: 1, preset: 'default', overrides: { exposure: 1 } };
  await writeFile(file, JSON.stringify({version: 1, name: 'Main', entities: [], postProcessing: profile}));
  const server = await createServer({ root: directory, configFile: false, logLevel: 'silent',
    plugins: [createPostProcessingPlugin({ profilePath: file })], server: { port: 0, host: '127.0.0.1' } });
  await server.listen();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  const url = `${origin}/__joy/post-processing`;
  const save = (body, extra = {}) => fetch(url, { method: 'POST', headers: {
    'Content-Type': 'application/json', 'X-Joy-Postprocessing': '1', Origin: origin, ...extra,
  }, body: JSON.stringify(body) });
  try {
    const initial = await (await fetch(url)).json();
    assert.deepEqual(initial.profile, profile);
    const changed = { ...profile, overrides: { exposure: 2 } };
    assert.equal((await save({ profile: changed, revision: initial.revision }, { Origin: 'https://evil.example' })).status, 403);
    assert.equal((await save({ profile: changed, revision: initial.revision }, { Origin: 'null' })).status, 403);
    assert.equal((await save({ profile: changed, revision: initial.revision, path: '/tmp/other' })).status, 400);
    assert.equal((await save({ profile: { ...profile, overrides: { exposure: -2 } }, revision: initial.revision })).status, 400);
    assert.equal((await save({ profile: changed, revision: initial.revision })).status, 200);
    assert.equal(JSON.parse(await readFile(file, 'utf8')).postProcessing.overrides.exposure, 2);
    assert.equal((await save({ profile, revision: initial.revision })).status, 409);
    const current = await (await fetch(url)).json();
    // Both contenders must change the current content. A no-op save can
    // legitimately retain its content-based revision and allow the next writer.
    const alternate = { ...profile, overrides: { exposure: 3 } };
    const parallel = await Promise.all([save({ profile, revision: current.revision }), save({ profile: alternate, revision: current.revision })]);
    assert.deepEqual(parallel.map(response => response.status).sort(), [200, 409]);
    await writeFile(file, '{');
    assert.equal((await fetch(url)).status, 400);
    assert.equal((await save({ profile, revision: current.revision })).status, 400);
    assert.equal(await readFile(file, 'utf8'), '{');
    await rm(file);
    const outside = path.join(directory, 'other.json');
    await writeFile(outside, JSON.stringify({version: 1, name: 'Main', entities: [], postProcessing: profile}));
    await symlink(outside, file);
    assert.equal((await fetch(url)).status, 400);
  } finally {
    await server.close();
    await rm(directory, { recursive: true, force: true });
  }
});
