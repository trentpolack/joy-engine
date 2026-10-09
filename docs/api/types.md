# Types API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## Vector2
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface Vector2 {
  x: number;
  y: number;
}
```
Shared numeric contracts. Units and gameplay meaning belong to each caller.



## Vector3
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
type Vector3 = [number, number, number];
```
Ordered X/Y/Z components in the coordinate space documented by the consuming API.



## Velocity2
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface Velocity2 {
  velocityX: number;
  velocityY: number;
}
```


## MovingBody
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
type MovingBody = Vector2 & Velocity2;
```


## Color
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
type Color = readonly [number, number, number, number];
```


## IndexedMesh3D
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface IndexedMesh3D {
  name: string;
  positions: readonly number[];
  normals: readonly number[];
  indices: readonly number[];
}
```


## ShadedTriangle3D
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
type ShadedTriangle3D = readonly [number, number, number, number, number, number, number, number, number, number];
```
A triangle stores three x/y/z positions followed by a face brightness multiplier.



## ShadedMesh3D
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface ShadedMesh3D {
  name: string;
  triangles: readonly ShadedTriangle3D[];
}
```


## ConfigField
Import from `joy-engine`. [Source](../../src/core/config-values.ts)

```ts
interface ConfigField {
  key: string;
  label: string;
  group: string;
  description: string;
  defaultValue: number;
  min: number;
  max: number;
  step: number;
  units: string;
  applyMode: ConfigApplyMode;
}
```


## SoundDefinition
Import from `joy-engine`. [Source](../../src/browser/audio-bank.ts)

```ts
interface SoundDefinition {
  url: string;
  volume?: number;
  maximumVoices?: number;
}
```


## AudioBankOptions
Import from `joy-engine`. [Source](../../src/browser/audio-bank.ts)

```ts
interface AudioBankOptions {
  maximumVoices?: number;
  contextFactory?: () => AudioContext;
}
```


## EnvironmentOptions
Import from `joy-engine`. [Source](../../src/environment/environment-system.ts)

```ts
type EnvironmentOptions = {
  timeOfDay?: number;
  dayLengthSeconds?: number;
  latitudeDegrees?: number;
  axialTiltDegrees?: number;
  dayOfYear?: number;
  northRadians?: number;
  timeScale?: number;
  wind?: WindOptions;
  atmosphere?: AtmosphereOptions;
  clouds?: CloudOptions;
  lightShafts?: LightShaftOptions;
  ambientOcclusion?: AmbientOcclusionOptions;
};
```


## EnvironmentFrame
Import from `joy-engine`. [Source](../../src/environment/environment-system.ts)

```ts
type EnvironmentFrame = {
  elapsedSeconds: number;
  timeOfDay: number;
  sun: AtmosphereLight;
  moon: AtmosphereLight;
  primaryLight: AtmosphereLight;
  skylight: Readonly<{
      color: readonly [
          number,
          number,
          number
      ];
      intensity: number;
  }>;
  wind: Readonly<{
      direction: readonly [
          number,
          number,
          number
      ];
      velocity: readonly [
          number,
          number,
          number
      ];
      speed: number;
  }>;
  atmosphere: Readonly<Required<AtmosphereOptions>>;
  clouds: Readonly<Required<CloudOptions>>;
  lightShafts: Readonly<Required<LightShaftOptions>>;
  ambientOcclusion: Readonly<Required<AmbientOcclusionOptions>>;
};
```


## AtmosphereLight
Import from `joy-engine`. [Source](../../src/environment/environment-system.ts)

```ts
type AtmosphereLight = {
  direction: readonly [
      number,
      number,
      number
  ];
  color: readonly [
      number,
      number,
      number
  ];
  intensity: number;
  angularRadius: number;
  kind: 'sun' | 'moon';
};
```


## ComponentFactory
Import from `joy-engine`. [Source](../../src/gameplay/component-registry.ts)

```ts
type ComponentFactory = (data: JsonRecord, entity: Entity) => EntityComponent;
```


## EntitySystemOptions
Import from `joy-engine`. [Source](../../src/gameplay/entity-system.ts)

```ts
interface EntitySystemOptions {
  requiredComponents?: readonly string[];
}
```


