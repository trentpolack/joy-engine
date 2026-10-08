// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Choose GPU backing density independently from a game's logical viewport.
 * The caller retains CSS-sized world and UI coordinates; only raster quality changes with display density.
 * The pixel budget prevents oversized textures.
 * @param width Logical width in CSS pixels.
 * @param height Logical height in CSS pixels.
 * @param devicePixelRatio Physical pixels per CSS pixel.
 * @param [limits] Maximum Pixel Ratio and Maximum Backing Pixels.
 * @returns The calculated backing pixel ratio.
 */
export function calculateBackingPixelRatio(width: number, height: number, devicePixelRatio: number, limits: {
    maximumPixelRatio?: number;
    maximumBackingPixels?: number;
} = {}) {
  const maximumPixelRatio = limits.maximumPixelRatio ?? 2;
  const maximumBackingPixels = limits.maximumBackingPixels ?? 3840*2160;
  const logicalPixels = Math.max(1, width)*Math.max(1, height);

  return(Math.max(0.01, Math.min(devicePixelRatio, maximumPixelRatio, Math.sqrt(maximumBackingPixels/logicalPixels))));
}
