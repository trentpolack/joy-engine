// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { RgbaColor } from './types.ts';

/**
 * Return a new straight-alpha color without mutating the borrowed palette color.
 * @param color The original RGBA color.
 * @param alpha The new alpha value.
 * @returns A new RGBA color with the specified alpha.
 */
export function withAlpha(color: RgbaColor, alpha: number): RgbaColor {
  return([color[0], color[1], color[2], alpha]);
}

/**
 * Decode #RGB or #RRGGBB notation into normalized RGBA.
 * Unsupported forms return null so callers can select their own fallback.
 * @param value The hex color string to parse.
 * @returns The parsed RGBA color or null if the input is invalid.
 */
export function parseHexColor(value: string | undefined): RgbaColor | null {
  if(!value || (!/^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i.test(value))) {
    return null;
  }

  const hex = (value.length === 4) ? (value.slice(1).split('').map((digit) => digit + digit).join('')) : value.slice(1);

  return([
    Number.parseInt(hex.slice(0, 2), 16)/255,
    Number.parseInt(hex.slice(2, 4), 16)/255,
    Number.parseInt(hex.slice(4, 6), 16)/255,
    1
  ]);
}
