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



## AffineTransform3D
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
type AffineTransform3D = readonly [
  ...basisX: Vector3,
  ...basisY: Vector3,
  ...basisZ: Vector3,
  ...translation: Vector3
];
```
Column-major affine transform: three scaled basis columns followed by translation. Maps local XYZ positions into the coordinate space documented by the consuming API.



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
  /** Defaults to true for the environment primary direct light. */
  castsShadow?: boolean;
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


## WorldOptions
Import from `joy-engine`. [Source](../../src/gameplay/world.ts)

```ts
interface WorldOptions {
  entities?: EntityWorldOptions;
  physics?: PhysicsWorldOptions;
  /** Borrowed solver; when present, physics options are invalid and the caller retains ownership. */
  physicsWorld?: PhysicsWorld;
}
```


## WorldStep
Import from `joy-engine`. [Source](../../src/gameplay/world.ts)

```ts
interface WorldStep {
  readonly elapsedSeconds: number;
  readonly physicsSteps: number;
}
```


## GridRoute
Import from `joy-engine`. [Source](../../src/gameplay/grid-navigation.ts)

```ts
interface GridRoute<Cell> {
  /** Accumulated step cost from the search origin, in caller-defined cost units. */
  cost: number;
  /** Cell this route entered from; null only for the origin. */
  previous: Cell | null;
}
```
Cheapest known route into one cell.



## GridSearchOptions
Import from `joy-engine`. [Source](../../src/gameplay/grid-navigation.ts)

```ts
interface GridSearchOptions<Cell> {
  /** Search origin. Always reachable at zero cost. */
  start: Cell;
  /** Inclusive maximum accumulated cost. Use Infinity to flood the whole connected region. */
  budget: number;
  /** Adjacent candidates. Iteration order is the deterministic tie-break between equal-cost routes. */
  neighbors: (cell: Cell) => Iterable<Cell>;
  /** Finite nonnegative cost to step from `from` into `to`, or null when the step is not allowed. */
  stepCost: (from: Cell, to: Cell) => number | null;
  /** Maximum settled cells before the search throws; guards unbounded implicit graphs. */
  cellLimit?: number;
}
```
Borrowed rules for one search. Callbacks must not mutate the grid during the search.



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


## DestructibleChunkDefinition
Import from `joy-engine`. [Source](../../src/physics/destruction/chunk-structure.ts)

```ts
interface DestructibleChunkDefinition {
  id: string;
  center: Vector3;
  size: Vector3;
  material: string;
  hitPoints: number;
}
```


## RadialDamage
Import from `joy-engine`. [Source](../../src/physics/destruction/chunk-structure.ts)

```ts
interface RadialDamage {
  center: readonly number[];
  radius: number;
  damage: number;
}
```


## ChunkChangeSet
Import from `joy-engine`. [Source](../../src/physics/destruction/chunk-structure.ts)

```ts
interface ChunkChangeSet {
  revision: number;
  /** All chunks leaving static geometry and queries at this boundary, including support detachments. */
  removed: readonly DestructibleChunkDefinition[];
  /** Subset of removed chunks released by support loss, retaining their remaining health. */
  detached: readonly DestructibleChunkDefinition[];
  activeChunkCount: number;
}
```


## ChunkDamageState
Import from `joy-engine`. [Source](../../src/physics/destruction/chunk-structure.ts)

```ts
interface ChunkDamageState {
  chunkId: string;
  remainingHitPoints: number;
  maximumHitPoints: number;
  active: boolean;
}
```
Copied committed damage state; hit points use the same units as queued damage.



## ChunkSupportOptions
Import from `joy-engine`. [Source](../../src/physics/destruction/chunk-support.ts)

```ts
interface ChunkSupportOptions {
  groundHeight: number;
  maximumCantileverDistance: number;
  contactTolerance?: number;
}
```
Optional Y-up masonry support approximation, in the same world units as chunk geometry. Face contacts carry support upward and sideways; horizontal offsets spend the cantilever allowance. This is a geometric support rule, not a stress, torque, or material-load solver.



## ChunkSupportState
Import from `joy-engine`. [Source](../../src/physics/destruction/chunk-support.ts)

```ts
interface ChunkSupportState {
  chunkId: string;
  anchored: boolean;
  supported: boolean;
  lateralDistance: number | null;
}
```
Copied static support diagnostics. Null distance means no permitted route to a ground anchor. Released rigid bodies are outside this graph, even after they land on the ground.



## PhysicsBackend
Import from `joy-engine`. [Source](../../src/physics/backend-contract.ts)

```ts
interface PhysicsBackend {
  readonly residency: PhysicsResidency;
  readonly activeBodyCount: number;
  applyMutations(batch: PhysicsMutationBatch): void | Promise<void>;
  step(elapsedSeconds:number): void | Promise<void>;
  query(origin:readonly number[], direction:readonly number[], maximumDistance:number):
    null | {shapeId:string; distance:number; point:Vector3};
  destroy():void;
}
```
Solver-neutral synchronization boundary for resident bodies, transforms, velocities, and shapes. Gameplay-visible mutation batches complete before query(), avoiding asynchronous readback on aim and character collision paths. GPU backends may retain simulation state on-device and publish only compact event/readback records; CPU implementations remain the required fallback.



## PhysicsResidency
Import from `joy-engine`. [Source](../../src/physics/backend-contract.ts)

```ts
type PhysicsResidency = 'cpu' | 'gpu';
```


## PhysicsCollisionShape
Import from `joy-engine`. [Source](../../src/physics/backend-contract.ts)

```ts
type PhysicsCollisionShape = Readonly<{id:string; center:Vector3; halfExtents:Vector3}>;
```


## PhysicsMutationBatch
Import from `joy-engine`. [Source](../../src/physics/backend-contract.ts)

```ts
type PhysicsMutationBatch = Readonly<{
  sequence:number;
  removedShapeIds:readonly string[];
  addedShapes:readonly PhysicsCollisionShape[];
  invalidatesContacts:boolean;
  rebuildsAccelerationStructure:boolean;
}>;
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
  /** Per-renderer recorded shadow work; all geometry adapters share each recorded light view. */
  shadows?: {available: boolean; castingPointLights: number; castingDirectionalLights: number; batches: number; views: number};
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
  currentJitter?: readonly [number, number];
  previousJitter?: readonly [number, number];
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
interface ParticlePointLight extends PointLight {
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
  /** Borrowed prepared environment; destroy it separately from the renderer. */
  environment?: PbrEnvironment;
  pointLights?: readonly PointLight[];
  /** Directional casting defaults to true when a WebGPU shadow camera is supplied. */
  castsShadow?: boolean;
  shadowCamera?: ShadowCameraFrame;
  shadows?: ShadowOptions | false;
}
```
World-space camera and lighting for a material pass. RGB light values are linear radiance.



## SceneGeometrySnapshot
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-geometry.ts)

```ts
interface SceneGeometrySnapshot {
  readonly kind: 'mesh' | 'terrain';
  readonly identity: SceneNode;
  readonly node: SceneNode;
  readonly localBounds: GeometryBounds;
  readonly worldBounds: GeometryBounds;
  readonly worldTransform: AffineTransform;
  readonly worldRevision: number;
  readonly coverage: 'opaque' | 'transparent';
  readonly castsShadow: boolean;
  readonly receivesShadow: boolean;
}
```
Representation-independent world snapshot consumed by color, depth, and shadow adapters.



## PreparedMeshGeometry
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-geometry.ts)

```ts
interface PreparedMeshGeometry extends SceneGeometrySnapshot {
  readonly kind: typeof SCENE_GEOMETRY_KIND.MESH;
  readonly instance: MeshInstance;
  readonly asset: MeshAsset;
}
```
Mesh adapter data borrows authored asset channels without copying them.



## PreparedTerrainGeometry
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-geometry.ts)

```ts
interface PreparedTerrainGeometry extends SceneGeometrySnapshot {
  readonly kind: typeof SCENE_GEOMETRY_KIND.TERRAIN;
  readonly instance: TerrainInstance;
  readonly asset: TerrainAsset;
}
```
Resident terrain snapshot retains indexed authored geometry outside the CPU triangle queues.



## PreparedTerrainPacket
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-geometry.ts)

```ts
interface PreparedTerrainPacket extends PreparedTerrainGeometry {
  readonly visible: boolean;
}
```


## PreparedSceneGeometry
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-geometry.ts)

```ts
type PreparedSceneGeometry = PreparedMeshGeometry | PreparedTerrainGeometry;
```


## PointLight
Import from `joy-engine`. [Source](../../src/rendering/lighting/point-lights.ts)

```ts
interface PointLight {
  x: number;
  y: number;
  z: number;
  r: number;
  g: number;
  b: number;
  range: number;
  /** Geometric shadows are enabled when omitted; false retains illumination only. */
  castsShadow?: boolean;
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



## AffineTransform
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
type AffineTransform = readonly [number, number, number, number, number, number, number, number, number, number, number, number];
```
Column-major affine basis columns followed by XYZ translation; supports shear.



## GeometryBounds
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface GeometryBounds {
  readonly min: readonly number[];
  readonly max: readonly number[];
}
```
Conservative axis-aligned bounds in the coordinate space of the consuming API.



## SceneNodeOptions
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface SceneNodeOptions {
  parent?: SceneNode | null;
  position?: readonly number[];
  rotation?: readonly number[];
  scale?: readonly number[];
  visible?: boolean;
}
```
Local transform settings. Euler radians apply X, then Y, then Z. Parenting preserves local values.



## MeshInstanceOptions
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface MeshInstanceOptions extends SceneNodeOptions {
  tint?: readonly number[];
  opacity?: number;
  /** Solid geometry casts and receives shadows by default. Transparent geometry never casts opaque shadows. */
  castsShadow?: boolean;
  receivesShadow?: boolean;
  pass?: 'auto' | 'opaque' | 'transparent';
  /** Perceptual surface roughness for screen-space effects, zero polished to one matte. */
  sceneRoughness?: number;
}
```
Mesh presentation settings; the scene borrows the asset and owns the node.



## TerrainInstanceOptions
Import from `joy-engine`. [Source](../../src/core/types.ts)

```ts
interface TerrainInstanceOptions extends SceneNodeOptions {
  castsShadow?: boolean;
  receivesShadow?: boolean;
  tolerance?: number;
  targetRadius?: number;
  /** Zero is normal shading; positive values select the terrain adapter diagnostic. */
  diagnostic?: number;
}
```
Resident terrain settings. Tolerance is CSS pixels; targetRadius is world units.



## SceneRendererOptions
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-renderer.ts)