## EntityWorldOptions
Import from `joy-engine`. [Source](../../src/gameplay/entity-world.ts)

```ts
interface EntityWorldOptions {
  registry?: EntityRegistry;
  componentRegistry?: ComponentRegistry;
}
```


## EntityDefinition
Import from `joy-engine`. [Source](../../src/gameplay/types.ts)

```ts
interface EntityDefinition {
  id: string;
  name: string;
  type: string;
  tags: string[];
  transform: EntityTransform;
  components: Record<string, JsonRecord>;
  template?: string;
  overrides?: JsonRecord;
}
```


## EntityOptions
Import from `joy-engine`. [Source](../../src/gameplay/types.ts)

```ts
interface EntityOptions {
  id: string;
  name?: string;
  type?: string;
  tags?: readonly string[];
  transform?: {
      position?: readonly number[];
      rotation?: readonly number[];
      scale?: readonly number[];
  };
  components?: Record<string, JsonRecord>;
  template?: string;
  overrides?: JsonRecord;
}
```


## EntityTransform
Import from `joy-engine`. [Source](../../src/gameplay/types.ts)

```ts
interface EntityTransform {
  position: EntityVector3;
  rotation: EntityVector3;
  scale: EntityVector3;
}
```


## ControlIntent
Import from `joy-engine`. [Source](../../src/gameplay/types.ts)

```ts
type ControlIntent = Record<string, number | boolean>;
```


## JsonValue
Import from `joy-engine`. [Source](../../src/gameplay/types.ts)

```ts
type JsonValue = null | boolean | number | string | JsonValue[] | {
    [key: string]: JsonValue;
};
```


## JsonRecord
Import from `joy-engine`. [Source](../../src/gameplay/types.ts)

```ts
type JsonRecord = Record<string, JsonValue>;
```


## LevelData
Import from `joy-engine`. [Source](../../src/gameplay/level-document.ts)

```ts
interface LevelData {
  version: 1;
  name: string;
  entities: EntityDefinition[];
  postProcessing?: PostProcessingProfile;
}
```


## PhysicsWorldOptions
Import from `joy-engine`. [Source](../../src/physics/physics-world.ts)

```ts
interface PhysicsWorldOptions {
  gravity?: readonly number[];
  fixedStep?: number;
  maxSubSteps?: number;
}
```


## PhysicsBodyOptions
Import from `joy-engine`. [Source](../../src/physics/physics-body.ts)

```ts
interface PhysicsBodyOptions {
  shape: typeof PRIMITIVE_SHAPE.BOX | typeof PRIMITIVE_SHAPE.SPHERE | typeof PRIMITIVE_SHAPE.PLANE;
  mass?: number;
  halfExtents?: readonly number[];
  radius?: number;
  position?: readonly number[];
  rotation?: readonly number[];
  friction?: number;
  restitution?: number;
  linearDamping?: number;
  angularDamping?: number;
}
```


## PhysicsVehicleOptions
Import from `joy-engine`. [Source](../../src/physics/physics-vehicle.ts)

```ts
interface PhysicsVehicleOptions {
  wheels: readonly PhysicsWheelOptions[];
  engineForce?: number;
  maxSteer?: number;
  brakeForce?: number;
  frictionSlip?: number;
}
```


## PhysicsWheelOptions
Import from `joy-engine`. [Source](../../src/physics/physics-vehicle.ts)

```ts
interface PhysicsWheelOptions {
  position: readonly number[];
  radius?: number;
  steering?: boolean;
  driven?: boolean;
  brake?: boolean;
  suspensionRestLength?: number;
  suspensionStiffness?: number;
  dampingCompression?: number;
  dampingRelaxation?: number;
  maxSuspensionTravel?: number;
  maxSuspensionForce?: number;
  frictionSlip?: number;
  rollInfluence?: number;
}
```


## PhysicsVehicleInput
Import from `joy-engine`. [Source](../../src/physics/physics-vehicle.ts)

```ts
interface PhysicsVehicleInput {
  throttle?: number;
  steer?: number;
  brake?: number;
}
```


## PhysicsTransform
Import from `joy-engine`. [Source](../../src/physics/physics-body.ts)

