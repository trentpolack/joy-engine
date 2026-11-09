// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// GPU back-to-front ordering of particle instances: a bitonic sort over one key per instance, then a scatter that
// writes the instances into the drawing buffer in sorted order (pipelines/particle_sort.rs).
//
// Every key is (segment, depth, index), compared in that order. The segment is the instance's prepared batch, so
// batches keep their place in the buffer; the depth key orders a source-over batch's particles farthest first (additive
// batches use zero, keeping generation order); and the index (the instance's upload position) breaks ties, so equal
// depths keep generation order exactly as the CPU sort does. Keys are unique, so the order is total and deterministic.
//
// The key count is padded to a power of two (at least one block) with keys that sort after every instance. Blocks of
// SORT_BLOCK keys sort in workgroup memory (`sortBlocks`); each wider merge stage then runs its long-distance steps one
// dispatch at a time (`mergeStep`) and finishes the steps within a block in workgroup memory (`mergeBlocks`), so a
// frame of n particles needs about log2(n / SORT_BLOCK)^2 / 2 dispatches rather than log2(n)^2 / 2.

// Threads per workgroup, and keys a workgroup sorts in workgroup memory (two per thread); must match particle_sort.rs.
const SORT_THREADS: u32 = 256u;
const SORT_BLOCK: u32 = 512u;

// Words per instance (`ParticleInstance` in particles/instance.rs is 64 bytes).
const INSTANCE_WORDS: u32 = 16u;

// The segment flag marking an additive batch, whose instances keep generation order.
const UNSORTED_SEGMENT: u32 = 0x80000000u;

// Padding keys sort after every instance.
const PADDING: u32 = 0xffffffffu;

struct SortKey {
  segment: u32,
  depth: u32,
  index: u32,
  unused: u32,
};

struct SortView {
  // XYZ the eye; depth is the distance in front of it along `forward`.
  eye: vec4f,
  forward: vec4f,
  // X instances, Y padded key count, ZW unused.
  counts: vec4u,
};

// One merge stage: X its width k (the bitonic sequence length being merged), Y the compare distance j, ZW unused.
struct SortStage {
  widths: vec4u,
};

struct InstanceWords {
  words: array<u32, INSTANCE_WORDS>,
};

@group(0) @binding(0) var<uniform> view: SortView;
@group(0) @binding(1) var<storage, read> sourceInstances: array<InstanceWords>;
@group(0) @binding(2) var<storage, read> segments: array<u32>;
@group(0) @binding(3) var<storage, read_write> keys: array<SortKey>;
@group(0) @binding(4) var<storage, read_write> sortedInstances: array<InstanceWords>;
@group(1) @binding(0) var<uniform> stage: SortStage;

var<workgroup> blockKeys: array<SortKey, SORT_BLOCK>;

/**
 * A float's bits reordered so unsigned comparison follows the float's total order (negative values flip every bit,
 * others only the sign), matching `f32::total_cmp`.
 */
fn orderedFloat(value: f32) -> u32 {
  let bits = bitcast<u32>(value);
  return select(bits | 0x80000000u, ~bits, (bits & 0x80000000u) != 0u);
}

/**
 * Whether `left` sorts strictly before `right`.
 */
fn keyBefore(left: SortKey, right: SortKey) -> bool {
  if(left.segment != right.segment) {
    return left.segment < right.segment;
  }
  if(left.depth != right.depth) {
    return left.depth < right.depth;
  }
  return left.index < right.index;
}

/**
 * The lower element of compare pair `pair` at distance `distance`: pairs fill each run of 2 * distance elements.
 */
fn pairStart(pair: u32, distance: u32) -> u32 {
  return ((pair/distance)*(2u*distance)) + (pair%distance);
}

/**
 * One key per padded slot: instances by segment, depth (farthest first for source-over batches), and upload position;
 * padding after everything.
 */
