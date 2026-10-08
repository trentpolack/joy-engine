// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/**
 * Create an independently owned deterministic random stream in [0, 1).
 * Keep separate instances for gameplay and presentation so changing cosmetic
 * draw counts cannot alter simulation decisions. This is not cryptographic.
 * @param seed Finite integer, normalized to an unsigned 32-bit seed.
 * @returns Source retaining only its own sequence state.
 */
export function createSeededRandom(seed: number): () => number {
  if(!Number.isInteger(seed)) {
    throw new TypeError('Random seed must be a finite integer.');
  }

  let state = seed >>> 0;

  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    return(state/4294967296);
  };
}