```ts
interface SceneRendererOptions {
  /**
   * WebGPU geometric shadows default to enabled. WebGL 2 retains illumination without shadows.
   */
  shadows?: ShadowOptions | false;
  camera?: OrbitCamera;
  atmosphere?: boolean;
  pixelRatio?: number;
  postProcessing?: Partial<PostProcessingConfig> | false;
  device?: GpuDeviceOptions;
  /**
   * Individual WebGPU effect opt-ins. Opaque scene effects precede transparency and HDR display processing.
   */
  sceneEffects?: SceneEffectsOptions;
}
```
Construction settings; the renderer borrows its canvas and camera and owns GPU resources.



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
  /**
   * Seconds since the preceding rendered frame, for exposure adaptation.
   */
  deltaTimeSeconds?: number;
  /**
   * Explicit camera cut, teleport, procedural topology change or other discontinuity.
   */
  resetHistory?: boolean;
}
```
Borrowed scene inputs for one submission; time is in seconds and geometry is world-space.



## FrameView
Import from `joy-engine`. [Source](../../src/rendering/scene/frame-view.ts)

```ts
interface FrameView {
  readonly camera: FrameCamera;
  readonly viewport: FrameViewport;
  readonly lighting: {
    readonly lights: readonly Readonly<PointLight>[];
    readonly ambient: number;
    readonly directional: Immutable<AtmosphereLight> | null;
  };
  readonly environment: Immutable<EnvironmentFrame> | null;
  readonly shadows: Readonly<ShadowOptions> | false;
}
```
One owned, deeply immutable CPU snapshot per scene submission. Matrices, light records, environment, and shadow settings are copied before any feature runs. World space is Y-up, distances are world units, and lighting is linear RGB. No render target or pass override belongs here; features retain this same snapshot for the duration of the submission.



## FrameViewOptions
Import from `joy-engine`. [Source](../../src/rendering/scene/frame-view.ts)

```ts
interface FrameViewOptions {
  camera: ShadowCameraFrame & {
    eye: readonly number[];
    backward: readonly number[];
  };
  viewport: {
    cssWidth: number;
    cssHeight: number;
    pixelRatio: number;
  };
  jitter?: readonly [number, number];
  lights?: readonly PointLight[];
  ambient?: number;
  environment?: EnvironmentFrame;
  /** Explicit direct light for low-level scenes without an atmosphere. Omission uses the environment. */
  directional?: AtmosphereLight | null;
  shadows?: ShadowOptions | false;
}
```
Borrowed CPU inputs copied during view creation; subsequent caller edits cannot alter the view.



## FrameCamera
Import from `joy-engine`. [Source](../../src/rendering/scene/frame-view.ts)

```ts
interface FrameCamera {
  readonly viewMatrix: readonly number[];
  readonly projectionMatrix: readonly number[];
  readonly viewProjectionMatrix: readonly number[];
  readonly rasterProjectionMatrix: readonly number[];
  readonly rasterViewProjectionMatrix: readonly number[];
  readonly inverseViewMatrix: readonly number[] | null;
  readonly inverseProjectionMatrix: readonly number[] | null;
  readonly inverseViewProjectionMatrix: readonly number[] | null;
  readonly inverseRasterViewProjectionMatrix: readonly number[] | null;
  readonly nearPlane: number;
  readonly farPlane: number;
  readonly eye: Readonly<Vector3>;
  readonly backward: Readonly<Vector3>;
  /** Top-left UV units. The inverse projection reconstructs jittered raster depth. */
  readonly jitter: readonly [number, number];
}
```
Column-major camera snapshots. Unjittered matrices drive culling and shadow fitting.



## FrameViewport
Import from `joy-engine`. [Source](../../src/rendering/scene/frame-view.ts)

```ts
interface FrameViewport {
  readonly cssWidth: number;
  readonly cssHeight: number;
  readonly pixelRatio: number;
  readonly width: number;
  readonly height: number;
}
```
CSS dimensions preserve the view; width and height are physical backing pixels.



## RenderPassContext
Import from `joy-engine`. [Source](../../src/rendering/scene/render-pass-context.ts)

```ts
interface RenderPassContext {
  readonly camera: PassCamera | null;
  readonly target: Framebuffer;
  readonly viewport: Readonly<{
    width: number;
    height: number;
  }>;
  readonly renderPass: RenderPass;
}
```
Borrowed GPU handles and camera overrides for one recorded pass. This context owns no resources; the caller ends the pass, and the target owner destroys its attachments. Shadow cameras and intermediate targets never replace any part of the shared FrameView. A null camera supports low-level callers that supply only custom packed shader uniforms.



## PassCamera
Import from `joy-engine`. [Source](../../src/rendering/scene/render-pass-context.ts)

```ts
interface PassCamera {
  readonly viewMatrix: readonly number[];
  readonly projectionMatrix: readonly number[];
  readonly viewProjectionMatrix: readonly number[];
  readonly nearPlane: number;
  readonly farPlane: number;
}
```
Pass-local column-major transforms and clip distances in world units.



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


## ShadowOptions
Import from `joy-engine`. [Source](../../src/rendering/shadows/types.ts)

```ts
interface ShadowOptions {
  /**
   * Shadow sampling quality preset; omitted uses the named default.
   */
  quality?: 'low' | 'balanced' | 'cinematic';
  /**
   * Directional depth-map width and height in pixels; omitted uses the backend quality preset.
   */
  directionalMapSize?: number;
  /**
   * Point-light cube-face width and height in pixels; omitted uses the backend quality preset.
   */
  pointMapSize?: number;
  /**
   * Receiver displacement along its world-space normal, in world units.
   */
  normalBias?: number;
  /**
   * Constant caster raster bias in backend depth units.
   */
  depthBias?: number;
  /**
   * Caster depth bias scaled by the depth slope; dimensionless.
   */
  depthBiasSlopeScale?: number;
  /**
   * Maximum directional shadow distance from the camera, in world units.
   */
  shadowDistance?: number;
}
```
ShadowOptions Interface Definition. Caller-owned configuration copied by shadow resource owners. Biases and distance must be finite and nonnegative when configuring SceneShadows.



## ShadowCameraFrame
Import from `joy-engine`. [Source](../../src/rendering/shadows/types.ts)

```ts
interface ShadowCameraFrame {
  viewMatrix: ArrayLike<number>;
  projectionMatrix: ArrayLike<number>;
  nearPlane: number;
  farPlane: number;
}
```
ShadowCameraFrame Interface Definition. Borrowed column-major world-to-view and WebGPU projection matrices, unchanged by preparation. Each matrix contains 16 values; clip distances are in world units and remain valid for the call.



## SceneLightFrame
Import from `joy-engine`. [Source](../../src/rendering/shadows/types.ts)

```ts
interface SceneLightFrame {
  readonly view: FrameView;
  /**
   * Complete opaque world-space XYZ/RGBA caster triangles, including objects outside the camera frustum.
   * Input remains caller-owned and must be valid until preparation returns.
   */
  casters: number[] | Float32Array;
  /** Camera-visible opaque runs; omitted means every colored receiver accepts shadows. */
  receivesShadowRanges?: readonly {firstVertex: number; vertexCount: number; receivesShadow: boolean}[];
}
```
SceneLightFrame Interface Definition. Shared immutable view and borrowed caster geometry for one default lit-triangle preparation. Preparation copies geometry into owned GPU storage and leaves this record and its inputs unchanged.



## PbrEnvironmentOptions
Import from `joy-engine`. [Source](../../src/rendering/environment/pbr-environment.ts)

```ts
type PbrEnvironmentOptions = Omit<PreparePBREnvironmentOptions, 'source' | 'intensity' | 'rotation'>;
```


## PbrEnvironmentPixels
Import from `joy-engine`. [Source](../../src/rendering/environment/pbr-environment.ts)

```ts
interface PbrEnvironmentPixels {
  width: number;
  height: number;
  data: Uint8Array | Float32Array;
}
```
Original equirectangular pixels: byte RGB is linear UNORM; float RGB is linear HDR.



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
