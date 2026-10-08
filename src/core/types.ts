// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import { MATERIAL_ALPHA_MODE } from '../rendering/pbr/constants.ts';

/** Shared numeric contracts. Units and gameplay meaning belong to each caller. */
export interface Vector2 {
  x: number;
  y: number;
}

export interface Velocity2 {
  velocityX: number;
  velocityY: number;
}

export type MovingBody = Vector2 & Velocity2;
export type RgbaColor = readonly [number, number, number, number];

export interface IndexedMesh3D {
  name: string;
  positions: readonly number[];
  normals: readonly number[];
  indices: readonly number[];
}

/** A triangle stores three x/y/z positions followed by a face brightness multiplier. */
export type ShadedTriangle3D = readonly [number, number, number, number, number, number, number, number, number, number];

export interface ShadedMesh3D {
  name: string;
  triangles: readonly ShadedTriangle3D[];
}

/**
 * Metallic/roughness material. Factors are linear, colors have RGB or RGBA order.
 * Texture URLs are borrowed sources; the renderer owns decoded GPU textures.
 * Color maps use sRGB; normal and scalar maps use linear data (scalar red channel).
 */
export interface PbrMaterial {
  baseColor: readonly number[];
  emissive: readonly number[];
  metallic: number;
  roughness: number;
  alphaMode: typeof MATERIAL_ALPHA_MODE.OPAQUE | typeof MATERIAL_ALPHA_MODE.MASK | typeof MATERIAL_ALPHA_MODE.BLEND;
  alphaCutoff: number;
  textures: {
    baseColor: string | null;
    normal: string | null;
    emissive: string | null;
    metallic: string | null;
    roughness: string | null;
    ao: string | null;
  };
}

/** Indexed, world-space Y-up geometry. Zero normals request a flat face normal. */
export interface PbrMesh {
  positions: readonly number[];
  normals: readonly number[];
  uvs: readonly number[];
  colors: readonly number[];
  alphas: readonly number[];
  triangles: readonly number[];
  triangleMaterials: readonly number[];
  materials: readonly PbrMaterial[];
}

/** World-space camera and lighting for a material pass. RGB light values are linear radiance. */
export interface PbrFrame {
  viewProjection: Float32Array | number[];
  eye: readonly number[];
  backward: readonly number[];
  lightDirection?: readonly number[];
  lightColor?: readonly number[];
  ambientColor?: readonly number[];
}

/** Local Y-up indexed mesh input. Optional attributes are empty or vertex-aligned. */
export interface MeshAssetData {
  positions: readonly number[];
  indices: readonly number[];
  normals?: readonly number[];
  uvs?: readonly number[];
  colors?: readonly number[];
  alphas?: readonly number[];
  triangleMaterials?: readonly number[];
  materials?: readonly unknown[];
  metadata?: Readonly<Record<string, unknown>>;
}

/** Mutable instance settings. Euler angles are radians, applied X then Y then Z. */
export interface MeshInstanceOptions {
  position?: readonly number[];
  rotation?: readonly number[];
  scale?: readonly number[];
  tint?: readonly number[];
  opacity?: number;
  visible?: boolean;
  pass?: 'auto' | 'opaque' | 'transparent';
  /** Perceptual surface roughness for screen-space effects, zero polished to one matte. */
  sceneRoughness?: number;
}

/** Ordered X/Y/Z components in the coordinate space documented by the consuming API. */
export type Vector3 = [number, number, number];
