#!/usr/bin/env node
// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Vendored dependencies compile before joy-game-script/build.rs runs. Configure their C compiler and the final
// Rust linker together, before starting Cargo. The browser builder and direct Cargo/CI checks share this entry point.
import {spawnSync} from 'node:child_process';
import {existsSync, readFileSync} from 'node:fs';
import {resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const TARGET = 'wasm32_unknown_unknown';
const RUST_TARGET_FLAGS = 'CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS';
const SDK_VERSION = '34.0';

/**
 * Return a child-process environment for vendored Lua on wasm32-unknown-unknown. Preserve caller Rust flags while
 * adding WASI's libc search path and shared allocator. lua-src enables standard Wasm exceptions for setjmp/longjmp.
 */
export function luaWasmEnvironment(environment = process.env) {
  if(!environment.WASI_SDK_PATH) {
    throw new Error('Lua browser builds require WASI SDK 34.0. Set WASI_SDK_PATH; see docs/rust-game-script.md.');
  }
  const sdk = resolve(environment.WASI_SDK_PATH);
  const versionFile = resolve(sdk, 'VERSION');
  if(!existsSync(versionFile) || readFileSync(versionFile, 'utf8').split(/\r?\n/, 1)[0] !== SDK_VERSION) {
    throw new Error(`Lua browser builds require WASI SDK ${SDK_VERSION}; check ${versionFile}.`);
  }
  const suffix = process.platform === 'win32' ? '.exe' : '';
  const compiler = resolve(sdk, `bin/clang${suffix}`);
  const archiver = resolve(sdk, `bin/llvm-ar${suffix}`);
  const libraries = resolve(sdk, 'share/wasi-sysroot/lib/wasm32-wasip1');
  for(const path of [compiler, archiver, resolve(libraries, 'libc.a'), resolve(libraries, 'libsetjmp.a')]) {
    if(!existsSync(path)) {
      throw new Error(`WASI SDK ${SDK_VERSION} is incomplete: missing ${path}.`);
    }
  }

  // Cargo gives encoded flags precedence over RUSTFLAGS and target flags. Keep that same precedence, and encode
  // each new argument separately so SDK paths containing spaces remain one linker argument.
  const rustFlags = environment.CARGO_ENCODED_RUSTFLAGS !== undefined
    ? environment.CARGO_ENCODED_RUSTFLAGS.split('\x1f').filter(Boolean)
    : (environment.RUSTFLAGS ?? environment[RUST_TARGET_FLAGS] ?? '').split(/\s+/).filter(Boolean);
  rustFlags.push(`-Lnative=${libraries}`, '-lstatic=c');

  return {
    ...environment,
    [`CC_${TARGET}`]: compiler,
    [`AR_${TARGET}`]: archiver,
    [`CFLAGS_${TARGET}`]: `${environment[`CFLAGS_${TARGET}`] ?? ''} --target=wasm32-wasip1`.trim(),
    CARGO_ENCODED_RUSTFLAGS: rustFlags.join('\x1f')
  };
}

// Direct checks use: node scripts/rust/lua-wasm-toolchain.mjs check --target wasm32-unknown-unknown ...
if(process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const result = spawnSync('cargo', process.argv.slice(2), {stdio: 'inherit', env: luaWasmEnvironment()});
    if(result.error) {
      throw result.error;
    }
    process.exitCode = result.status ?? 1;
  } catch(error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
