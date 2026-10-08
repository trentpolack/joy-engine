// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Validate a project-relative authored asset reference without performing I/O.
 * @param value @param [extensions]
 */
export function assetPath(value: unknown, extensions: readonly string[] = []): string {
  if(typeof value !== 'string' || !value.startsWith('assets/') || /[\\?#%:\u0000-\u001f]/.test(value) ||
    value.split('/').some(part => !part || part === '.' || part === '..')) {
    throw new TypeError('Asset path must stay beneath assets/.');
  }
  if(extensions.length && !extensions.some(extension => value.toLowerCase().endsWith(extension))) {
    throw new TypeError(`Asset path must use ${extensions.join(' or ')}.`);
  }
  return value;
}