```ts
interface PhysicsTransform {
  position?: readonly number[];
  rotation?: readonly number[];
}
```


## DevToolsOptions
Import from `joy-engine`. [Source](../../src/browser/diagnostics/dev-tools.ts)

```ts
interface DevToolsOptions {
  title?: string;
  enabled?: boolean;
  sampleWindowMilliseconds?: number;
}
```


## RendererDiagnostics
Import from `joy-engine`. [Source](../../src/rendering/gpu/gpu-diagnostics.ts)

```ts
interface RendererDiagnostics {
  /** Device.statsManager is shared by all luma devices in this JavaScript realm. */
  scope: 'all-luma-devices';
  /** Bytes tracked by luma; excludes untracked driver/browser allocations. Null means unavailable. */
  memory: {allocatedBytes: number | null; bufferBytes: number | null; textureBytes: number | null} | null;
  /** Current active and lifetime-created handles, not draw calls. Null means unavailable. */
  resources: Record<string, {active: number | null; created: number | null}> | null;
  /** Existing scene culling counts for this renderer's most recent rendered frame. */
  scene?: {visible: number; culled: number};
}
```


## RendererDiagnosticOptions
Import from `joy-engine`. [Source](../../src/rendering/gpu/gpu-diagnostics.ts)

```ts
interface RendererDiagnosticOptions {
  memory?: boolean;
  resources?: boolean;
}
```


## RendererDiagnosticSource
Import from `joy-engine`. [Source](../../src/rendering/gpu/gpu-diagnostics.ts)

```ts
interface RendererDiagnosticSource {
  /** Copy requested counters on demand; callers own the returned snapshot. */
  getDiagnostics(options?: RendererDiagnosticOptions): RendererDiagnostics | null;
}
```


## SceneEffectsOptions
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-effect-config.ts)

```ts
type SceneEffectsOptions = {
  colorFormat?: typeof GPU_TEXTURE_FORMAT.RGBA16FLOAT | typeof GPU_TEXTURE_FORMAT.RGBA8UNORM;
  depthAwareBlur?: boolean | SceneDepthAwareBlurOptions;
  ssao?: boolean | SceneSsaoOptions;
  gtao?: boolean | SceneGtaoOptions;
  ssgi?: boolean | SceneSsgiOptions;
  outlines?: boolean | SceneOutlineOptions;
  taa?: boolean | SceneTaaOptions;
  motionBlur?: boolean | SceneMotionBlurOptions;
  ssr?: boolean | SceneSsrOptions;
  heightFog?: boolean | SceneHeightFogOptions;
  clusteredLighting?: boolean | SceneClusteredLightingOptions;
  adaptiveExposure?: boolean | SceneAdaptiveExposureOptions;
};
```
Each effect accepts true for defaults, false to disable, or a deliberately narrow tuning record.



## SceneEffectsFrame
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-effects.ts)

```ts
type SceneEffectsFrame = {
  depthTexture?: Texture;
  normalTexture?: Texture;
  velocityTexture?: Texture;
  taaVelocityTexture?: Texture;
  projectionMatrix?: ArrayLike<number>;
  inverseProjectionMatrix?: ArrayLike<number>;
  inverseViewMatrix?: ArrayLike<number>;
  inverseViewProjectionMatrix?: ArrayLike<number>;
  previousViewProjectionMatrix?: ArrayLike<number>;
  inverseRasterViewProjectionMatrix?: ArrayLike<number>;
  previousRasterViewProjectionMatrix?: ArrayLike<number>;
  nearPlane?: number;
  farPlane?: number;
  projection?: 'perspective' | 'orthographic';
  currentJitter?: [number, number];
  previousJitter?: [number, number];
  directionalLightDirectionView?: readonly [number, number, number];
  directionalLightColor?: readonly [number, number, number];
  clusteredLighting?: SceneVolumeLightBindings;
  frameIndex?: number;
  timeSeconds?: number;
  deltaTimeSeconds?: number;
  resetHistory?: boolean;
};
```
All textures, buffers, and column-major camera matrices are borrowed for this frame. Depth and reconstructed matrices use WebGPU [0,1] depth and top-left UVs. normalTexture packs encoded view normals in RGB and perceptual roughness in A. velocityTexture is current minus previous jittered UV; taaVelocityTexture excludes jitter. Projection matrices reconstruct the rasterized depth; camera reprojection matrices are unjittered.



