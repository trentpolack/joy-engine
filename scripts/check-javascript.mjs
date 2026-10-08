#!/usr/bin/env node
// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Syntax gate for JavaScript that TypeScript does not type-check (launchers, tooling
// and Node tests). Published as the `joy-check-javascript` package bin so games and
// tools share one implementation instead of copying it.

import { readdirSync, statSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawnSync } from 'node:child_process';

const roots = process.argv.slice(2);

if(roots.length === 0) {
  console.error('Usage: joy-check-javascript <file-or-directory> [...]');
  process.exit(1);
}

/** Collect ES modules and CommonJS launchers for the same syntax gate. */
function findJavaScriptFiles(path) {
  const absolutePath = resolve(path);

  if(!statSync(absolutePath).isDirectory()) {
    return /\.(?:js|mjs|cjs)$/.test(absolutePath)
      ? [absolutePath]
      : [];
  }

  return readdirSync(absolutePath, { withFileTypes: true }).flatMap((entry) =>
    findJavaScriptFiles(resolve(absolutePath, entry.name)),
  );
}

const files = roots.flatMap(findJavaScriptFiles);

for(const file of files) {
  const result = spawnSync(process.execPath, ['--check', file], {
    stdio: 'inherit',
  });

  if(result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

console.log(`Checked ${files.length} JavaScript files.`);
