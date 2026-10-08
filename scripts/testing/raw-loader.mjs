// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { readFileSync } from 'node:fs';

const RAW_SUFFIX = '?raw';

/** Allow Node-based engine tests to consume the same authored text assets as Vite. */
export function resolve(specifier, context, nextResolve) {
  if(!specifier.endsWith(RAW_SUFFIX)) {
    return nextResolve(specifier, context);
  }
  const resolved = nextResolve(specifier.slice(0, -RAW_SUFFIX.length), context);
  return { ...resolved, shortCircuit: true };
}

/** Return asset text as an ES module; production builds use Vite's native raw imports. */
export function load(url, context, nextLoad) {
  if(!/\.(?:glsl|wgsl|css|joyfx|form)$/.test(new URL(url).pathname)) {
    return nextLoad(url, context);
  }
  const source = readFileSync(new URL(url), 'utf8');
  return { format: 'module', source: `export default ${JSON.stringify(source)};`, shortCircuit: true };
}
