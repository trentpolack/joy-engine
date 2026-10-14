// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {createFormLibrary, openFormLibraryAsset, type FormLibraryAsset} from '../../src/form-library.ts';
import type {FormDocument} from '../../src/document.ts';

// Vite discovers authored files recursively and bundles their text for offline/production use.
// New or removed files invalidate this catalog in development; no source-writing endpoint is exposed.
const files = import.meta.glob<string>([
  '../../../../../assets/examples/**/*.{form,formlab}',
  '../../../../../assets/form/**/*.{form,formlab}'
], {eager: true, query: '?raw', import: 'default', caseSensitive: false});

const ENGINE_DIRECTORY_PREFIX = '../../../../../';
export const FORM_ASSETS: readonly FormLibraryAsset[] = createFormLibrary(Object.fromEntries(
  Object.entries(files).map(([path, text]) => [path.slice(ENGINE_DIRECTORY_PREFIX.length), text])
));
const DEFAULT_FORM_ASSET_PATH = 'assets/examples/wave-study.form';

/**
 * Preserve the initial study when available; an edited or empty library must still open.
 */
export function createInitialFormDocument(): FormDocument {
  const asset = FORM_ASSETS.find(asset => asset.path === DEFAULT_FORM_ASSET_PATH) ?? FORM_ASSETS[0];
  return asset ? openFormLibraryAsset(asset) : {version: 1, name: 'Untitled', source: '', overrides: {}};
}
