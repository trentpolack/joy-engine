// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Device, RenderPassProps, QuerySet } from '@luma.gl/core';

/**
 * Optional timestamp profiling for the borrowed terrain render pass. One query
 * pair is reused, with at most one asynchronous read pending and one sample per
 * 500 ms. No frame waits on results. Query-owned staging buffers are retained.
 */

export class TerrainTiming {
  declare query: QuerySet | null;
  declare milliseconds: number | null;
  declare status: string;
  declare pending: boolean;
  declare active: boolean;
  declare lastSample: number;
  declare disposed: boolean;

  /** @param device */
  constructor(device: Device) {
    // The caller supplies the render pass and submission; this owner only
    // manages the query pair and the latest completed duration in milliseconds.
    this.query = device.features.has('timestamp-query') ? device.createQuerySet({type:'timestamp',count:2}) : null;
    this.milliseconds = ((null) as number | null);
    this.status = this.query ? 'awaiting timestamp sample' : 'unsupported';
    this.pending = false;
    this.active = false;
    this.lastSample = -Infinity;
    this.disposed = false;
  }
  /**
   * Reserve a sample and return timestamp properties for the caller's next pass.
   * @param nowMilliseconds */
  begin(nowMilliseconds: number): Partial<RenderPassProps> {
    // Skip instead of queueing reads: one outstanding sample bounds staging
    // work and prevents a slow GPU from accumulating promises every frame.
    if(!this.query || this.pending || this.active || this.disposed || nowMilliseconds - this.lastSample < 500) { return {}; }
    this.active = true;
    this.lastSample = nowMilliseconds;
    return {timestampQuerySet:this.query,beginTimestampIndex:0,endTimestampIndex:1};
  }
  /**
   * Call after submitting the sampled pass. Resolves without blocking rendering.
   */
  async end() {
    if(!this.active || !this.query || this.disposed) { return; }
    this.active = false;
    this.pending = true;
    try {
      const value = await this.query.readTimestampDuration(0,1);
      // A result can arrive after destroy(); never publish it into a dead owner.
      if(!this.disposed) { this.milliseconds = value; this.status = 'render pass (compute excluded)'; }
    } catch(error) {
      if(!this.disposed) { this.status = `timestamp read failed: ${String(error)}`; }
    } finally { this.pending = false; }
  }
  /**
   * Release the owned query and its staging resources; pending results are ignored.
   */
  destroy() {
    if(this.disposed) { return; }
    this.disposed = true; this.query?.destroy();
  }
}
