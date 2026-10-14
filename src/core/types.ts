// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {SceneNode} from '../rendering/scene/scene-node.ts';
import type {PbrEnvironment} from '../rendering/environment/pbr-environment.ts';
import type {PointLight} from '../rendering/lighting/point-lights.ts';
import type {ShadowCameraFrame, ShadowOptions} from '../rendering/shadows/types.ts';
import { MATERIAL_ALPHA_MODE } from '../rendering/materials/pbr/constants.ts';

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
export type Color = readonly [number, number, number, number];

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
  /** Borrowed prepared environment; destroy it separately from the renderer. */
  environment?: PbrEnvironment;
  pointLights?: readonly PointLight[];
  /** Directional casting defaults to true when a WebGPU shadow camera is supplied. */
  castsShadow?: boolean;
  shadowCamera?: ShadowCameraFrame;
  shadows?: ShadowOptions | false;
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

/** Column-major affine basis columns followed by XYZ translation; supports shear. */
export type AffineTransform = readonly [number, number, number, number, number, number, number, number, number, number, number, number];

/** Conservative axis-aligned bounds in the coordinate space of the consuming API. */
export interface GeometryBounds {
  readonly min: readonly number[];
  readonly max: readonly number[];
}

/** Local transform settings. Euler radians apply X, then Y, then Z. Parenting preserves local values. */
export interface SceneNodeOptions {
  parent?: SceneNode | null;
  position?: readonly number[];
  rotation?: readonly number[];
  scale?: readonly number[];
  visible?: boolean;
}

/** Mesh presentation settings; the scene borrows the asset and owns the node. */
export interface MeshInstanceOptions extends SceneNodeOptions {
  tint?: readonly number[];
  opacity?: number;
  /** Solid geometry casts and receives shadows by default. Transparent geometry never casts opaque shadows. */
  castsShadow?: boolean;
  receivesShadow?: boolean;
  pass?: 'auto' | 'opaque' | 'transparent';
  /** Perceptual surface roughness for screen-space effects, zero polished to one matte. */
  sceneRoughness?: number;
}

/** Resident terrain settings. Tolerance is CSS pixels; targetRadius is world units. */
export interface TerrainInstanceOptions extends SceneNodeOptions {
  castsShadow?: boolean;
  receivesShadow?: boolean;
  tolerance?: number;
  targetRadius?: number;
  /** Zero is normal shading; positive values select the terrain adapter diagnostic. */
  diagnostic?: number;
}

/** Ordered X/Y/Z components in the coordinate space documented by the consuming API. */
export type Vector3 = [number, number, number];

/**
 * Column-major affine transform: three scaled basis columns followed by translation.
 * Maps local XYZ positions into the coordinate space documented by the consuming API.
 */
export type AffineTransform3D = readonly [
  ...basisX: Vector3,
  ...basisY: Vector3,
  ...basisZ: Vector3,
  ...translation: Vector3
];
