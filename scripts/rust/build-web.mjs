#!/usr/bin/env node
// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Build a Rust game, tool, or example for browsers: compile to WebAssembly, generate JavaScript bindings with the
// wasm-bindgen CLI pinned by the workspace lockfile, and write a static page that hosts the canvas. The output folder
// is self-contained and can be served by any static host (the site, GitHub Pages, `npx serve`).
//
//   node scripts/rust/build-web.mjs --example form-viewer [--release]
//   node joy-engine/scripts/rust/build-web.mjs --manifest-path Cargo.toml --package vehicle-playground --release
//
// Options:
//   --package <crate>        Crate to build (default: joy-engine, the engine crate).
//   --example <name>         Build an example; otherwise --bin <name>, otherwise the crate's cdylib library.
//   --bin <name>             Build a binary target.
//   --manifest-path <file>   Workspace Cargo.toml (default: the engine workspace).
//   --out <dir>              Output folder (default: <target>/web/<artifact>).
//   --release                Optimize for size with the `web-release` profile.
//   --dev                    Use the fast `dev` profile even when --release is given (development servers pass it so a
//                            workspace's build script keeps its title and canvas but builds quickly).
//   --title <text>           Page title (default: the artifact name).
//   --canvas <id>            Canvas element ID the game looks up (default: joy-canvas).
//
// Requires the wasm32-unknown-unknown target (`rustup target add wasm32-unknown-unknown`) and the wasm-bindgen CLI at
// the lockfile's version; set JOY_WASM_BINDGEN to its path when it is not on PATH.

import {spawnSync} from 'node:child_process';
import {copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync} from 'node:fs';
import {basename, dirname, relative, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {parseArgs} from 'node:util';

const scriptRoot = dirname(fileURLToPath(import.meta.url));
const engineRoot = resolve(scriptRoot, '../..');
const WASM_TARGET = 'wasm32-unknown-unknown';
// Licenses of third-party code and fonts compiled into the engine, relative to the engine root.
const THIRD_PARTY_LICENSES = [
  'crates/joy-physics/src/sdf/LICENSE-sdf-dcd-demo',
  'crates/joy-ui/fonts/Archivo-OFL.txt',
  'crates/joy-ui/fonts/ArchivoBlack-OFL.txt',
  'crates/joy-ui/fonts/Meslo-LICENSE.txt'
];

const {values: options} = parseArgs({
  options: {
    package: {type: 'string', default: 'joy-engine'},
    example: {type: 'string'},
    bin: {type: 'string'},
    'manifest-path': {type: 'string', default: resolve(engineRoot, 'Cargo.toml')},
    out: {type: 'string'},
    release: {type: 'boolean', default: false},
    dev: {type: 'boolean', default: false},
    title: {type: 'string'},
    canvas: {type: 'string', default: 'joy-canvas'}
  }
});

if(options.example && options.bin) {
  fail('Choose either --example or --bin, not both.');
}
if(!/^[A-Za-z0-9_-]+$/.test(options.canvas)) {
  fail('--canvas must be a plain element ID.');
}

const manifestPath = resolve(options['manifest-path']);
const workspaceRoot = dirname(manifestPath);
const bindgen = findBindgen(requiredBindgenVersion(resolve(workspaceRoot, 'Cargo.lock')));

const profile = options.release && !options.dev ? 'web-release' : 'dev';
const cargoArguments = ['build', '--manifest-path', manifestPath, '--target', WASM_TARGET, '--package', options.package, '--profile', profile];
if(options.example) {
  cargoArguments.push('--example', options.example);
} else if(options.bin) {
  cargoArguments.push('--bin', options.bin);
} else {
  cargoArguments.push('--lib');
}
run('cargo', cargoArguments);

const targetDirectory = cargoTargetDirectory();
const profileDirectory = resolve(targetDirectory, WASM_TARGET, profile === 'dev' ? 'debug' : profile);
const artifact = options.example ?? options.bin ?? options.package;
const wasmFile = options.example
  ? resolve(profileDirectory, 'examples', `${artifact}.wasm`)
  : resolve(profileDirectory, `${options.bin ?? options.package.replaceAll('-', '_')}.wasm`);
if(!existsSync(wasmFile)) {
  fail(`Expected ${relative(process.cwd(), wasmFile)}; library builds need crate-type = ["cdylib", "rlib"].`);
}

const outputDirectory = resolve(options.out ?? resolve(targetDirectory, 'web', artifact));
mkdirSync(outputDirectory, {recursive: true});
const moduleName = artifact.replaceAll('-', '_');
run(bindgen.path, ['--target', 'web', '--no-typescript', '--out-dir', outputDirectory, '--out-name', moduleName, wasmFile]);

const template = readFileSync(resolve(scriptRoot, 'web-template.html'), 'utf8');
const page = template
  .replaceAll('{{title}}', escapeHtml(options.title ?? artifact))
  .replaceAll('{{canvas}}', options.canvas)
  .replaceAll('{{module}}', `./${moduleName}.js`);
writeFileSync(resolve(outputDirectory, 'index.html'), page);
copyFileSync(resolve(engineRoot, 'LICENSE'), resolve(outputDirectory, 'LICENSE'));
writeFileSync(resolve(outputDirectory, 'THIRD-PARTY-NOTICES.txt'), thirdPartyNotices());
console.log(`Web build ready: ${relative(process.cwd(), outputDirectory) || '.'} (${basename(wasmFile)}, ${profile}).`);

/**
 * Licenses for the SDF collision reference implementation and embedded authoring fonts, separated by a rule.
 */
function thirdPartyNotices() {
  const notices = THIRD_PARTY_LICENSES.map(path => readFileSync(resolve(engineRoot, path), 'utf8').trimEnd());
  return `${notices.join('\n\n----------------------------------------\n\n')}\n`;
}

/** Read the wasm-bindgen version the lockfile resolved; the CLI must match it exactly. */
function requiredBindgenVersion(lockfile) {
  if(!existsSync(lockfile)) {
    fail(`Missing ${lockfile}; run cargo once to create it.`);
  }
  const match = readFileSync(lockfile, 'utf8').match(/\[\[package\]\]\s*name = "wasm-bindgen"\s*version = "([^"]+)"/);
  if(!match) {
    fail(`${lockfile} does not depend on wasm-bindgen; is the crate a browser target?`);
  }
  return match[1];
}