## GpuBackend
Import from `joy-engine`. [Source](../../src/rendering/gpu/gpu-device.ts)

```ts
type GpuBackend = typeof GPU_BACKEND.BEST_AVAILABLE | typeof GPU_BACKEND.WEBGPU | typeof GPU_BACKEND.WEBGL;
```


## PostProcessingProfile
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/profile.ts)

```ts
interface PostProcessingProfile {
  version: 1;
  preset: string;
  overrides: Partial<PostProcessingConfig>;
}
```


## PostProcessingConfig
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/config.ts)

```ts
type PostProcessingConfig = {
  toneMapper: typeof TONE_MAPPER.AGX | typeof TONE_MAPPER.ACES | typeof TONE_MAPPER.NONE;
  antialiasStrength: number;
  exposure: number;
  bloomStrength: number;
  bloomRadius: number;
  bloomThreshold: number;
  bloomKnee: number;
  bloomAnamorphic: number;
  bloomQuality: typeof BLOOM_QUALITY.LOW | typeof BLOOM_QUALITY.MEDIUM | typeof BLOOM_QUALITY.HIGH | typeof BLOOM_QUALITY.ULTRA;
  bloomScatter: number;
  bloomLensFlare: number;
  brightness: number;
  vibrance: number;
  blurRadius: number;
  saturation: number;
  contrast: number;
  filmGrain: number;
  filmGrainSize: number;
  filmGrainSpeed: number;
  filmGrainNoise: typeof FILM_GRAIN_NOISE.ANALOG | typeof FILM_GRAIN_NOISE.DECORRELATED;
  vignetteStrength: number;
  vignetteRadius: number;
  vignetteBlur: number;
  agxLook: typeof AGX_LOOK.NEUTRAL | typeof AGX_LOOK.PUNCHY;
};
```


## ParticleEffectAsset
Import from `joy-engine`. [Source](../../src/particles/asset.ts)

```ts
interface ParticleEffectAsset {
  version: 1;
  name: string;
  maxParticles: number;
  parameters: Record<string, ParticleParameter>;
  emitters: ParticleEmitterAsset[];
}
```


## ParticleEmitterAsset
Import from `joy-engine`. [Source](../../src/particles/asset.ts)

```ts
interface ParticleEmitterAsset {
  id: string;
  enabled: boolean;
  root: boolean;
  duration: number;
  rate: number;
  bursts: {
      time: number;
      count: number;
  }[];
  lifetime: number;
  renderer: typeof PARTICLE_RENDERER.SOFT | typeof PARTICLE_RENDERER.STREAK | typeof PARTICLE_RENDERER.RING | typeof PARTICLE_RENDERER.BILLOW | typeof PARTICLE_RENDERER.CIRCLE | typeof PARTICLE_RENDERER.SQUARE | typeof PARTICLE_RENDERER.TEXTURED | typeof PARTICLE_RENDERER.FLIPBOOK;
  texture?: ParticleTexture;
  light?: ParticleLight;
  spawn: string;
  update: string;
  trail?: ParticleChild & {
      interval: number;
  };
  death?: ParticleChild;
}
```


## ParticleParameter
Import from `joy-engine`. [Source](../../src/particles/asset.ts)

```ts
type ParticleParameter = {
    value: number;
    min: number;
    max: number;
    type?: typeof PARTICLE_PARAMETER_TYPE.SCALAR;
} | {
    type: typeof PARTICLE_PARAMETER_TYPE.VECTOR3;
    value: [
        number,
        number,
        number
    ];
    min: [
        number,
        number,
        number
    ];
    max: [
        number,
        number,
        number
    ];
} | {
    type: typeof PARTICLE_PARAMETER_TYPE.COLOR;
    value: [
        number,
        number,
        number,
        number
    ];
    min: [
        number,
        number,
        number,
        number
    ];
    max: [
        number,
        number,
        number,
        number
    ];
};
```


## CompiledParticleEffect
Import from `joy-engine`. [Source](../../src/particles/asset.ts)