@compute @workgroup_size(SORT_THREADS)
fn computeKeys(@builtin(global_invocation_id) id: vec3u) {
  let index = id.x;
  if(index >= view.counts.y) {
    return;
  }
  var key: SortKey;
  key.index = index;
  key.unused = 0u;
  if(index >= view.counts.x) {
    key.segment = PADDING;
    key.depth = PADDING;
    keys[index] = key;
    return;
  }
  let segment = segments[index];
  key.segment = segment & ~UNSORTED_SEGMENT;
  key.depth = 0u;
  if((segment & UNSORTED_SEGMENT) == 0u) {
    let words = sourceInstances[index].words;
    let position = vec3f(bitcast<f32>(words[0]), bitcast<f32>(words[1]), bitcast<f32>(words[2]));
    // Farthest first: the complement of the ordered depth sorts larger depths earlier.
    key.depth = ~orderedFloat(dot(position - view.eye.xyz, view.forward.xyz));
  }
  keys[index] = key;
}

/**
 * Order the pair at `lower` and `lower + distance` of the block in workgroup memory for merge width `width`, whose
 * direction follows the element's global position `blockStart + lower`.
 */
fn exchangeInBlock(lower: u32, distance: u32, width: u32, blockStart: u32) {
  let upper = lower + distance;
  let ascending = ((blockStart + lower) & width) == 0u;
  let first = blockKeys[lower];
  let second = blockKeys[upper];
  if(keyBefore(second, first) == ascending) {
    blockKeys[lower] = second;
    blockKeys[upper] = first;
  }
}

/**
 * Copy this workgroup's block of keys into workgroup memory.
 */
fn loadBlock(thread: u32, blockStart: u32) {
  blockKeys[thread] = keys[blockStart + thread];
  blockKeys[thread + SORT_THREADS] = keys[blockStart + thread + SORT_THREADS];
  workgroupBarrier();
}

/**
 * Copy this workgroup's block of keys back.
 */
fn storeBlock(thread: u32, blockStart: u32) {
  keys[blockStart + thread] = blockKeys[thread];
  keys[blockStart + thread + SORT_THREADS] = blockKeys[thread + SORT_THREADS];
}

/**
 * Every merge stage up to the block's width, inside each block.
 */
@compute @workgroup_size(SORT_THREADS)
fn sortBlocks(@builtin(local_invocation_id) local: vec3u, @builtin(workgroup_id) group: vec3u) {
  let thread = local.x;
  let blockStart = group.x*SORT_BLOCK;
  loadBlock(thread, blockStart);
  for(var width = 2u; width <= SORT_BLOCK; width = width*2u) {
    for(var distance = width/2u; distance > 0u; distance = distance/2u) {
      exchangeInBlock(pairStart(thread, distance), distance, width, blockStart);
      workgroupBarrier();
    }
  }
  storeBlock(thread, blockStart);
}

/**
 * One step of a merge wider than a block: compare distance at least one block, so pairs span workgroups.
 */
@compute @workgroup_size(SORT_THREADS)
fn mergeStep(@builtin(global_invocation_id) id: vec3u) {
  let width = stage.widths.x;
  let distance = stage.widths.y;
  let lower = pairStart(id.x, distance);
  let upper = lower + distance;
  if(upper >= view.counts.y) {
    return;
  }
  let ascending = (lower & width) == 0u;
  let first = keys[lower];
  let second = keys[upper];
  if(keyBefore(second, first) == ascending) {
    keys[lower] = second;
    keys[upper] = first;
  }
}

/**
 * The remaining steps of a merge wider than a block, whose compare distances now fit inside each block.
 */
@compute @workgroup_size(SORT_THREADS)
fn mergeBlocks(@builtin(local_invocation_id) local: vec3u, @builtin(workgroup_id) group: vec3u) {
  let thread = local.x;
  let blockStart = group.x*SORT_BLOCK;
  let width = stage.widths.x;
  loadBlock(thread, blockStart);
  for(var distance = SORT_BLOCK/2u; distance > 0u; distance = distance/2u) {
    exchangeInBlock(pairStart(thread, distance), distance, width, blockStart);
    workgroupBarrier();
  }
  storeBlock(thread, blockStart);
}

/**
 * Write each instance into the drawing buffer at its sorted position.
 */
@compute @workgroup_size(SORT_THREADS)
fn scatterInstances(@builtin(global_invocation_id) id: vec3u) {
  let index = id.x;
  if(index >= view.counts.x) {
    return;
  }
  sortedInstances[index] = sourceInstances[keys[index].index];
}
