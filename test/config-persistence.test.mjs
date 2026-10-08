// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { createServer } from 'vite';
import { createConfigPlugin } from '../src/development/config-plugin.ts';

const fields = [{ key: 'speed', label: 'Speed', group: 'Movement', description: 'World units per tick.', defaultValue: 9, min: 1, max: 20, step: 0.1, units: 'units/tick', applyMode: 'live' }];

test('config endpoint saves only valid overrides and protects concurrent edits', async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'joy-config-'));
  const file = path.join(root, 'tuning-overrides.json');
  const initial = { version: 1, values: {} };
  await writeFile(file, JSON.stringify(initial));
  const server = await createServer({ root, configFile: false, base: '/games/test/', plugins: [createConfigPlugin({ file, fields })], server: { host: '127.0.0.1', port: 0 } });
  try {
    await server.listen();
    const address = server.httpServer.address();
    const origin = `http://127.0.0.1:${address.port}`;
    const url = `${origin}/games/test/__joy/config`;
    const get = () => fetch(url).then(response => response.json());
    const put = (body, requestOrigin = origin) => fetch(url, { method: 'PUT', headers: { 'content-type': 'application/json', 'x-joy-config': '1', origin: requestOrigin }, body: JSON.stringify(body) });
    const original = await get();
    assert.deepEqual(original.values, { speed: 9 });
    const invalid = await put({ revision: original.revision, values: { speed: 30 } });
    assert.equal(invalid.status, 400);
    assert.deepEqual(JSON.parse(await readFile(file, 'utf8')), initial);
    assert.equal((await put({ revision: original.revision, values: { unknown: 1 } })).status, 400);
    assert.equal((await put({ revision: original.revision, values: { speed: 10 } }, 'https://other.example')).status, 403);
    assert.equal((await fetch(url, { method: 'PUT', body: '{}' })).status, 403);
    const writes = await Promise.all([put({ revision: original.revision, values: { speed: 10 } }), put({ revision: original.revision, values: { speed: 12 } })]);
    assert.deepEqual(writes.map(response => response.status).sort(), [200, 409]);
    const winningValue = writes[0].status === 200 ? 10 : 12;
    assert.equal(JSON.parse(await readFile(file, 'utf8')).values.speed, winningValue);
    const saved = await get();
    await writeFile(file, JSON.stringify({ version: 1, values: { speed: 15 } }));
    assert.equal((await put({ revision: saved.revision, values: { speed: 11 } })).status, 409);
    assert.equal((await get()).values.speed, 15);
    await writeFile(file, '{bad json');
    assert.equal((await fetch(url)).status, 400);
  } finally {
    await server.close();
    await rm(root, { recursive: true, force: true });
  }
});
