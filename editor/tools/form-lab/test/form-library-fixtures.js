// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {readdirSync, readFileSync} from 'node:fs';
import {join, relative, sep} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createFormLibrary} from '../src/form-library.ts';

// Node tests consume the authored files directly; Vite owns browser discovery and packaging.
const assetRoot = fileURLToPath(new URL('../../../../assets/', import.meta.url));
/** @type {Record<string, string>} */
const files = {};
for(const folder of ['examples', 'form']) {
  for(const entry of readdirSync(join(assetRoot, folder), {recursive: true, withFileTypes: true})) {
    if(!entry.isFile() || !/\.(form|formlab)$/i.test(entry.name)) {
      continue;
    }
    const path = join(entry.parentPath, entry.name);
    files[`assets/${relative(assetRoot, path).split(sep).join('/')}`] = readFileSync(path, 'utf8');
  }
}
export const FORM_LIBRARY_FIXTURES = createFormLibrary(files);
