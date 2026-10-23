// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {execFileSync} from 'node:child_process';
import {readFileSync, readdirSync} from 'node:fs';
import {dirname, extname, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

// The architecture policy lives in the Cargo manifests: the workspace manifest's `[workspace.metadata.joy]` orders the
// layers and names the backend crates' owning layers, and every engine crate names its layer in
// `[package.metadata.joy]`. Cargo already limits a crate to the dependencies it declares, so checking manifests covers
// every path its source can name; only the facade's examples share its dependencies and need a source scan.
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// The facade package re-exports the engine crates for apps; it is the composition root, not a layer.
const FACADE_PACKAGE = 'joy-engine';
const ENGINE_CRATE_DIRECTORY = 'crates/';
// Generated output and installed packages are not authored source.
const ignoredSourceDirectories = new Set(['node_modules', 'dist', 'artifacts', 'target']);

const workspace = readCargoWorkspace();
const policy = readArchitecturePolicy(workspace.metadata);
const enginePackages = classifyEnginePackages(workspace.packages, policy);

checkEngineCrateDependencies();
checkAppManifests();
checkExampleSources();

/**
 * Enforce the engine crates' layers through their manifests. An engine crate may depend only on engine crates in its
 * own or an earlier layer, may name backend crates only from their owning layers, never depends on the facade or an
 * app, and takes test support only as a dev-dependency.
 */
function checkEngineCrateDependencies() {
  const violations = [];
  for(const enginePackage of enginePackages.layered.values()) {
    const layerIndex = policy.layers.indexOf(enginePackage.layer);
    for(const dependency of enginePackage.record.dependencies) {
      const name = dependency.name;
      if(enginePackages.testSupport.has(name)) {
        if(dependency.kind !== 'dev') {
          violations.push(`${enginePackage.name} depends on test support ${name} outside [dev-dependencies]`);
        }
        continue;
      }
      if(name === FACADE_PACKAGE || enginePackages.apps.has(name)) {
        violations.push(`${enginePackage.name} (${enginePackage.layer}) -> ${name} (engine crates never depend on the facade or apps)`);
        continue;
      }
      const target = enginePackages.layered.get(name);
      if(target && policy.layers.indexOf(target.layer) > layerIndex) {
        violations.push(`${enginePackage.name} (${enginePackage.layer}) -> ${name} (${target.layer})`);
      }
      const owners = policy.backendCrates.get(name);
      if(owners && !owners.includes(enginePackage.layer)) {
        violations.push(`${enginePackage.name} (${enginePackage.layer}) -> backend crate ${name}`);
      }
    }
  }
  if(violations.length > 0) {
    fail('Engine crate dependency layers were violated:', violations);
  }
  console.log(`Engine crate layers checked (${enginePackages.layered.size} crates).`);
}

/**
 * Keep the workspace's apps (Joy Editor and the tool editors) on the public API: their manifests may name the facade but no
 * engine crate or backend crate, so `joy_engine::physics` and `joy_engine::egui` are the only paths their source has.
 */
function checkAppManifests() {
  const violations = [];
  for(const app of enginePackages.apps.values()) {
    for(const dependency of app.dependencies) {
      if(enginePackages.layered.has(dependency.name)) {
        violations.push(`${app.name} depends on ${dependency.name} (apps depend on ${FACADE_PACKAGE} only)`);
      }
      if(policy.backendCrates.has(dependency.name)) {
        violations.push(`${app.name} depends on backend crate ${dependency.name} (use ${FACADE_PACKAGE}'s APIs and re-exports)`);
      }
    }
  }
  if(violations.length > 0) {
    fail('Apps must reach the engine through joy-engine:', violations);
  }
  console.log(`App manifests checked (${enginePackages.apps.size} packages).`);
}

/**
 * The facade's examples compile with the facade's own dependencies, so their source is scanned instead: they may not
 * name an engine crate (`joy_physics` rather than `joy_engine::physics`) or a backend crate. egui is re-exported by the
 * facade, so only direct `use egui` imports are rejected for it.
 */
function checkExampleSources() {
  const facade = workspace.packages.find(workspacePackage => workspacePackage.name === FACADE_PACKAGE);
  const exampleDirectories = new Set(facade.targets
    .filter(target => target.kind.includes('example'))
    .map(target => dirname(target.src_path)));

  const violations = [];
  let checkedFiles = 0;
  for(const directory of exampleDirectories) {
    for(const sourcePath of collectRustSources(directory)) {
      checkedFiles++;
      const source = stripRustComments(readFileSync(sourcePath, 'utf8'));
      const exampleModule = relative(packageRoot, sourcePath).replaceAll('\\', '/');
      for(const crateName of policy.backendCrates.keys()) {
        const identifier = rustIdentifier(crateName);
        const direct = identifier === 'egui' ? /^\s*(?:pub\s+)?use\s+::?egui\b/m.test(source) : namesRustCrate(source, identifier);
        if(direct) {
          violations.push(`${exampleModule} names ${identifier}`);
        }
      }
      for(const crateName of enginePackages.layered.keys()) {
        if(namesRustCrate(source, rustIdentifier(crateName))) {
          violations.push(`${exampleModule} names ${rustIdentifier(crateName)} (use joy_engine's re-export)`);
        }
      }
    }
  }
  if(violations.length > 0) {
    fail('Examples must reach the engine through joy_engine:', violations);
  }
  console.log(`Example sources checked (${checkedFiles} files).`);
}

/**
 * Read the workspace's packages and metadata from Cargo, without resolving registry dependencies.
 * @returns {{packages: object[], metadata: object}} `cargo metadata` package records and the workspace metadata table.
 */
function readCargoWorkspace() {
  const manifestPath = resolve(packageRoot, 'Cargo.toml');
  const output = execFileSync('cargo', ['metadata', '--no-deps', '--format-version', '1', '--manifest-path', manifestPath], {encoding: 'utf8', maxBuffer: 64*1024*1024});
  const metadata = JSON.parse(output);
  return {packages: metadata.packages, metadata: metadata.metadata ?? {}};
}

/**
 * Validate and normalize `[workspace.metadata.joy]`.
 * @param {object} workspaceMetadata The workspace metadata table from `cargo metadata`.
 * @returns {{layers: string[], backendCrates: Map<string, string[]>}} Ordered layers and backend owners.
 */
function readArchitecturePolicy(workspaceMetadata) {
  const definition = workspaceMetadata.joy;
  if(!definition || !Array.isArray(definition.layers) || definition.layers.length === 0) {
    fail('The workspace manifest needs [workspace.metadata.joy] with an ordered `layers` list.', []);
  }

  const backendCrates = new Map();
  for(const [crateName, owners] of Object.entries(definition['backend-crates'] ?? {})) {
    const unknown = owners.filter(owner => !definition.layers.includes(owner));
    if(unknown.length > 0) {
      fail(`Backend crate ${crateName} names unknown layers:`, unknown);
    }
    backendCrates.set(crateName, owners);
  }
  return {layers: definition.layers, backendCrates};
}

/**
 * Sort workspace packages into layered engine crates, test support, and apps. Every package in `crates/` must name a
 * known layer or mark itself test support; nothing outside `crates/` may claim a layer.
 * @param {object[]} packages `cargo metadata` package records.
 * @param {{layers: string[]}} architecture The validated policy.
 */
function classifyEnginePackages(packages, architecture) {
  const layered = new Map();
  const testSupport = new Set();
  const apps = new Map();
  const violations = [];

  for(const workspacePackage of packages) {
    const manifest = relative(packageRoot, workspacePackage.manifest_path).replaceAll('\\', '/');
    const joy = workspacePackage.metadata?.joy ?? {};
    const isEngineCrate = manifest.startsWith(ENGINE_CRATE_DIRECTORY);

    if(!isEngineCrate) {
      if(joy.layer !== undefined || joy['test-support'] !== undefined) {
        violations.push(`${manifest} (${workspacePackage.name}) is outside ${ENGINE_CRATE_DIRECTORY} but declares an engine layer`);
      }
      if(workspacePackage.name !== FACADE_PACKAGE) {
        apps.set(workspacePackage.name, workspacePackage);
      }
      continue;
    }

    if(joy['test-support'] === true) {
      testSupport.add(workspacePackage.name);
      continue;
    }
    if(!architecture.layers.includes(joy.layer)) {
      violations.push(`${manifest} (${workspacePackage.name}) needs [package.metadata.joy] layer = one of ${architecture.layers.join(', ')}`);
      continue;
    }
    layered.set(workspacePackage.name, {name: workspacePackage.name, layer: joy.layer, record: workspacePackage});
  }

  if(violations.length > 0) {
    fail('Engine crates must declare their architecture layer:', violations);
  }
  return {layered, testSupport, apps};
}

/**
 * The identifier Rust source uses for a package name (`joy-physics` is `joy_physics`).
 * @param {string} packageName Cargo package name.
 */
function rustIdentifier(packageName) {
  return packageName.replaceAll('-', '_');
}

/**
 * Whether Rust source names an external crate as a path root (`wgpu::`), in a `use`, or with `extern crate`.
 */
function namesRustCrate(source, crateName) {
  return new RegExp(`(?<![\\w:$])${crateName}::|^\\s*(?:pub\\s+)?use\\s+::?${crateName}\\b|extern\\s+crate\\s+${crateName}\\b`, 'm').test(source);
}

/**
 * Remove ordinary comments, keeping doc comments (their links are real references).
 */
function stripRustComments(source) {
  return source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/(?![/!]).*$/gm, '');
}

function collectRustSources(directory) {
  const sources = [];
  for(const entry of readdirSync(directory, {withFileTypes: true})) {
    if(entry.name.startsWith('.') || ignoredSourceDirectories.has(entry.name)) {
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
