// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { resolve } from 'node:path';
import { fetchProjectPatch } from './fetch-project-patch.mjs';
import { createPatchTarget } from './patch-delivery.mjs';

const ROOT_DIRECTORY = resolve(import.meta.dirname, '../..');
const target = createPatchTarget({
  id: 'joy-engine',
  label: 'Joy Engine',
  rootDirectory: ROOT_DIRECTORY
});

try {
  await fetchProjectPatch(target);
} catch(error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}
