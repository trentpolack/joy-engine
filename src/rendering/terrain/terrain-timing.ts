// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  Device,
  QuerySet,
  RenderPassProps
} from '@luma.gl/core';

const TIMESTAMP_QUERY_COUNT = 2;
const SAMPLE_INTERVAL_MILLISECONDS = 500;
const TIMING_STATUS = {
  AWAITING: 'awaiting timestamp sample',
  COMPLETE: 'render pass (compute excluded)',
  UNSUPPORTED: 'unsupported'
} as const;

/**
 * TerrainTiming Class Definition.
 *  Owns optional timestamp profiling for a borrowed terrain render pass.
 *  One query pair is reused, at most one asynchronous read remains pending,
 *  and no rendered frame waits for a result. Destroy before the borrowed device.
 */
export class TerrainTiming {
  private query: QuerySet | null;
  private pending = false;
  private active = false;
  private lastSampleMilliseconds = -Infinity;
  private disposed = false;

  milliseconds: number | null = null;
  status: string;

  /**
   * Create a timing owner for a borrowed rendering device.
   * @param device Borrowed device that must outlive this timing owner.
   */
  constructor(device: Device) {
    this.query = device.features.has('timestamp-query')
      ? device.createQuerySet({
        type: 'timestamp',
        count: TIMESTAMP_QUERY_COUNT
      })
      : null;
    this.status = this.query ? TIMING_STATUS.AWAITING : TIMING_STATUS.UNSUPPORTED;
  }

  /**
   * Reserve a timestamp sample for the caller's next render pass.
   * Calls made while another sample is active, pending, or inside the sampling interval are ignored.
   * @param nowMilliseconds Monotonic frame timestamp in milliseconds.
   * @returns Borrowed-query render-pass properties, or an empty record when no sample is due.
   */
  begin(nowMilliseconds: number): Partial<RenderPassProps> {
    const insideSampleInterval = (nowMilliseconds - this.lastSampleMilliseconds) < SAMPLE_INTERVAL_MILLISECONDS;
    if(!this.query || this.pending || this.active || this.disposed || insideSampleInterval) {
      return {};
    }

    this.active = true;
    this.lastSampleMilliseconds = nowMilliseconds;

    return {
      timestampQuerySet: this.query,
      beginTimestampIndex: 0,
      endTimestampIndex: 1
    };
  }

  /**
   * Read the active sample after its pass is submitted without blocking rendering.
   * Failures become diagnostic status text; results arriving after disposal are ignored.
   */
  async end(): Promise<void> {
    if(!this.active || !this.query || this.disposed) {
      return;
    }

    this.active = false;
    this.pending = true;

    try {
      const durationMilliseconds = await this.query.readTimestampDuration(0, 1);
      if(!this.disposed) {
        this.milliseconds = durationMilliseconds;
        this.status = TIMING_STATUS.COMPLETE;
      }
    } catch(error) {
      if(!this.disposed) {
        this.status = `timestamp read failed: ${String(error)}`;
      }
    } finally {
      this.pending = false;
    }
  }

  /**
   * Release the owned query and its staging resources; pending results are ignored.
   */
  destroy(): void {
    if(this.disposed) {
      return;
    }

    this.disposed = true;
    this.query?.destroy();
    this.query = null;
  }
}
