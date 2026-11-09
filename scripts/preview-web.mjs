// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Serve a built browser output folder for manual preview (`npm run preview` in a game, tool, or example).

import { resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { startStaticServer } from './static-server.mjs';

const { values } = parseArgs({ options: {
  directory: { type: 'string', default: process.cwd() },
  'out-dir': { type: 'string', default: 'dist' },
  host: { type: 'string', default: '127.0.0.1' },
  port: { type: 'string', default: '4173' }
} });

const root = resolve(values.directory, values['out-dir']);
const server = await startStaticServer({ root, host: values.host, port: Number(values.port) });
console.log(`Previewing ${root} at ${server.url}`);

// The server lives until the process is interrupted.
process.on('SIGINT', async () => {
  await server.close();
  process.exit(0);
});
