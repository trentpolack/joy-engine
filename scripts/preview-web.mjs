// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { resolve } from 'node:path';
import { parseArgs } from 'node:util';
import { preview } from 'vite';

const { values } = parseArgs({ options: {
  directory: { type: 'string', default: process.cwd() },
  host: { type: 'string', default: '127.0.0.1' },
  port: { type: 'string', default: '4173' }
} });

const server = await preview({
  root: resolve(values.directory),
  configFile: false,
  preview: { host: values.host, port: Number(values.port) }
});
server.printUrls();
