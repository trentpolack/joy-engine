// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/** Original procedural 16×16 white cross with transparent corners. */
export const DEFAULT_PARTICLE_TEXTURE = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAJElEQVR4nGP4//8/Aw6MDrCqw6V51AASDaAIDBMDBj4WRroBAMgTbL72rES3AAAAAElFTkSuQmCC';

/** Validate an embedded PNG header before any browser decoding or GPU allocation.
 * Returns pixel dimensions; does not decode pixels or allocate host resources.
 * The resource owner must still handle asynchronous image decoding failures.
 * @param source */
export function readParticleTextureSource(source: unknown): {
    width: number;
    height: number;
} {
  const prefix = 'data:image/png;base64,';
  if(typeof source !== 'string' || !source.startsWith(prefix)) {
    throw new Error('Particle texture source must be an embedded PNG data URL.');
  }
  const encoded = source.slice(prefix.length);
  if(encoded.length > Math.ceil(1024 * 1024 / 3) * 4 ||
    !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(encoded)) {
    throw new Error('Particle PNG must be valid base64 and at most 1 MiB.');
  }
  const bytes = atob(encoded);
  if(bytes.length > 1024 * 1024) {
    throw new Error('Particle PNG must be at most 1 MiB.');
  }
  if(bytes.length < 33 ||
    bytes.slice(0, 8) !== '\x89PNG\r\n\x1a\n' ||
    readUint32(bytes, 8) !== 13 || bytes.slice(12, 16) !== 'IHDR') {
    throw new Error('Particle texture has an invalid PNG signature or IHDR header.');
  }
  const width = readUint32(bytes, 16);
  const height = readUint32(bytes, 20);
  if(width < 1 || height < 1 || width > 2048 || height > 2048) {
    throw new Error('Particle PNG dimensions must be 1–2048 pixels per axis.');
  }
  return { width, height };
}

/** @param bytes @param offset */
function readUint32(bytes: string, offset: number) {
  return bytes.charCodeAt(offset) * 0x1000000 +
    bytes.charCodeAt(offset + 1) * 0x10000 +
    bytes.charCodeAt(offset + 2) * 0x100 + bytes.charCodeAt(offset + 3);
}