/** Locate the wasm-bindgen CLI and confirm its version. */
function findBindgen(version) {
  const path = process.env.JOY_WASM_BINDGEN || 'wasm-bindgen';
  const result = spawnSync(path, ['--version'], {encoding: 'utf8'});
  const found = result.status === 0 ? result.stdout.trim().split(/\s+/).pop() : null;
  if(found !== version) {
    fail([
      found ? `wasm-bindgen ${found} found, but the lockfile needs ${version}.` : 'The wasm-bindgen CLI was not found.',
      `Install it with: cargo install wasm-bindgen-cli --version ${version} --locked`,
      'Then put it on PATH or set JOY_WASM_BINDGEN to its path.'
    ].join('\n'));
  }
  return {path, version};
}

/** The workspace's target directory, honoring CARGO_TARGET_DIR and configuration. */
function cargoTargetDirectory() {
  const result = spawnSync('cargo', ['metadata', '--format-version', '1', '--no-deps', '--manifest-path', manifestPath], {encoding: 'utf8', maxBuffer: 64*1024*1024});
  if(result.status !== 0) {
    fail(result.stderr || 'cargo metadata failed.');
  }
  return JSON.parse(result.stdout).target_directory;
}

function run(command, commandArguments) {
  const result = spawnSync(command, commandArguments, {stdio: 'inherit'});
  if(result.error || result.status !== 0) {
    fail(result.error ? `${command}: ${result.error.message}` : `${command} exited with status ${result.status}.`);
  }
}

function escapeHtml(text) {
  return text.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
}

function fail(message) {
  console.error(message);
  process.exit(1);
}
