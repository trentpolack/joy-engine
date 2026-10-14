// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {readDocument, type FormDocument} from './document.ts';

const FORM_LIBRARY_PATH = /^assets\/(?:examples|form)\/.+\.(?:form|formlab)$/i;

/** Read-only bundled text; opening an asset creates independent document state. */
export interface FormLibraryAsset {
  readonly path: string;
  readonly folder: string;
  readonly filename: string;
  readonly text: string;
}

/**
 * Describe discovered FORM assets in folder order without evaluating scripts or loading GPU resources.
 * Paths are relative to Joy Engine; unrelated assets never enter the library.
 */
export function createFormLibrary(files: Readonly<Record<string, string>>): FormLibraryAsset[] {
  const assets: FormLibraryAsset[] = [];
  for(const [path, text] of Object.entries(files)) {
    if(!FORM_LIBRARY_PATH.test(path) || path.split('/').includes('..')) {
      continue;
    }
    const separator = path.lastIndexOf('/');
    assets.push({path, folder: path.slice(0, separator), filename: path.slice(separator + 1), text});
  }
  return assets.sort((first, second) => first.path.localeCompare(second.path, 'en'));
}

/**
 * Parse a fresh working copy on each open. Document resources and variants retain their values;
 * editing them never mutates the bundled source or a previously opened copy.
 */
export function openFormLibraryAsset(asset: FormLibraryAsset): FormDocument {
  if(asset.filename.toLowerCase().endsWith('.formlab')) {
    return readDocument(asset.text);
  }
  const name = asset.filename.replace(/\.form$/i, '').replace(/[-_]/g, ' ');
  return readDocument(JSON.stringify({version: 1, name, source: asset.text, overrides: {}}));
}
