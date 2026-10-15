// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Enforce `architecture/layers.json` on the Rust workspace: each engine crate owns one layer, may depend only on the
// crates of layers it may import, and only the owning layers may depend on a backend crate (wgpu, winit, rapier,
// egui). Cargo already forbids cycles; this keeps the crate graph aligned with the documented one-way model so the
// TypeScript and Rust engines share one architecture. Reads manifests only; does not need a Rust toolchain.

import {existsSync, readFileSync, readdirSync} from 'node:fs';
import {dirname, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const architecture = JSON.parse(readFileSync(resolve(packageRoot, 'architecture/layers.json'), 'utf8'));
const crateRoot = resolve(packageRoot, 'crates');

const crateLayers = architecture.rustCrates;
const facade = architecture.rustFacadeCrate;
const backendCrates = architecture.rustBackendCrates;
const violations = [];

const manifests = readdirSync(crateRoot, {withFileTypes: true})
  .filter(entry => entry.isDirectory() && existsSync(resolve(crateRoot, entry.name, 'Cargo.toml')))
  .map(entry => ({directory: entry.name, ...readManifest(resolve(crateRoot, entry.name, 'Cargo.toml'))}));

for(const manifest of manifests) {
  if(manifest.name !== facade && !Object.hasOwn(crateLayers, manifest.name)) {
    violations.push(`crates/${manifest.directory}: ${manifest.name} is not assigned a layer in architecture/layers.json`);
    continue;
  }
  const layer = crateLayers[manifest.name];
  for(const dependency of manifest.dependencies) {
    if(Object.hasOwn(crateLayers, dependency)) {
      const target = crateLayers[dependency];
      const allowed = manifest.name === facade || target === layer || architecture.layers[layer].mayImport.includes(target);
      if(!allowed) {
        violations.push(`${manifest.name} (${layer}) -> ${dependency} (${target})`);
      }
    } else if(dependency === facade) {
      violations.push(`${manifest.name} depends on the ${facade} facade; layers depend on each other directly`);
    } else if(Object.hasOwn(backendCrates, dependency) && !backendCrates[dependency].layers.includes(layer)) {
      violations.push(`${manifest.name} (${layer ?? 'facade'}) -> backend crate ${dependency}`);
    }
  }
}

for(const name of Object.keys(crateLayers)) {
  if(!manifests.some(manifest => manifest.name === name)) {
    violations.push(`architecture/layers.json names missing crate ${name}`);
  }
}

if(violations.length > 0) {
  console.error('Rust crate dependency layers were violated:');
  console.error(violations.join('\n'));
  process.exit(1);
}
console.log(`Rust crate layers checked (${manifests.length} crates).`);

/**
 * Read a crate's package name and the names of its normal, development, build, and target-specific dependencies.
 * Handles the manifest forms the workspace uses: `name = ...`, `name.workspace = true`, and inline tables.
 * @param path Absolute `Cargo.toml` path.
 */
function readManifest(path) {
  let section = '';
  let name = '';
  const dependencies = new Set();
  for(const rawLine of readFileSync(path, 'utf8').split(/\r?\n/)) {
    const line = rawLine.replace(/\s+#.*$/, '').trim();
    if(line === '' || line.startsWith('#')) {
      continue;
    }
    const header = line.match(/^\[(.+)\]$/);
    if(header) {
      section = header[1];
      continue;
    }
    const key = line.match(/^([A-Za-z0-9_-]+)(?:\.[A-Za-z0-9_.-]+)?\s*=/);
    if(!key) {
      continue;
    }
    if(section === 'package' && key[1] === 'name' && !line.includes('.')) {
      name = JSON.parse(line.slice(line.indexOf('=') + 1).trim());
    } else if(/(?:^|\.)(?:dev-|build-)?dependencies$/.test(section)) {
      dependencies.add(key[1]);
    }
  }
  return {name, dependencies: [...dependencies]};
}
