// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { dirname, isAbsolute, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// Ask the compiler which files the engine program pulls in (including JSDoc and
// type-only imports) and fail if any lives outside the engine's own sources or an
// installed dependency. Paths resolve from this package rather than process.cwd(),
// so the check behaves the same in the JoyGames monorepo and a standalone checkout.

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SOURCE_ROOT = 'src/';
const DEPENDENCY_SEGMENT = 'node_modules/';

// Resolve TypeScript the way Node would from this package; in the monorepo it is hoisted.
// The package only exports its manifest, so locate the declared bin beside it.
const require = createRequire(import.meta.url);
const typescriptManifest = require.resolve('typescript/package.json');
const compiler = resolve(dirname(typescriptManifest), require(typescriptManifest).bin.tsc);

const result = spawnSync(process.execPath, [
  compiler, '--project', 'tsconfig.json', '--listFilesOnly'
], { cwd: packageRoot, encoding: 'utf8' });

if(result.error || result.status !== 0) {
  console.error(result.error ?? result.stdout + result.stderr);
  process.exit(1);
}

const unexpectedFiles = result.stdout.trim().split(/\r?\n/).filter((file) => {
  const localPath = relative(packageRoot, file).replaceAll('\\', '/');
  if(isAbsolute(localPath)) {
    return true;
  }
  if(localPath.startsWith(SOURCE_ROOT)) {
    return false;
  }

  // Installed dependencies may sit in this package or, under npm workspaces, an ancestor.
  return !(localPath.startsWith(DEPENDENCY_SEGMENT) || localPath.includes(`/${DEPENDENCY_SEGMENT}`));
});

if(unexpectedFiles.length > 0) {
  console.error('Engine imports must stay within the engine and its dependencies:');
  console.error(unexpectedFiles.join('\n'));
  process.exit(1);
}

console.log('Engine dependency boundary checked (including JSDoc imports).');