```ts
interface CompiledParticleEffect {
  readonly asset: ParticleEffectAsset;
  readonly emitters: readonly CompiledEmitter[];
}
```


## EffectParticle
Import from `joy-engine`. [Source](../../src/particles/particle-effect.ts)

```ts
type EffectParticle = Record<string, number> & {
    x: number;
    y: number;
    z: number;
    vx: number;
    vy: number;
    vz: number;
    ax: number;
    ay: number;
    az: number;
    drag: number;
    size: number;
    rotation: number;
    spin: number;
    r: number;
    g: number;
    b: number;
    alpha: number;
    lightIntensity: number;
    lightRange: number;
    lifetime: number;
    age: number;
    u0: number;
    u1: number;
    u2: number;
    u3: number;
    emitterIndex: number;
    generation: number;
    index: number;
    originX: number;
    originY: number;
    originZ: number;
    parentVx: number;
    parentVy: number;
    parentVz: number;
    nextTrail: number;
};
```


## ParticleEffectOptions
Import from `joy-engine`. [Source](../../src/particles/particle-effect.ts)

```ts
interface ParticleEffectOptions {
  seed?: number;
  random?: () => number;
  parameters?: Record<string, number | [
      number,
      number,
      number
  ] | [
      number,
      number,
      number,
      number
  ]>;
  position?: {
      x: number;
      y: number;
      z?: number;
  };
  profiling?: boolean;
  profileClock?: () => number;
}
```


## EditorGridView
Import from `joy-engine`. [Source](../../src/rendering/editor-grid.ts)

```ts
interface EditorGridView {
  /** Borrowed Y-up camera target and backward direction, in world coordinates. */
  target: readonly number[];
  backward: readonly number[];
  distance: number;
  viewportHeightPixels: number;
  /** Height of the editor's XZ reference plane, in the caller's unchanged world units. */
  height?: number;
}
```


## EditorGridFrame
Import from `joy-engine`. [Source](../../src/rendering/editor-grid.ts)

```ts
interface EditorGridFrame {
  /** Borrowed cached XYZ/RGBA triangles. Submit as unlit, depth-tested transparency; never mutate. */
  triangles: Float32Array;
  /** Minor spacing in world units; major spacing is five times this value. */
  spacing: number;
}
```


## ParticleCameraBasis
Import from `joy-engine`. [Source](../../src/particles/rendering.ts)

```ts
interface ParticleCameraBasis {
  right: readonly number[];
  up: readonly number[];
  backward: readonly number[];
}
```


## ParticlePointLight
Import from `joy-engine`. [Source](../../src/particles/lights.ts)

```ts
interface ParticlePointLight {
  x: number;
  y: number;
  z: number;
  r: number;
  g: number;
  b: number;
  range: number;
  emitterIndex: number;
  particleIndex: number;
}
```


## ParticleProfile
Import from `joy-engine`. [Source](../../src/particles/profile.ts)

```ts
type ParticleProfile = ReturnType<typeof createParticleProfile>;
```


## ParticleRenderMetrics
Import from `joy-engine`. [Source](../../src/particles/rendering.ts)

```ts
type ParticleRenderMetrics = ReturnType<typeof createParticleRenderMetrics>;
```


## ParticleLight
Import from `joy-engine`. [Source](../../src/particles/asset.ts)

```ts
interface ParticleLight {
  intensity: number;
  range: number;
}
```


