// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type { Device, Buffer } from '@luma.gl/core';

export interface DynamicGpuBufferOptions {
  usage: number;
  initialByteLength?: number;
  id?: string;
  onReplace?: (buffer: Buffer) => void;
}

/**
 * A reusable luma.gl buffer that grows geometrically and keeps its allocation
 * stable between writes. The wrapper owns every buffer it creates.
 */
export class DynamicGpuBuffer {
  declare device: Device;
  declare usage: number;
  declare id: string;
  declare onReplace: (buffer: Buffer) => void;
  declare buffer: Buffer;

  /**
   * @param device
   * @param options
   */
  constructor(device: Device, { usage, initialByteLength = 64 * 1024, id = 'dynamic-buffer', onReplace = () => {} }: DynamicGpuBufferOptions) {
    if(!Number.isInteger(initialByteLength) || initialByteLength <= 0) {
      throw new RangeError('Dynamic GPU buffer size must be a positive integer.');
    }

    // Borrowed device; replacement notifications let dependent vertex arrays rebind.
    this.device = device;
    this.usage = usage;
    this.id = id;
    this.onReplace = onReplace;
    this.buffer = this.createBuffer(initialByteLength);

    this.onReplace(this.buffer);
  }

  /** @param byteLength */
  createBuffer(byteLength: number) {
    return this.device.createBuffer({ id: this.id, byteLength, usage: this.usage });
  }

  /**
   * Write data, replacing the GPU allocation only when its capacity is exceeded.
   * @param data
   */
  write(data: ArrayBuffer | ArrayBufferView) {
    if(data.byteLength > this.buffer.byteLength) {
      this.resize(data.byteLength);
    }
    if(data.byteLength > 0) {
      this.buffer.write(data);
    }
  }

  /** Replace the owned GPU buffer with the next power-of-two capacity.
   * @param requiredByteLength
   */
  resize(requiredByteLength: number) {
    const maximumByteLength = this.device.limits?.maxBufferSize ?? Number.MAX_SAFE_INTEGER;
    if(requiredByteLength > maximumByteLength) {
      throw new RangeError(`GPU buffer requires ${requiredByteLength} bytes, exceeding the device limit of ${maximumByteLength}.`);
    }
    let byteLength = this.buffer.byteLength;
    while(byteLength < requiredByteLength) {
      byteLength = Math.min(byteLength*2, maximumByteLength);
    }
    const previousBuffer = this.buffer;
    this.buffer = this.createBuffer(byteLength);
    // Rebind dependents before releasing the allocation they previously borrowed.
    this.onReplace(this.buffer);
    previousBuffer.destroy();
  }

  /** Release the current allocation; the borrowed device remains alive. */
  destroy() {
    this.buffer.destroy();
  }
}
