// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {readFileSync, readdirSync} from 'node:fs';
import {createRequire} from 'node:module';
import {dirname, extname, isAbsolute, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sourceRoot = resolve(packageRoot, 'src');
const architecture = JSON.parse(readFileSync(resolve(packageRoot, 'architecture/layers.json'), 'utf8'));
const sourceExtensions = new Set(['.ts', '.js', '.mjs']);

checkPackageBoundary();
checkLayerDirection();

/** Ensure the compiler cannot pull project or editor sources into the engine package. */
function checkPackageBoundary() {
  const require = createRequire(import.meta.url);
  const typescriptManifest = require.resolve('typescript/package.json');
  const compiler = resolve(dirname(typescriptManifest), require(typescriptManifest).bin.tsc);
  const result = spawnSync(process.execPath, [compiler, '--project', 'tsconfig.json', '--listFilesOnly'], {cwd: packageRoot, encoding: 'utf8'});
  if(result.error || result.status !== 0) {
    console.error(result.error ?? result.stdout + result.stderr);
    process.exit(1);
  }
  const unexpectedFiles = result.stdout.trim().split(/\r?\n/).filter(file => {
    const localPath = relative(packageRoot, file).replaceAll('\\', '/');
    return isAbsolute(localPath)
      || (!localPath.startsWith('src/') && !localPath.startsWith('node_modules/') && !localPath.includes('/node_modules/'));
  });
  if(unexpectedFiles.length > 0) {
    fail('Engine imports must stay within the engine and its dependencies:', unexpectedFiles);
  }
}

/** Enforce documented source layers while grandfathering named migration debt only. */
function checkLayerDirection() {
  const knownExceptions = new Set(architecture.existingViolations);
  const encounteredExceptions = new Set();
  const violations = [];
  for(const sourcePath of collectSources(sourceRoot)) {
    const sourceModule = relative(packageRoot, sourcePath).replaceAll('\\', '/');
    const sourceLayer = layerFor(sourceModule);
    const source = readFileSync(sourcePath, 'utf8');
    for(const specifier of collectModuleSpecifiers(source)) {
      if(!specifier.startsWith('.')) {
        continue;
      }
      const targetPath = resolve(dirname(sourcePath), specifier);
      const targetModule = relative(packageRoot, targetPath).replaceAll('\\', '/');
      if(!targetModule.startsWith('src/')) {
        continue;
      }
      const targetLayer = layerFor(targetModule);
      if(sourceLayer === targetLayer || architecture.layers[sourceLayer].mayImport.includes(targetLayer)) {
        continue;
      }
      const edge = `${sourceModule} -> ${targetModule}`;
      if(knownExceptions.has(edge)) {
        encounteredExceptions.add(edge);
      } else {
        violations.push(edge);
      }
    }
  }
  const staleExceptions = [...knownExceptions].filter(edge => !encounteredExceptions.has(edge));
  violations.push(...staleExceptions.map(edge => `stale exception: ${edge}`));
  if(violations.length > 0) {
    fail('Engine source dependency layers were violated:', violations);
  }
  console.log(`Engine dependency boundaries checked (${knownExceptions.size} recorded migration violations).`);
}

/** Collect static imports/exports and literal dynamic imports without executing source. */
function collectModuleSpecifiers(source) {
  const specifiers = [];
  const patterns = [
    /(?:import|export)\s+(?:type\s+)?(?:[\s\S]*?\s+from\s+)?['"]([^'"]+)['"]/g,
    /import\s*\(\s*['"]([^'"]+)['"]\s*\)/g
  ];
  for(const pattern of patterns) {
    for(const match of source.matchAll(pattern)) {
      specifiers.push(match[1]);
    }
  }
  return specifiers;
}

function collectSources(directory) {
  const sources = [];
  for(const entry of readdirSync(directory, {withFileTypes: true})) {
    const path = resolve(directory, entry.name);
    if(entry.isDirectory()) {
      sources.push(...collectSources(path));
    } else if(sourceExtensions.has(extname(entry.name)) && !entry.name.endsWith('.d.ts')) {
      sources.push(path);
    }
  }
  return sources;
}

function layerFor(modulePath) {
  for(const [layer, definition] of Object.entries(architecture.layers)) {
    if(definition.roots.some(root => modulePath.startsWith(`src/${root}/`) || modulePath === `src/${root}.ts`)) {
      return layer;
    }
  }
  throw new Error(`No architecture layer owns ${modulePath}.`);
}

function fail(message, details) {
  console.error(message);
  console.error(details.join('\n'));
  process.exit(1);
}
