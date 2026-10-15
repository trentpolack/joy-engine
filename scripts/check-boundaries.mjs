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
const consumerExtensions = new Set(['.ts', '.js', '.mjs', '.cjs', '.html']);

// Generated output and installed packages are not authored consumer source.
const ignoredConsumerDirectories = new Set(['node_modules', 'dist', 'artifacts']);

checkPackageBoundary();
checkLayerDirection();
checkConsumerBackendPackages();

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
        const backendEdge = backendPackageViolation(sourceModule, sourceLayer, specifier);
        if(backendEdge && knownExceptions.has(backendEdge)) {
          encounteredExceptions.add(backendEdge);
        } else if(backendEdge) {
          violations.push(backendEdge);
        }
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

/**
 * Keep backend packages behind the engine: the editor and examples consume `joy-engine` exports only.
 * Any mention of a backend package specifier counts, including JSDoc `import()` types and Vite
 * dependency lists, because each one couples the consumer to the backend's API or packaging.
 */
function checkConsumerBackendPackages() {
  const prefixes = Object.keys(architecture.backendPackages);
  const violations = [];
  for(const root of architecture.consumerRoots) {
    for(const sourcePath of collectConsumerSources(resolve(packageRoot, root))) {
      const source = readFileSync(sourcePath, 'utf8');
      const consumerModule = relative(packageRoot, sourcePath).replaceAll('\\', '/');
      for(const prefix of prefixes) {
        if(source.includes(prefix)) {
          violations.push(`${consumerModule} names ${prefix}`);
        }
      }
    }
  }
  if(violations.length > 0) {
    fail('Editor and example sources must reach backend packages through joy-engine exports:', violations);
  }
  console.log(`Consumer backend boundaries checked (${prefixes.join(', ')}).`);
}

/**
 * Return the violating edge when an engine module imports a backend package outside its owning layers.
 * @param sourceModule Package-relative importing module.
 * @param sourceLayer Architecture layer that owns the importing module.
 * @param specifier Bare package specifier from the import.
 * @returns The edge recorded for exceptions, or null when the import is permitted.
 */
function backendPackageViolation(sourceModule, sourceLayer, specifier) {
  for(const [prefix, ownership] of Object.entries(architecture.backendPackages)) {
    if(specifier.startsWith(prefix) && !ownership.layers.includes(sourceLayer)) {
      return `${sourceModule} -> ${specifier}`;
    }
  }
  return null;
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

function collectConsumerSources(directory) {
  const sources = [];
  for(const entry of readdirSync(directory, {withFileTypes: true})) {
    if(entry.name.startsWith('.') || ignoredConsumerDirectories.has(entry.name)) {
      continue;
    }
    const path = resolve(directory, entry.name);
    if(entry.isDirectory()) {
      sources.push(...collectConsumerSources(path));
    } else if(consumerExtensions.has(extname(entry.name))) {
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
