// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {COMPILING_PHASE} from './compiler-protocol.ts';
import { compile } from 'joy-engine/form';
import { ScriptError } from 'joy-engine/form';
// Worker owns the interpreter. The editor can terminate it at any point.
self.onmessage = event => {
  self.postMessage({phase:COMPILING_PHASE});
  const start = performance.now();
  try {
    const data = compile(event.data.source, event.data.overrides, event.data.materials);
    self.postMessage({ok: true, data, milliseconds: performance.now() - start});
  } catch(error) {
    self.postMessage({
      ok: false,
      message: error instanceof Error ? error.message : String(error),
      line: error instanceof ScriptError ? error.line : 1,
      column: error instanceof ScriptError ? error.column : 1
    });
  }
};
