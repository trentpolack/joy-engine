// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

/** Borrowed luma counter storage. Reading it must never create or reset counters. */
interface CounterGroup {
  stats: Record<string, {count: number} | undefined>;
}
interface DiagnosticDevice {
  statsManager: {stats: ReadonlyMap<string, CounterGroup>};
}

export interface RendererDiagnosticOptions {
  memory?: boolean;
  resources?: boolean;
}
export interface RendererDiagnostics {
  /** Device.statsManager is shared by all luma devices in this JavaScript realm. */
  scope: 'all-luma-devices';
  /** Bytes tracked by luma; excludes untracked driver/browser allocations. Null means unavailable. */
  memory: {allocatedBytes: number | null; bufferBytes: number | null; textureBytes: number | null} | null;
  /** Current active and lifetime-created handles, not draw calls. Null means unavailable. */
  resources: Record<string, {active: number | null; created: number | null}> | null;
  /** Existing scene culling counts for this renderer's most recent rendered frame. */
  scene?: {visible: number; culled: number};
}
export interface RendererDiagnosticSource {
  /** Copy requested counters on demand; callers own the returned snapshot. */
  getDiagnostics(options?: RendererDiagnosticOptions): RendererDiagnostics | null;
}

const RESOURCE_COUNTS = 'GPU Resource Counts';
const TIME_AND_MEMORY = 'GPU Time and Memory';
const RESOURCE_NAMES = ['Resources', 'Buffers', 'Textures', 'RenderPipelines', 'ComputePipelines', 'Framebuffers'] as const;

/**
 * Copy existing luma 9.4 counters without creating stats, resetting shared values,
 * subscribing to frames, or querying the GPU. Each requested section is independent.
 * @param device Borrowed device; its stats manager covers all luma devices in this realm.
 * @param options Opt into memory/resource reads only while their detail views are open.
 */
export function readGpuDiagnostics(device: DiagnosticDevice, options: RendererDiagnosticOptions = {}): RendererDiagnostics {
  const snapshot: RendererDiagnostics = {scope: 'all-luma-devices', memory: null, resources: null};
  if(options.memory) {
    const counters = device.statsManager.stats.get(TIME_AND_MEMORY);
    snapshot.memory = {
      allocatedBytes: readCounter(counters, 'GPU Memory'),
      bufferBytes: readCounter(counters, 'Buffer Memory'),
      textureBytes: readCounter(counters, 'Texture Memory')
    };
  }
  if(options.resources) {
    const counters = device.statsManager.stats.get(RESOURCE_COUNTS);
    snapshot.resources = {};
    for(const name of RESOURCE_NAMES) {
      snapshot.resources[name] = {
        active: readCounter(counters, `${name} Active`),
        created: readCounter(counters, `${name} Created`)
      };
    }
  }
  return snapshot;
}

/** Missing or non-finite counters remain unavailable; a recorded zero stays zero. */
function readCounter(group: CounterGroup | undefined, name: string): number | null {
  const value = group?.stats[name]?.count;
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}
