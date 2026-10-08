// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { ParticleEffectAsset } from 'joy-engine';

import { compileParticleEffect } from 'joy-engine';

export const MAX_ASSET_BYTES = 6_000_000;

/**
 * Parse and fully validate a portable particle asset.
 * The shared compiler is the authority for every nested emitter and script.
 *
 * @param text
 */
export function readAsset(text: string): ParticleEffectAsset {
  if(new Blob([text]).size > MAX_ASSET_BYTES) {
    throw new Error(`Particle assets cannot exceed ${MAX_ASSET_BYTES.toLocaleString()} bytes.`);
  }

  const value = JSON.parse(text);
  compileParticleEffect(value);
  return structuredClone(((value) as ParticleEffectAsset));
}

/**
 * @param asset
 */
export function writeAsset(asset: ParticleEffectAsset): string {
  return `${JSON.stringify(asset, null, 2)}\n`;
}

/** @param name */
export function fileStem(name: string) {
  return name.toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '') || 'particle-effect';
}

/** @param name @param text */
export function downloadAsset(name: string, text: string) {
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = `${fileStem(name)}.joyfx`;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
