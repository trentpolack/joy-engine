// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {readFileSync, readdirSync} from 'node:fs';
import {dirname, extname, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sourceRoot = resolve(packageRoot, 'src');
const architecture = JSON.parse(readFileSync(resolve(packageRoot, 'architecture/layers.json'), 'utf8'));
// Generated output and installed packages are not authored consumer source.
const ignoredConsumerDirectories = new Set(['node_modules', 'dist', 'artifacts', 'target']);

checkRustLayerDirection();
checkRustConsumerBackendCrates();

/**
 * Enforce the engine crate's module layers and backend ownership.
 * A Rust module's layer comes from its top-level folder (`src/physics/...` is simulation); every `crate::<module>`
 * path it names, including grouped `use crate::{a, b}` imports and doc links, must point into its own layer or one it
 * may import, and backend crates (`rustBackendCrates`) may only be named from their owning layers. The crate root
 * (`src/lib.rs`, the public API) and binaries (`src/bin/`, which use that API) are exempt. Rust edges have no
 * migration allowlist.
 */
function checkRustLayerDirection() {
  const moduleLayers = new Map();
  for(const [layer, definition] of Object.entries(architecture.layers)) {
    for(const root of definition.roots) {
      moduleLayers.set(root.replaceAll('-', '_'), layer);
    }
  }
  const backendOwners = Object.entries(architecture.rustBackendCrates)
    .map(([name, ownership]) => [name.replaceAll('-', '_'), ownership.layers]);

  const violations = [];
  let checkedFiles = 0;
  for(const sourcePath of collectRustSources(sourceRoot)) {
    const sourceModule = relative(packageRoot, sourcePath).replaceAll('\\', '/');
    if(sourceModule === 'src/lib.rs' || sourceModule.startsWith('src/bin/')) {
      continue;
    }
    const topModule = sourceModule.slice('src/'.length).split('/')[0].replace(/\.rs$/, '');
    const sourceLayer = moduleLayers.get(topModule);
    if(!sourceLayer) {
      violations.push(`${sourceModule} is in no architecture layer (add src/${topModule} to a layer's roots)`);
      continue;
    }
    checkedFiles++;
    const source = stripRustComments(readFileSync(sourcePath, 'utf8'));
    for(const target of collectRustCrateModules(source)) {
      const targetLayer = moduleLayers.get(target);
      if(!targetLayer) {
        violations.push(`${sourceModule} -> crate::${target} (layers name modules, not crate-root items)`);
      } else if(targetLayer !== sourceLayer && !architecture.layers[sourceLayer].mayImport.includes(targetLayer)) {
        violations.push(`${sourceModule} (${sourceLayer}) -> crate::${target} (${targetLayer})`);
      }
    }
    for(const [crateName, layers] of backendOwners) {
      if(!layers.includes(sourceLayer) && namesRustCrate(source, crateName)) {
        violations.push(`${sourceModule} (${sourceLayer}) -> backend crate ${crateName}`);
      }
    }
  }
  if(violations.length > 0) {
    fail('Rust module dependency layers were violated:', violations);
  }
  console.log(`Rust module layers checked (${checkedFiles} files).`);
}

/**
 * Keep Rust examples on the public API: they may not name wgpu, winit, rapier, or the egui integrations directly.
 * egui itself is re-exported by `joy_engine`, so only direct `use egui` imports are rejected.
 */
function checkRustConsumerBackendCrates() {
  const violations = [];
  for(const root of architecture.consumerRoots) {
    for(const sourcePath of collectRustSources(resolve(packageRoot, root))) {
      const source = stripRustComments(readFileSync(sourcePath, 'utf8'));
      const consumerModule = relative(packageRoot, sourcePath).replaceAll('\\', '/');
      for(const crateName of Object.keys(architecture.rustBackendCrates).map(name => name.replaceAll('-', '_'))) {
        const direct = crateName === 'egui' ? new RegExp(`^\\s*(?:pub\\s+)?use\\s+::?egui\\b`, 'm').test(source) : namesRustCrate(source, crateName);
        if(direct) {
          violations.push(`${consumerModule} names ${crateName}`);
        }
      }
    }
  }
  if(violations.length > 0) {
    fail('Rust examples must reach backend crates through joy_engine:', violations);
  }
}

/** Top-level module names referenced through `crate::`, expanding grouped imports such as `crate::{core::X, physics}`. */
function collectRustCrateModules(source) {
  const modules = new Set();
  const pattern = /(?<![$\w])crate::/g;
  for(const match of source.matchAll(pattern)) {
    const start = match.index + match[0].length;
    if(source[start] !== '{') {
      const name = /^[A-Za-z_]\w*/.exec(source.slice(start));
      if(name) {
        modules.add(name[0]);
      }
      continue;
    }
    let depth = 0;
    let partStart = start + 1;
    for(let index = start; index < source.length; index++) {
      const character = source[index];
      if(character === '{') {
        depth++;
      } else if(character === '}' && --depth === 0) {
        addLeadingName(source.slice(partStart, index), modules);
        break;
      } else if(character === ',' && depth === 1) {
        addLeadingName(source.slice(partStart, index), modules);
        partStart = index + 1;
      }
    }
  }
  return modules;
}

function addLeadingName(part, modules) {
  const name = /^\s*([A-Za-z_]\w*)/.exec(part);
  if(name && name[1] !== 'self') {
    modules.add(name[1]);
  }
}

/** Whether Rust source names an external crate as a path root (`wgpu::`), in a `use`, or with `extern crate`. */
function namesRustCrate(source, crateName) {
  return new RegExp(`(?<![\\w:$])${crateName}::|^\\s*(?:pub\\s+)?use\\s+::?${crateName}\\b|extern\\s+crate\\s+${crateName}\\b`, 'm').test(source);
}

/** Remove ordinary comments, keeping doc comments (their links are real references) and string contents intact enough. */
function stripRustComments(source) {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/(?![/!]).*$/gm, '');
}

function collectRustSources(directory) {
  const sources = [];
  for(const entry of readdirSync(directory, {withFileTypes: true})) {
    if(entry.name.startsWith('.') || ignoredConsumerDirectories.has(entry.name)) {
      continue;
    }
    const path = resolve(directory, entry.name);
    if(entry.isDirectory()) {
      sources.push(...collectRustSources(path));
    } else if(extname(entry.name) === '.rs') {
      sources.push(path);
    }
  }
  return sources;
}

function fail(message, details) {
  console.error(message);
  console.error(details.join('\n'));
  process.exit(1);
}