## PbrMaterial
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface PbrMaterial {
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
```
Metallic/roughness material. Factors are linear, colors have RGB or RGBA order. Texture URLs are borrowed sources; the renderer owns decoded GPU textures. Color maps use sRGB; normal and scalar maps use linear data (scalar red channel).



## PbrMesh
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface PbrMesh {
  positions: readonly number[];
  normals: readonly number[];
  uvs: readonly number[];
  colors: readonly number[];
  alphas: readonly number[];
  triangles: readonly number[];
  triangleMaterials: readonly number[];
  materials: readonly PbrMaterial[];
}
```
Indexed, world-space Y-up geometry. Zero normals request a flat face normal.



## PbrFrame
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface PbrFrame {
  viewProjection: Float32Array | number[];
  eye: readonly number[];
  backward: readonly number[];
  lightDirection?: readonly number[];
  lightColor?: readonly number[];
  ambientColor?: readonly number[];
}
```
World-space camera and lighting for a material pass. RGB light values are linear radiance.



## PointLight
Import from `joy-engine`. [Source](../../src/rendering/pbr/point-lights.ts)

```ts
interface PointLight {
  x: number;
  y: number;
  z: number;
  r: number;
  g: number;
  b: number;
  range: number;
}
```


## MeshAssetData
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface MeshAssetData {
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
```
Local Y-up indexed mesh input. Optional attributes are empty or vertex-aligned.



## MeshInstanceOptions
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface MeshInstanceOptions {
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
```
Mutable instance settings. Euler angles are radians, applied X then Y then Z.



## SceneRendererOptions
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-renderer.ts)

```ts
interface SceneRendererOptions {
  camera?: OrbitCamera;
  atmosphere?: boolean;
  pixelRatio?: number;
  postProcessing?: Partial<PostProcessingConfig> | false;
  device?: GpuDeviceOptions;
  /** Individual WebGPU effect opt-ins. Opaque scene effects precede transparency and HDR display processing. */
  sceneEffects?: SceneEffectsOptions;
}
```


## SceneFrameOptions
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-renderer.ts)

```ts
interface SceneFrameOptions {
  environment?: EnvironmentFrame;
  lights?: readonly PointLight[];
  ambient?: number;
  clearColor?: Color;
  timeSeconds?: number;
  opaqueTriangles?: readonly number[];
  transparentTriangles?: readonly number[];
  culling?: boolean;
  /** Seconds since the preceding rendered frame, for exposure adaptation. */
  deltaTimeSeconds?: number;
  /** Explicit camera cut, teleport, procedural topology change or other discontinuity. */
  resetHistory?: boolean;
}
```


## InputAction
Import from `joy-engine`. [Source](../../src/browser/input/input-manager.ts)

```ts
interface InputAction {
  bindings: readonly InputBinding[];
  contexts?: readonly string[];
}
```


## InputAxis
Import from `joy-engine`. [Source](../../src/browser/input/input-manager.ts)

```ts
interface InputAxis {
  positive?: string;
  negative?: string;
  gamepadAxis?: number;
  deadZone?: number;
  contexts?: readonly string[];
}
```


## InputBinding
Import from `joy-engine`. [Source](../../src/browser/input/input-manager.ts)

```ts
type InputBinding = {
    key: string;
} | {
    pointer: number;
} | {
    button: number;
} | {
    virtual: string;
};
```


## InputSnapshot
Import from `joy-engine`. [Source](../../src/browser/input/input-manager.ts)

```ts
interface InputSnapshot {
  actions: Record<string, InputActionState>;
  axes: Record<string, number>;
}
```


## TerrainAsset
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-asset.ts)

```ts
interface TerrainAsset {
  manifest: TerrainManifest;
  heights: Uint16Array;
  patches: TerrainPatch[];
  tilesPerAxis: number;
  maxLod: number;
}
```


## TerrainManifest
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-asset.ts)

```ts
interface TerrainManifest {
  version: number;
  size: number;
  tileCells: number;
  extent: number[];
  heightOffset: number;
  heightScale: number;
  heightFile: string;
  orientation: string;
  patches: TerrainPatch[];
}
```


## TerrainView
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-selection.ts)

```ts
interface TerrainView {
  eye: number[];
  projectionScale: number;
  tolerance: number;
  target: number[];
  targetRadius: number;
}
```


## TerrainSelection
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-selection.ts)

```ts
interface TerrainSelection {
  detail: number;
  lod: number;
  edges: number[];
}
```


## ProjectCapability
Import from `joy-engine/project-manifest`. [Source](../../src/project/project-manifest.ts)

```ts
interface ProjectCapability {
  id: string;
  label: string;
  extensions: string[];
  assetRoot: string;
  renderer: string;
  features: Record<string, boolean>;
}
```


## ProjectManifest
Import from `joy-engine/project-manifest`. [Source](../../src/project/project-manifest.ts)

```ts
interface ProjectManifest {
  $schema?: string;
  schemaVersion: 1;
  name: string;
  preview: {
      root: string;
      entry: string;
  };
  capabilities: ProjectCapability[];
}
```
