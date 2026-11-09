// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// One thread of real work inside each frame profile mark. The profiler writes its GPU timestamps as compute passes
// begin, and Metal samples a pass's counters only when its encoder executes something: an empty pass left every Apple
// GPU timestamp at zero. Counting marks gives the pass a side effect no driver may skip; nothing reads the count.

@group(0) @binding(0) var<storage, read_write> marks: atomic<u32>;

@compute @workgroup_size(1)
fn mark_main() {
    atomicAdd(&marks, 1u);
}
