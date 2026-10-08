// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/** Read a local draft, treating blocked browser storage as an absent draft. @param key */
export function readStoredText(key: string) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** Persist a local draft without interrupting editing when storage is unavailable.
 * @param key @param value
 * @returns Whether browser storage accepted the draft.
 */
export function storeText(key: string, value: string): boolean {
  try {
    localStorage.setItem(key, value);
    return true;
  } catch {
    return false;
  }
}
