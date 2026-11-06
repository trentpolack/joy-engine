// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Each generated game module owns one WASI instance. No filesystem directories, environment variables, or process
// arguments are exposed; these imports support libc only. Script capabilities remain controlled by the VM.
import {WASI, File, OpenFile, ConsoleStdout} from '@bjorn3/browser_wasi_shim';

export class LuaWasiRuntime {
  constructor() {
    this.wasi = new WASI([], [], [
      new OpenFile(new File([])),
      ConsoleStdout.lineBuffered(text => console.log(text)),
      ConsoleStdout.lineBuffered(text => console.error(text))
    ], {debug: false});
    this.instance = null;
    this.failure = null;
  }

  /** Bind linear memory and run C static constructors once, before wasm-bindgen calls the game's entry point. */
  initialize(instance) {
    if(this.instance === instance) {
      if(this.failure) {
        throw this.failure;
      }
      return;
    }
    if(this.instance) {
      throw new Error('A Lua WASI runtime belongs to one WebAssembly instance.');
    }
    if(typeof instance.exports.__wasm_call_ctors !== 'function') {
      throw new Error('The Lua browser module is missing its C constructors. Rebuild with the Joy web builder.');
    }

    // Record the owner before calling constructors, so reentrant initialization cannot construct the C runtime twice.
    this.instance = instance;
    try {
      // Bind only memory; WASI.initialize would otherwise also invoke an optional reactor _initialize export.
      this.wasi.initialize({exports: {memory: instance.exports.memory}});
      instance.exports.__wasm_call_ctors();
    } catch(error) {
      this.failure = error;
      throw error;
    }
  }
}

const runtime = new LuaWasiRuntime();
export const initializeWasi = instance => runtime.initialize(instance);
export const environ_get = (...args) => runtime.wasi.wasiImport.environ_get(...args);
export const environ_sizes_get = (...args) => runtime.wasi.wasiImport.environ_sizes_get(...args);
export const clock_time_get = (...args) => runtime.wasi.wasiImport.clock_time_get(...args);
export const fd_close = (...args) => runtime.wasi.wasiImport.fd_close(...args);
export const fd_fdstat_get = (...args) => runtime.wasi.wasiImport.fd_fdstat_get(...args);
export const fd_seek = (...args) => runtime.wasi.wasiImport.fd_seek(...args);
export const fd_write = (...args) => runtime.wasi.wasiImport.fd_write(...args);
export const proc_exit = (...args) => runtime.wasi.wasiImport.proc_exit(...args);
export const fd_fdstat_set_flags = (...args) => runtime.wasi.wasiImport.fd_fdstat_set_flags(...args);
export const fd_prestat_get = (...args) => runtime.wasi.wasiImport.fd_prestat_get(...args);
export const fd_prestat_dir_name = (...args) => runtime.wasi.wasiImport.fd_prestat_dir_name(...args);
export const fd_read = (...args) => runtime.wasi.wasiImport.fd_read(...args);
export const fd_renumber = (...args) => runtime.wasi.wasiImport.fd_renumber(...args);
export const path_open = (...args) => runtime.wasi.wasiImport.path_open(...args);
export const path_remove_directory = (...args) => runtime.wasi.wasiImport.path_remove_directory(...args);
export const path_rename = (...args) => runtime.wasi.wasiImport.path_rename(...args);
export const path_unlink_file = (...args) => runtime.wasi.wasiImport.path_unlink_file(...args);
export const random_get = (...args) => runtime.wasi.wasiImport.random_get(...args);
