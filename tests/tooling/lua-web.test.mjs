// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import {mkdirSync, mkdtempSync, rmSync, writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {pathToFileURL} from 'node:url';
import test from 'node:test';

import {LuaWasiRuntime} from '../../scripts/rust/lua-wasi.mjs';
import {initializeBeforeBindgenStart} from '../../scripts/rust/lua-web-build.mjs';
import {luaWasmEnvironment} from '../../scripts/rust/lua-wasm-toolchain.mjs';

test('WASI binds memory before constructors and never reconstructs an owned instance', () => {
  const runtime = new LuaWasiRuntime();
  const memory = new WebAssembly.Memory({initial: 1});
  let constructors = 0;
  const instance = {exports: {memory, _initialize() { throw new Error('reactor startup must not run twice'); }, __wasm_call_ctors() {
    constructors+= 1;
    assert.equal(runtime.wasi.wasiImport.environ_sizes_get(0, 4), 0);
  }}};
  runtime.initialize(instance);
  runtime.initialize(instance);
  assert.equal(constructors, 1);
  assert.equal(new DataView(memory.buffer).getUint32(0, true), 0);
  assert.throws(() => runtime.initialize({exports: instance.exports}), /one WebAssembly instance/);

  const failedRuntime = new LuaWasiRuntime();
  let attempts = 0;
  const failedInstance = {exports: {memory, __wasm_call_ctors() {
    attempts+= 1;
    throw new Error('constructor failed');
  }}};
  assert.throws(() => failedRuntime.initialize(failedInstance), /constructor failed/);
  assert.throws(() => failedRuntime.initialize(failedInstance), /constructor failed/);
  assert.equal(attempts, 1);
});

test('WASI accesses the current memory after the shared heap grows', () => {
  const runtime = new LuaWasiRuntime();
  const memory = new WebAssembly.Memory({initial: 1});
  runtime.initialize({exports: {memory, __wasm_call_ctors() {}}});
  memory.grow(1);
  const view = new DataView(memory.buffer);
  view.setUint32(65536, 123, true);
  view.setUint32(65540, 123, true);
  assert.equal(runtime.wasi.wasiImport.environ_sizes_get(65536, 65540), 0);
  assert.equal(view.getUint32(65536, true), 0);
  assert.equal(view.getUint32(65540, true), 0);
  assert.equal(runtime.wasi.wasiImport.fd_close(20), 8);
  // Standard Lua links libc filesystem functions, but the browser host exposes no preopened directories.
  assert.equal(runtime.wasi.wasiImport.fd_prestat_get(3, 0), 8);
  assert.equal(runtime.wasi.wasiImport.path_open(3, 0, 0, 0, 0, 0n, 0n, 0, 4), 8);
});

test('vendored Lua receives its WASI compiler and preserves caller Rust flags before Cargo starts', () => {
  const directory = mkdtempSync(join(tmpdir(), 'joy-lua-toolchain-'));
  const sdk = join(directory, 'sdk with spaces');
  try {
    const libraries = join(sdk, 'share/wasi-sysroot/lib/wasm32-wasip1');
    mkdirSync(join(sdk, 'bin'), {recursive: true});
    mkdirSync(libraries, {recursive: true});
    writeFileSync(join(sdk, 'VERSION'), '34.0\nwasi-libc: pinned\nllvm: pinned\n');
    const suffix = process.platform === 'win32' ? '.exe' : '';
    for(const name of [`bin/clang${suffix}`, `bin/llvm-ar${suffix}`, 'share/wasi-sysroot/lib/wasm32-wasip1/libc.a',
      'share/wasi-sysroot/lib/wasm32-wasip1/libsetjmp.a']) {
      writeFileSync(join(sdk, name), '');
    }

    const input = {WASI_SDK_PATH: sdk, RUSTFLAGS: '-Cdebuginfo=0', CFLAGS_wasm32_unknown_unknown: '-DHOST_FLAG'};
    const result = luaWasmEnvironment(input);
    assert.equal(result.CC_wasm32_unknown_unknown, join(sdk, `bin/clang${suffix}`));
    assert.equal(result.AR_wasm32_unknown_unknown, join(sdk, `bin/llvm-ar${suffix}`));
    assert.equal(result.CFLAGS_wasm32_unknown_unknown, '-DHOST_FLAG --target=wasm32-wasip1');
    assert.deepEqual(result.CARGO_ENCODED_RUSTFLAGS.split('\x1f'), ['-Cdebuginfo=0', `-Lnative=${libraries}`, '-lstatic=c']);
    assert.equal(input.CARGO_ENCODED_RUSTFLAGS, undefined);

    const encoded = luaWasmEnvironment({...input, CARGO_ENCODED_RUSTFLAGS: '-Copt-level=1\x1f-Cdebuginfo=2'});
    assert.deepEqual(encoded.CARGO_ENCODED_RUSTFLAGS.split('\x1f'),
      ['-Copt-level=1', '-Cdebuginfo=2', `-Lnative=${libraries}`, '-lstatic=c']);
    assert.throws(() => luaWasmEnvironment({}), /WASI_SDK_PATH/);
    writeFileSync(join(sdk, 'VERSION'), '33.0\n');
    assert.throws(() => luaWasmEnvironment(input), /require WASI SDK 34.0/);
  } finally {
    rmSync(directory, {recursive: true, force: true});
  }
});

test('generated binding startup initializes the C runtime before entering the game', async () => {
  const directory = mkdtempSync(join(tmpdir(), 'joy-lua-start-'));
  try {
    writeFileSync(join(directory, 'lua-wasi.mjs'), `
      export function initializeWasi(instance) { instance.exports.__wasm_call_ctors(); }
    `);
    const bindings = `
      import * as wasi from "wasi_snapshot_preview1";
      export function start(instance) {
        const wasm = instance.exports;
        wasm.__wbindgen_start();
        return wasm;
      }
    `;
    writeFileSync(join(directory, 'bindings.mjs'), initializeBeforeBindgenStart(bindings));
    const module = await import(pathToFileURL(join(directory, 'bindings.mjs')));
    const events = [];
    module.start({exports: {
      __wasm_call_ctors() { events.push('constructors'); },
      __wbindgen_start() { events.push('game'); }
    }});
    assert.deepEqual(events, ['constructors', 'game']);
    assert.throws(() => initializeBeforeBindgenStart('export const unsupported = true;'), /Unsupported wasm-bindgen/);
  } finally {
    rmSync(directory, {recursive: true, force: true});
  }
});
