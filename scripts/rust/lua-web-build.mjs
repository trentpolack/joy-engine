// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {copyFileSync, cpSync, readFileSync, writeFileSync} from 'node:fs';
import {dirname, resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const scriptRoot = dirname(fileURLToPath(import.meta.url));
const WASI_MODULE = 'wasi_snapshot_preview1';
const BINDGEN_START = 'wasm.__wbindgen_start();';
const SUPPORTED_IMPORTS = new Set([
  'environ_get', 'environ_sizes_get', 'clock_time_get', 'fd_close', 'fd_fdstat_get', 'fd_fdstat_set_flags',
  'fd_prestat_get', 'fd_prestat_dir_name', 'fd_read', 'fd_renumber', 'fd_seek', 'fd_write', 'path_open',
  'path_remove_directory', 'path_rename', 'path_unlink_file', 'proc_exit', 'random_get'
]);

/**
 * Add the WASI host only when the compiled artifact uses it. Verify the generated bindings' startup hook rather than
 * letting a wasm-bindgen update silently start the game before libc memory and C constructors are initialized.
 */
export function prepareLuaBrowserRuntime(directory, moduleName) {
  const bytes = readFileSync(resolve(directory, `${moduleName}_bg.wasm`));
  const module = new WebAssembly.Module(bytes);
  const imports = WebAssembly.Module.imports(module).filter(entry => entry.module === WASI_MODULE);
  if(imports.length === 0) {
    return '';
  }
  for(const entry of imports) {
    if(!SUPPORTED_IMPORTS.has(entry.name)) {
      throw new Error(`Unsupported Lua WASI import: ${entry.name}. Update the browser adapter before shipping this build.`);
    }
  }
  if(!WebAssembly.Module.exports(module).some(entry => entry.name === '__wasm_call_ctors')) {
    throw new Error('The Lua browser build must export __wasm_call_ctors.');
  }

  const bindingsPath = resolve(directory, `${moduleName}.js`);
  writeFileSync(bindingsPath, initializeBeforeBindgenStart(readFileSync(bindingsPath, 'utf8')));
  const adapter = readFileSync(resolve(scriptRoot, 'lua-wasi.mjs'), 'utf8');
  writeFileSync(resolve(directory, 'lua-wasi.mjs'), adapter.replace("'@bjorn3/browser_wasi_shim'", "'./wasi/index.js'"));

  const shimRoot = resolve(dirname(fileURLToPath(import.meta.resolve('@bjorn3/browser_wasi_shim'))), '..');
  cpSync(resolve(shimRoot, 'dist'), resolve(directory, 'wasi'), {recursive: true, filter: path => !path.endsWith('.tsbuildinfo')});
  for(const license of ['LICENSE-MIT', 'LICENSE-APACHE']) {
    copyFileSync(resolve(shimRoot, license), resolve(directory, 'wasi', license));
  }

  const notices = [
    readFileSync(resolve(shimRoot, 'LICENSE-MIT'), 'utf8'),
    ...['LICENSE-Lua', 'LICENSE-mlua', 'LICENSE-libc-alloc', 'LICENSE-wasi-libc'].map(name =>
      readFileSync(resolve(scriptRoot, '../../crates/joy-game-script/build', name), 'utf8'))
  ];
  return `\n\nLua browser runtime dependencies\n\n${notices.join('\n\n----------------------------------------\n\n')}`;
}

/** Patch only the pinned bindings' supported import and startup shapes; fail on ambiguous or changed output. */
export function initializeBeforeBindgenStart(bindings) {
  if(bindings.split(BINDGEN_START).length !== 2 || !bindings.includes(`from "${WASI_MODULE}"`)) {
    throw new Error('Unsupported wasm-bindgen startup glue. Expected one start call and WASI namespace imports.');
  }
  return `import {initializeWasi} from './lua-wasi.mjs';\n${bindings}`
    .replaceAll(`from "${WASI_MODULE}"`, 'from "./lua-wasi.mjs"')
    .replace(BINDGEN_START, `initializeWasi(instance);\n    ${BINDGEN_START}`);
}
