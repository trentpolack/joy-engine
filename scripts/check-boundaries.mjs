// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {execFileSync} from 'node:child_process';
import {readFileSync, readdirSync} from 'node:fs';
import {dirname, extname, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const sourceRoot = resolve(packageRoot, 'src');
const architecture = JSON.parse(readFileSync(resolve(packageRoot, 'architecture/layers.json'), 'utf8'));
// Generated output and installed packages are not authored consumer source.
const ignoredConsumerDirectories = new Set(['node_modules', 'dist', 'artifacts', 'target']);
// The facade package re-exports the engine crates for games and tools; it is the composition root, not a layer.
const FACADE_PACKAGE = 'joy-engine';

const crateLayers = collectCrateLayers();
const testSupportCrates = new Set(architecture.testSupportCrates ?? []);
const backendOwners = new Map(Object.entries(architecture.rustBackendCrates).map(([name, ownership]) => [name, ownership.layers]));
const workspacePackages = readCargoPackages();

checkRustLayerDirection();
checkRustCrateManifests();
checkRustConsumerBackendCrates();

/**
 * Enforce the layers of the facade package's own modules in `src/` (the modules that gather several engine crates), and
 * their backend ownership. A Rust module's layer comes from its top-level file or folder (`src/runtime.rs` and
 * `src/runtime/...` are platform); every `crate::<module>`
 * path it names, including grouped `use crate::{a, b}` imports and doc links, must point into its own layer or one it
 * may import. Engine crates it names (`joy_physics::...`) follow the same rule, and backend crates (`rustBackendCrates`)
 * may only be named from their owning layers. The crate root (`src/lib.rs`, the public API) and binaries (`src/bin/`,
 * which use that API) are exempt. Rust edges have no migration allowlist.
 */
function checkRustLayerDirection() {
  const moduleLayers = new Map();
  for(const [layer, definition] of Object.entries(architecture.layers)) {
    for(const root of definition.roots) {
      moduleLayers.set(root.replaceAll('-', '_'), layer);
    }
  }

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
    for(const [crateName, targetLayer] of crateLayers) {
      const allowed = targetLayer === sourceLayer || architecture.layers[sourceLayer].mayImport.includes(targetLayer);
      if(!allowed && namesRustCrate(source, rustIdentifier(crateName))) {
        violations.push(`${sourceModule} (${sourceLayer}) -> ${crateName} (${targetLayer})`);
      }
    }
    for(const [crateName, layers] of backendOwners) {
      if(!layers.includes(sourceLayer) && namesRustCrate(source, rustIdentifier(crateName))) {
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
 * Enforce the engine crates' layers through their manifests: Cargo already limits a crate to the dependencies it
 * declares, so checking `[dependencies]` covers every path its source can name. Each package in `crates/` belongs to
 * exactly one layer (or is test support); an engine crate may depend only on engine crates in its own or a lower
 * layer, may name backend crates only from their owning layers, never depends on the facade or a tool, and takes test
 * support only as a dev-dependency.
 */
function checkRustCrateManifests() {
  const packagesByName = new Map(workspacePackages.map(workspacePackage => [workspacePackage.name, workspacePackage]));
  const violations = [];

  for(const workspacePackage of workspacePackages) {
    const manifest = relative(packageRoot, workspacePackage.manifest_path).replaceAll('\\', '/');
    const inCrates = manifest.startsWith('crates/');
    if(inCrates && !crateLayers.has(workspacePackage.name) && !testSupportCrates.has(workspacePackage.name)) {
      violations.push(`${manifest} (${workspacePackage.name}) is in no architecture layer (add it to a layer's crates)`);
    }
  }

  for(const [crateName, layer] of crateLayers) {
    const enginePackage = packagesByName.get(crateName);
    if(!enginePackage) {
      violations.push(`${crateName} is listed in architecture/layers.json but is not a workspace package`);
      continue;
    }
    for(const dependency of enginePackage.dependencies) {
      const name = dependency.name;
      if(testSupportCrates.has(name)) {
        if(dependency.kind !== 'dev') {
          violations.push(`${crateName} depends on test support ${name} outside [dev-dependencies]`);
        }
        continue;
      }
      if(name === FACADE_PACKAGE || isToolPackage(packagesByName.get(name))) {
        violations.push(`${crateName} (${layer}) -> ${name} (engine crates never depend on the facade or tools)`);
        continue;
      }
      const targetLayer = crateLayers.get(name);
      if(targetLayer && targetLayer !== layer && !architecture.layers[layer].mayImport.includes(targetLayer)) {
        violations.push(`${crateName} (${layer}) -> ${name} (${targetLayer})`);
      }
      const owners = backendOwners.get(name);
      if(owners && !owners.includes(layer)) {
        violations.push(`${crateName} (${layer}) -> backend crate ${name}`);
      }
    }
  }
  if(violations.length > 0) {
    fail('Rust crate dependency layers were violated:', violations);
  }
  console.log(`Rust crate layers checked (${crateLayers.size} crates).`);
}

/**
 * Keep Rust examples and tools on the public API: they may not name wgpu, winit, rapier, or the egui integrations
 * directly, and they reach engine crates through `joy_engine` (`joy_engine::physics`, not `joy_physics`). egui itself
 * is re-exported by `joy_engine`, so only direct `use egui` imports are rejected. Tool manifests may not list engine
 * crates other than the facade.
 */
function checkRustConsumerBackendCrates() {
  const violations = [];
  for(const root of architecture.consumerRoots) {
    for(const sourcePath of collectRustSources(resolve(packageRoot, root))) {
      const source = stripRustComments(readFileSync(sourcePath, 'utf8'));
      const consumerModule = relative(packageRoot, sourcePath).replaceAll('\\', '/');
      for(const crateName of Object.keys(architecture.rustBackendCrates).map(rustIdentifier)) {
        const direct = crateName === 'egui' ? new RegExp(`^\\s*(?:pub\\s+)?use\\s+::?egui\\b`, 'm').test(source) : namesRustCrate(source, crateName);
        if(direct) {
          violations.push(`${consumerModule} names ${crateName}`);
        }
      }
      for(const crateName of crateLayers.keys()) {
        if(namesRustCrate(source, rustIdentifier(crateName))) {
          violations.push(`${consumerModule} names ${rustIdentifier(crateName)} (use joy_engine's re-export)`);
        }
      }
    }
  }
  for(const workspacePackage of workspacePackages.filter(isToolPackage)) {
    for(const dependency of workspacePackage.dependencies) {
      if(crateLayers.has(dependency.name)) {
        violations.push(`${workspacePackage.name} depends on ${dependency.name} (tools depend on ${FACADE_PACKAGE} only)`);
      }
    }
  }
  if(violations.length > 0) {
    fail('Rust examples must reach backend crates through joy_engine:', violations);
  }
}

/**
 * Map every engine crate named in `architecture/layers.json` to its layer.
 * @returns {Map<string, string>} Crate package name (`joy-physics`) to layer name.
 */
function collectCrateLayers() {
  const layers = new Map();
  for(const [layer, definition] of Object.entries(architecture.layers)) {
    for(const crateName of definition.crates ?? []) {
      layers.set(crateName, layer);
    }
  }
  return layers;
}

/**
 * Read the workspace's packages from Cargo, without resolving registry dependencies.
 * @returns {object[]} `cargo metadata` package records (name, manifest_path, dependencies).
 */
function readCargoPackages() {
  const manifestPath = resolve(packageRoot, 'Cargo.toml');
  const output = execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--manifest-path', manifestPath], {encoding: 'utf8', maxBuffer: 64*1024*1024});
  return JSON.parse(output).packages;
}

/**
 * Whether a workspace package is a consumer tool (the editor or a lab) rather than the facade or an engine crate.
 * @param {object | undefined} workspacePackage A `cargo metadata` package record.
 */
function isToolPackage(workspacePackage) {
  if(!workspacePackage) {
    return false;
  }
  const manifest = relative(packageRoot, workspacePackage.manifest_path).replaceAll('\\', '/');
  return architecture.consumerRoots.some(root => manifest.startsWith(`${root}/`));
}

/**
 * The identifier Rust source uses for a package name (`joy-physics` is `joy_physics`).
 * @param {string} packageName Cargo package name.
 */
function rustIdentifier(packageName) {
  return packageName.replaceAll('-', '_');
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
