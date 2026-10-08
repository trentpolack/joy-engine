// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Require a matching HTML element during startup, with a useful markup error.
 * @param root
 * @param selector
 * @param elementType
 */
export function requireElement<T extends HTMLElement>(root: Document | HTMLElement, selector: string, elementType: {
    new (...args: never[]): T;
}): T {
  const element = root.querySelector(selector);
  if(!(element instanceof elementType)) {
    throw new Error(`Missing or invalid UI element: ${selector}`);
  }

  return element;
}
