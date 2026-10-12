// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import {MeshAsset} from 'joy-engine';
import type {MeshInstance, RenderScene, Vector3} from 'joy-engine';
import {IMPACT_DEBUG_CONFIG} from './config/destruction-config.ts';

type Marker = {mesh: MeshInstance; ageSeconds: number};
type MarkerGeometry = {positions: number[]; indices: number[]; colors: number[]};
const SEGMENT_INDICES = [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4,
  1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];
const DEBUG_RENDER_PASS = 'transparent';

/**
 * Owns bounded world-space hit, incoming-ray, and damage-radius debug meshes.
 * Borrows the scene; all geometry is local to each impact. Dispose before destroying the scene.
 */
export class ImpactDebugDraw {
  private scene: RenderScene;
  private markers: Marker[] = [];
  private enabled: boolean = IMPACT_DEBUG_CONFIG.enabled;

  constructor(scene: RenderScene) {
    this.scene = scene;
  }

  /**
   * Draw one hit in meters using a normalized world-space incoming direction and damage radius.
   */
  add(point: Vector3, direction: readonly number[], radius: number) {
    if(!this.enabled) {
      return;
    }
    if(this.markers.length === IMPACT_DEBUG_CONFIG.maximumMarkers) {
      this.removeMarker(0);
    }
    const position = point.map((value, axis) => value - direction[axis]*IMPACT_DEBUG_CONFIG.surfaceOffset);
    const mesh = this.scene.createMesh(createMarkerAsset(direction, radius), {
      position,
      // The transparent scene pass is unlit but remains depth-tested against the structure.
      pass: DEBUG_RENDER_PASS
    });
    this.markers.push({mesh, ageSeconds: 0});
  }

  /**
   * Advance marker lifetime by elapsed seconds and fade only near expiry.
   */
  update(elapsedSeconds: number) {
    for(let index = this.markers.length - 1; index >= 0; index--) {
      const marker = this.markers[index];
      marker.ageSeconds+= elapsedSeconds;
      const remaining = IMPACT_DEBUG_CONFIG.lifetimeSeconds - marker.ageSeconds;
      if(remaining <= 0) {
        this.removeMarker(index);
        continue;
      }
      marker.mesh.opacity = Math.min(1, remaining/IMPACT_DEBUG_CONFIG.fadeSeconds);
    }
  }

  /**
   * Toggle diagnostic drawing; disabling releases existing markers immediately.
   */
  setEnabled(enabled: boolean) {
    this.enabled = enabled;
    if(!enabled) {
      this.clear();
    }
  }

  /**
   * Copied diagnostic visibility and live marker count.
   */
  snapshot() {
    return {enabled: this.enabled, markers: this.markers.length};
  }

  /**
   * Release all impact meshes while retaining the selected visibility preference.
   */
  clear() {
    while(this.markers.length) {
      this.removeMarker(this.markers.length - 1);
    }
  }

  /**
   * Release all scene resources. Safe to repeat.
   */
  dispose() {
    this.clear();
  }

  private removeMarker(index: number) {
    this.scene.remove(this.markers[index].mesh);
    this.markers.splice(index, 1);
  }
}

function createMarkerAsset(direction: readonly number[], radius: number) {
  const geometry: MarkerGeometry = {positions: [], indices: [], colors: []};
  const origin: Vector3 = [0, 0, 0];
  const rayStart = direction.map(value => -value*IMPACT_DEBUG_CONFIG.rayLength);
  appendSegment(geometry, rayStart, origin, IMPACT_DEBUG_CONFIG.rayColor);

  // Three great circles show the spherical damage volume from any orbit angle.
  for(let normalAxis = 0; normalAxis < 3; normalAxis++) {
    const firstAxis = (normalAxis + 1)%3;
    const secondAxis = (normalAxis + 2)%3;
    for(let segment = 0; segment < IMPACT_DEBUG_CONFIG.ringSegments; segment++) {
      const firstAngle = segment*Math.PI*2/IMPACT_DEBUG_CONFIG.ringSegments;
      const secondAngle = (segment + 1)*Math.PI*2/IMPACT_DEBUG_CONFIG.ringSegments;
      const first = [0, 0, 0];
      const second = [0, 0, 0];
      first[firstAxis] = Math.cos(firstAngle)*radius;
      first[secondAxis] = Math.sin(firstAngle)*radius;
      second[firstAxis] = Math.cos(secondAngle)*radius;
      second[secondAxis] = Math.sin(secondAngle)*radius;
      appendSegment(geometry, first, second, IMPACT_DEBUG_CONFIG.radiusColor);
    }
  }
  for(let axis = 0; axis < 3; axis++) {
    const first = [0, 0, 0];
    const second = [0, 0, 0];
    first[axis] = -IMPACT_DEBUG_CONFIG.pointSize/2;
    second[axis] = IMPACT_DEBUG_CONFIG.pointSize/2;
    appendSegment(geometry, first, second, IMPACT_DEBUG_CONFIG.pointColor);
  }
  return new MeshAsset(geometry);
}

/**
 * Append a thin closed prism so diagnostics retain a visible width from any camera angle.
 */
function appendSegment(geometry: MarkerGeometry, first: readonly number[], second: readonly number[], color: readonly number[]) {
  const delta = second.map((value, axis) => value - first[axis]);
  const length = Math.hypot(...delta);
  const forward = delta.map(value => value/length);
  const reference = Math.abs(forward[1]) < 0.9 ? [0, 1, 0] : [1, 0, 0];
  const side = cross(forward, reference);
  const sideLength = Math.hypot(...side);
  const right = side.map(value => value/sideLength);
  const up = cross(right, forward);
  const halfWidth = IMPACT_DEBUG_CONFIG.lineWidth/2;
  const start = geometry.positions.length/3;

  for(const point of [first, second]) {
    for(const [rightSign, upSign] of [[-1, -1], [1, -1], [1, 1], [-1, 1]]) {
      for(let axis = 0; axis < 3; axis++) {
        geometry.positions.push(point[axis] + (right[axis]*rightSign + up[axis]*upSign)*halfWidth);
      }
      geometry.colors.push(...color);
    }
  }
  for(const index of SEGMENT_INDICES) {
    geometry.indices.push(start + index);
  }
}

function cross(first: readonly number[], second: readonly number[]) {
  return [
    first[1]*second[2] - first[2]*second[1],
    first[2]*second[0] - first[0]*second[2],
    first[0]*second[1] - first[1]*second[0]
  ];
}
