# Rendering API
[API index](README.md) · [Engine guide](../README.md)

Generated from public exports, TypeScript declarations and source documentation with `npm run docs:api`. Implementations remain the source of truth; private methods are omitted.

## createEnvironmentLighting
Import from `joy-engine`. [Source](../../src/rendering/environment/environment-uniforms.ts)

```ts
function createEnvironmentLighting(frame: EnvironmentFrame)
```
Adapt the environment's primary atmosphere light and skylight to a PBR frame. The returned record borrows the frame's immutable arrays and can be spread into a PbrFrame without allocating GPU resources.

- **param** frame


## createEnvironmentUniforms
Import from `joy-engine`. [Source](../../src/rendering/environment/environment-uniforms.ts)

```ts
function createEnvironmentUniforms(frame: EnvironmentFrame): Float32Array
```
Pack an environment snapshot into a portable 16-byte-aligned GPU uniform block. Directions and wind are Y-up world-space values; distance fields use world units; RGB values are linear. The returned array is caller-owned and safe to upload.

- **param** frame Borrowed immutable snapshot.


## ENVIRONMENT_UNIFORM_BYTES
Import from `joy-engine`. [Source](../../src/rendering/environment/environment-uniforms.ts)

```ts
const ENVIRONMENT_UNIFORM_BYTES
```


## ATMOSPHERE_RENDER_UNIFORM_BYTES
Import from `joy-engine`. [Source](../../src/rendering/environment/atmosphere-rendering.ts)

```ts
const ATMOSPHERE_RENDER_UNIFORM_BYTES
```


## ATMOSPHERE_SHADERS
Import from `joy-engine`. [Source](../../src/rendering/environment/atmosphere-rendering.ts)

```ts
const ATMOSPHERE_SHADERS
```
Portable procedural sky program for the engine environment frame. The pass renders a full-screen triangle without a vertex buffer. It combines Rayleigh/Mie atmosphere approximations, sun/moon discs, wind-advected cloud coverage, and a forward-scattering halo that provides the first light-shaft contribution. Scene-depth fog, occlusion-aware shafts, cloud shadows, and AO remain separate scene passes because they need renderer-owned textures.



## createAtmosphereRenderUniforms
Import from `joy-engine`. [Source](../../src/rendering/environment/atmosphere-rendering.ts)

```ts
function createAtmosphereRenderUniforms(inverseViewProjection: ArrayLike<number>, cameraPosition: readonly [
    number,
    number,
    number
], viewportSize: readonly [
    number,
    number
], frame: EnvironmentFrame): Float32Array<ArrayBuffer>
```
Pack camera and environment state for ATMOSPHERE_SHADERS. The inverse view-projection matrix is column-major. Camera position and cloud distances are Y-up world units; viewport dimensions are physical pixels. The returned array is caller-owned and contains no GPU resources.

- **param** inverseViewProjection
- **param** cameraPosition
- **param** viewportSize
- **param** frame


## addCircle
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addCircle(vertices: number[], x: number, y: number, radius: number, color: Color, segments: number = 12, z: number = 0)
```
Append a triangle fan approximating a filled circle. Mutates the supplied vertex array; coordinates use the caller's logical units.

- **param** vertices
- **param** x
- **param** y
- **param** radius
- **param** color
- **param** segments
- **param** z


## addMesh
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addMesh(vertices: number[], mesh: ShadedMesh3D, x: number, y: number, scale: number, rotation: number, color: Color, z: number = 0)
```
Append one normalized, faceted 3D model to the shared GPU batch.

- **param** vertices
- **param** mesh
- **param** x
- **param** y
- **param** scale
- **param** rotation
- **param** color
- **param** z


## addQuad
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addQuad(vertices: number[], x: number, y: number, radius: number, color: Color, rotation: number = Math.PI / 4, z: number = 0)
```
Append a square as two triangles. Radius measures to a corner, not an edge. Mutates the supplied vertex array; coordinates use the caller's logical units.

- **param** vertices
- **param** x
- **param** y
- **param** radius
- **param** color
- **param** rotation
- **param** z


## addTriangle
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addTriangle(vertices: number[], x: number, y: number, radius: number, color: Color, rotation: number = 0, z: number = 0)
```
Append an equilateral triangle. Radius measures from its center to each corner. Mutates the supplied vertex array; coordinates use the caller's logical units.

- **param** vertices
- **param** x
- **param** y
- **param** radius
- **param** color
- **param** rotation
- **param** z


## addPolygon
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addPolygon(vertices: number[], x: number, y: number, radius: number, color: Color, sides: number, rotation: number, z: number)
```
Append a regular polygon using equal radial samples.

- **param** vertices
- **param** x
- **param** y
- **param** radius
- **param** color
- **param** sides
- **param** rotation
- **param** z


## addRadialPolygon
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addRadialPolygon(vertices: number[], x: number, y: number, radius: number, color: Color, shape: readonly number[], rotation: number, z: number)
```
Append a triangle fan from evenly spaced radial multipliers. A sample of 1 lies at radius; rotation is in radians. Each vertex repeats x/y/z/RGBA because the engine consumes a non-indexed triangle list.

- **param** vertices
- **param** x
- **param** y
- **param** radius
- **param** color
- **param** shape
- **param** rotation
- **param** z


## addGradientRadialPolygon
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addGradientRadialPolygon(
  vertices: number[], x: number, y: number, radius: number, centerColor: Color, edgeColor: Color, shape: readonly number[], rotation: number, z: number,
)
```
Append an irregular triangle fan with independent center and edge colors. Interpolation creates a portable radial falloff without textures or a particle-specific shader, making the primitive useful for light blooms, smoke, and other soft particles on every renderer backend.

- **param** vertices
- **param** x
- **param** y
- **param** radius
- **param** centerColor
- **param** edgeColor
- **param** shape
- **param** rotation
- **param** z


## addRing
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addRing(vertices: number[], x: number, y: number, radius: number, width: number, color: Color, segments: number, z: number)
```
Append an annulus as two triangles per segment. Width is the HALF thickness: the radii are radius - width and radius + width. Clamp the inner radius to zero while the ring is smaller than its thickness.

- **param** vertices
- **param** x
- **param** y
- **param** radius
- **param** width
- **param** color
- **param** segments
- **param** z


## addLine
Import from `joy-engine`. [Source](../../src/rendering/geometry/geometry.ts)

```ts
function addLine(vertices: number[], ax: number, ay: number, bx: number, by: number, width: number, color: Color, z: number)
```
Append a butt-ended thick segment as two triangles. Width is the full thickness in the caller's logical units; z is constant along the segment.

- **param** vertices
- **param** ax
- **param** ay
- **param** bx
- **param** by
- **param** width
- **param** color
- **param** z


## GpuTriangleRenderer
Import from `joy-engine`. [Source](../../src/rendering/renderer.ts)

Depth-tested 3D triangle rendering through luma.gl's portable Device API. Games provide world-space vertices, camera uniforms, and matching shaders. Opaque triangles write depth. Translucent triangles use straight-alpha source-over blending and test against opaque depth without writing it. The caller supplies translucent triangles already sorted back-to-front for its camera. Both batches share one framebuffer and one clear per frame.

### create
```ts
static async create(
    canvas: HTMLCanvasElement,
    options: RendererOptions): Promise<GpuTriangleRenderer>
```
Create a renderer and transfer its device/canvas ownership to the result. Failed construction destroys the newly created roots before rejecting.

- **param** canvas Borrowed DOM canvas dedicated to GPU rendering; never removed here.
- **param** options Shader sources, optional features, and device creation settings.
- **returns** An owned renderer; its caller must destroy it after dependent environments.

### constructor
```ts
constructor(
    device: Device,
    canvasContext: CanvasContext,
    {
      shaderSources,
      texturedShaderSources,
      opaqueShaderSources,
      uniformBufferSize = 16,
      initialVertexBufferSize = 64*1024,
      postProcessing,
      sceneEffects,
      pbr = false,
      atmosphere = false,
      shadows = {}
    }: RendererOptions)
```
Initialize owned geometry and optional scene passes in dependency order. Direct construction transfers device roots on success; callers handle failed roots. Invalid scene-effect settings and GPU allocation failures throw synchronously.

- **param** device Device owned by this renderer after successful construction.
- **param** canvasContext Canvas context owned alongside the device.
- **param** options Borrowed shaders and feature settings; buffer capacities are bytes.

### getDiagnostics
```ts
getDiagnostics(options: RendererDiagnosticOptions = {}): RendererDiagnostics | null
```
Copy requested luma counters on demand. Scope is all luma devices in this realm, not this renderer alone. Performs no GPU readback.

- **param** options Counters to sample; disabled groups are omitted from the snapshot.
- **returns** A new diagnostic snapshot, or null after this renderer is destroyed.

### createFrameView
```ts
createFrameView(options: Omit<FrameViewOptions, 'shadows'>): FrameView
```
Build the shared immutable view using this renderer's fixed shadow configuration. Call once per submission before preparing features; input camera and light records are copied. Shadow quality, map sizes, and biases remain fixed for this renderer's lifetime.

- **param** options Borrowed camera, light, environment, and frame-timing inputs.
- **returns** A new immutable CPU snapshot; environment GPU bindings remain borrowed.

### setMaterialMesh
```ts
async setMaterialMesh(mesh: PbrMesh): Promise<boolean>
```
Upload a world-space material mesh; await readiness before displaying new document data. Requires PBR at creation. Failed upload preserves the previous accepted mesh.

- **param** mesh Borrowed geometry and material data copied into renderer-owned resources.
- **returns** Whether the mesh was installed; false if superseded or already disposed.

### createMaterialEnvironment
```ts
createMaterialEnvironment(
    pixels: PbrEnvironmentPixels,
    options: PbrEnvironmentOptions = {}): PbrEnvironment
```
Prepare original equirectangular pixels through luma. The caller owns the result and must destroy it before this renderer/device; ordinary frames borrow it. Invalid pixels, unsupported preparation, or a destroyed renderer throw.

- **param** pixels Borrowed RGBA equirectangular pixels; float RGB represents linear HDR.
- **param** options Borrowed environment integration settings.
- **returns** An owned prepared environment; failed preparation releases its allocations.

### setPostProcessingConfig
```ts
setPostProcessingConfig(config: Partial<PostProcessingConfig>): void
```
Replace authored settings for the next frame without reallocating GPU resources. Requires postProcessing at creation; omitted fields reset to engine defaults. Invalid settings throw before modifying the last working configuration.

- **param** config Borrowed settings, copied on success.

### setSceneEffectsConfig
```ts
setSceneEffectsConfig(config: SceneEffectsOptions): void
```
Replace the optional scene graph atomically; failed construction retains the previous graph. Requires a live offscreen scene. Successful replacement releases attachments and history.

- **param** config Borrowed effect settings; the graph uses the renderer's color format.

### resetSceneHistory
```ts
resetSceneHistory(): void
```
Invalidate owned temporal history after a camera cut or scene replacement. Has no effect when scene effects are disabled.


### sceneVolumeStats
```ts
get sceneVolumeStats()
```
Read the current auxiliary volume-light statistics without allocating GPU resources.

- **returns** Borrowed statistics owned by the attachment pass, or null before it exists.

### bindVertexBuffer
```ts
bindVertexBuffer(buffer: Buffer = this.vertexBuffer?.buffer): void
```
Bind the interleaved stream using each backend's vertex-array semantics. The allocation remains owned by its source; this method only replaces vertex bindings.

- **param** buffer Borrowed buffer, defaulting to the renderer's current geometry allocation.

### resize
```ts
resize(
    width: number,
    height: number,
    pixelRatio: number): void
```
Resize the backing surface without changing the caller's logical geometry.

- **param** width Logical width in CSS pixels.
- **param** height Logical height in CSS pixels.
- **param** pixelRatio Backing pixels per CSS pixel.

### render
```ts
render(
    vertices: number[] | Float32Array,
    uniformData: Float32Array<ArrayBuffer>,
    clearColor: Color,
    transparentVertices: number[] | Float32Array = [],
    timeSeconds: number = 0,
    transparentDraws: readonly TransparentDraw[] = [],
    materialFrame: PbrFrame | null = null,
    sceneFrame: SceneEffectsFrame = {},
    atmosphereUniforms: Float32Array<ArrayBuffer> | null = null,
    attachmentFrame: SceneAttachmentFrame | null = null,
    lightFrame: SceneLightFrame | null = null,
    nativeGeometry: readonly NativeSceneGeometry[] = []): void
```
Submit opaque geometry, then sorted translucent geometry; clear even if empty. CPU arrays and records remain unchanged. Borrowed textures must outlive submission. Missing optional pass support, invalid uniform size, and GPU failures throw. Submission failure invalidates temporal history before rethrowing.

- **param** vertices Borrowed world-space triangle list of x, y, z, r, g, b, a values.
- **param** uniformData Borrowed scene values matching the configured uniform byte size.
- **param** clearColor Borrowed RGBA frame background.
- **param** transparentVertices Borrowed back-to-front world-space XYZ/RGBA triangles.
- **param** timeSeconds Animation time in seconds used by temporal post effects.
- **param** transparentDraws Additional globally sorted XYZ/RGBA or XYZ/RGBA/UV material runs; textures require texturedShaderSources.
- **param** materialFrame Camera/lighting for the optional PBR mesh. Null hides it.
- **param** sceneFrame Borrowed effect inputs; depth defaults to the owned scene attachment. Supply real velocities and projection jitter; reset history after cuts or scene replacement.
- **param** [atmosphereUniforms] Optional packed background snapshot. Requires atmosphere at creation; null hides it.
- **param** [attachmentFrame] Borrowed geometry and shared FrameView for auxiliary scene attachments.
- **param** lightFrame Shared immutable view and complete shadow casters. Pass cameras/targets remain local. Standalone views must retain this renderer's construction-time shadow configuration.

### destroy
```ts
destroy(): void
```
Release owned passes, buffers, shaders, canvas context, and device in dependency order. Repeated destruction is harmless. Caller-owned environments must be destroyed first.



## calculateBackingPixelRatio
Import from `joy-engine`. [Source](../../src/rendering/display.ts)

```ts
function calculateBackingPixelRatio(width: number, height: number, devicePixelRatio: number, limits: {
    maximumPixelRatio?: number;
    maximumBackingPixels?: number;
} = {})
```
Choose GPU backing density independently from a game's logical viewport. The caller retains CSS-sized world and UI coordinates; only raster quality changes with display density. The pixel budget prevents oversized textures.

- **param** width Logical width in CSS pixels.
- **param** height Logical height in CSS pixels.
- **param** devicePixelRatio Physical pixels per CSS pixel.
- **param** [limits] Maximum Pixel Ratio and Maximum Backing Pixels.
- **returns** The calculated backing pixel ratio.


## createTopDownOrthographicMatrix
Import from `joy-engine`. [Source](../../src/rendering/camera/top-down-camera.ts)

```ts
function createTopDownOrthographicMatrix(width: number, height: number, center: {
    x: number;
    y: number;
}, camera: {
    height: number;
    near: number;
    far: number;
}, backend: string): number[]
```
Build a column-major orthographic view-projection matrix for a fixed camera looking down the negative Z axis. X/Y use caller-owned world units and +Z points toward the camera. Backing-store density is intentionally absent: callers provide the logical viewport so display DPI cannot change framing. Near and far are positive distances measured forward from the camera. The returned projection maps those distances to the native clip-depth range of the selected luma.gl backend, producing equivalent depth-buffer values.

- **param** width Logical viewport width, greater than zero.
- **param** height Logical viewport height, greater than zero.
- **param** center World-space point at viewport center.
- **param** camera Camera distances in world units.
- **param** backend luma.gl device type; `webgpu` selects [0, 1] clip depth.
- **returns** Sixteen values suitable for a column-major mat4 uniform.


## sortTopDownTransparentTriangles
Import from `joy-engine`. [Source](../../src/rendering/camera/top-down-camera.ts)

```ts
function sortTopDownTransparentTriangles(vertices: readonly number[]): number[]
```
Return a back-to-front copy of interleaved triangle vertices for a fixed top-down camera. Each vertex is x/y/z/R/G/B/A, so a triangle occupies 21 values. Smaller world Z is farther from a camera looking down -Z. Centroid ordering is appropriate for parallel effect planes. It is not a general solution for intersecting transparent meshes or rotating cameras. Equal-depth triangles retain authored order through JavaScript's stable sort.

- **param** vertices Complete interleaved triangles.
- **returns** Independently owned, depth-sorted vertex data.


## DynamicGpuBuffer
Import from `joy-engine`. [Source](../../src/rendering/gpu/dynamic-gpu-buffer.ts)

A reusable luma.gl buffer that grows geometrically and keeps its allocation stable between writes. The wrapper owns every buffer it creates.

### constructor
```ts
constructor(device: Device, { usage, initialByteLength = 64 * 1024, id = 'dynamic-buffer', onReplace = () => {} }: DynamicGpuBufferOptions)
```
- **param** device
- **param** options

### createBuffer
```ts
createBuffer(byteLength: number)
```
- **param** byteLength

### write
```ts
write(data: ArrayBuffer | ArrayBufferView)
```
Write data, replacing the GPU allocation only when its capacity is exceeded.

- **param** data

### resize
```ts
resize(requiredByteLength: number)
```
Replace the owned GPU buffer with the next power-of-two capacity.

- **param** requiredByteLength

### destroy
```ts
destroy()
```
Release the current allocation; the borrowed device remains alive.



## createGpuCanvasDevice
Import from `joy-engine`. [Source](../../src/rendering/gpu/gpu-device.ts)

```ts
async function createGpuCanvasDevice(canvas: HTMLCanvasElement | OffscreenCanvas, options: GpuDeviceOptions = {}): Promise<Device>
```
Create the repository's portable luma.gl device and its default canvas context. Backend registration stays explicit so bundlers include only the adapters the engine supports. A WebGL retry handles browsers that expose WebGPU while failing adapter acquisition; callers can disable it when WebGPU is required.

- **param** canvas
- **param** [options]
- **returns** Caller-owned device with a canvas context; destroy after dependent resources.


## createPortableShaders
Import from `joy-engine`. [Source](../../src/rendering/gpu/gpu-device.ts)

```ts
function createPortableShaders(device: Device, sources: PortableShaderSources, id: string = 'portable-shader')
```
Assemble modules and compile the shader language selected by the device. Each compilation owns an isolated assembler; registering modules cannot change shaders in another game or renderer. Callers own returned shaders.

- **param** device
- **param** sources
- **param** [id]


## gpuBackendFromQuery
Import from `joy-engine`. [Source](../../src/rendering/gpu/gpu-device.ts)

```ts
function gpuBackendFromQuery(search: string): GpuBackend
```
Resolve an explicit development backend from a URL query. Unknown or absent values retain luma.gl's WebGPU-first selection and WebGL 2 fallback.

- **param** search


## bloom
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## bloomShaderPassPipeline
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## createBloomShaderPassPipeline
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## brightnessContrast
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## denoise
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## gaussianBlur
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## hueSaturation
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## noise
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## sepia
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## tiltShift
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## toneMapping
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## vibrance
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## vignette
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## fxaa
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## createMotionBlurShaderPassPipeline
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## createOutlineShaderPassPipeline
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## createSSAOShaderPassPipeline
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## createTAAShaderPassPipeline
Re-exported from `@luma.gl/effects` through `joy-engine`. The underlying package owns its API.

## ShaderPassRenderer
Import from `joy-engine`. [Source](../../src/rendering/gpu/shader-pass-renderer.ts)

luma.gl pass execution with linear sampling for the shared color chain. luma.gl 9.4 allocates its swap textures with nearest sampling; FXAA and fractional blur offsets require interpolation. Reapply after resize because upstream replaces these attachments. All other execution stays upstream.

### constructor
```ts
constructor(device: Device, props: ShaderPassRendererProps)
```
- **param** device Borrowed device.
- **param** props Pass descriptions and color format.

### resize
```ts
resize(size?: [
    number,
    number
])
```
- **param** [size] Drawing-buffer dimensions.

### destroy
```ts
destroy()
```
Release pass resources before their borrowed sampler.



## GLSLShaderAssembler
Re-exported from `@luma.gl/shadertools` through `joy-engine`. The underlying package owns its API.

## WGSLShaderAssembler
Re-exported from `@luma.gl/shadertools` through `joy-engine`. The underlying package owns its API.

## SceneEffects
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-effects.ts)

Owns optional upstream scene effects and temporal targets. The device and frame resources remain borrowed. Output remains valid until the next render, resize, reset, or destroy. WebGL preserves the source and reports each requested effect as unavailable.

### constructor
```ts
constructor(device: Device, options: SceneEffectsOptions = {})
```
Create enabled WebGPU stages, releasing completed stages if initialization fails.

- **param** device Borrowed rendering device; this owner never destroys it.
- **param** options Effect configuration copied and normalized for this instance.

### render
```ts
render(sourceTexture: Texture, frame: SceneEffectsFrame = {}): Texture
```
Validate every attachment before resizing or mutating history, then execute scene-linear passes. A failed stage invalidates history for the entire frame and rethrows the original error.

- **param** sourceTexture Borrowed scene color texture matching the configured color format.
- **param** frame Borrowed attachments, camera matrices, and timing in seconds for this submission.
- **returns** Borrowed output owned by this instance, valid until the next render, resize, reset, or destroy. Callers must not destroy it. When disabled, returns the caller's borrowed source unchanged.

### resize
```ts
resize(width: number, height: number): void
```
Resize owned targets and invalidate history when drawing-buffer dimensions change.

- **param** width Positive integer drawing-buffer width in pixels.
- **param** height Positive integer drawing-buffer height in pixels.

### resetHistory
```ts
resetHistory(): void
```
Invalidate owned history for camera cuts, visibility changes, or resumed rendering. Throws after destruction; borrowed frame resources remain alive.


### destroy
```ts
destroy(): void
```
Release owned stages in reverse submission order; repeated destruction is harmless. Borrowed source, attachments, buffers, and device remain alive.


### assertAlive
```ts
#assertAlive(): void
```
Reject operations after owned rendering resources have been destroyed.



## agxToneMapping
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/supplemental-passes.ts)

```ts
const agxToneMapping
```
Portable AgX display transform with neutral and punchy looks.



## animatedGrain
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/supplemental-passes.ts)

```ts
const animatedGrain
```
Animated monochrome film grain built on luma.gl's portable noise hash.



## antialias
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/supplemental-passes.ts)

```ts
const antialias
```
Blends the original scene with luma.gl's portable FXAA result.



## linearExposure
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/supplemental-passes.ts)

```ts
const linearExposure
```
Applies exposure without a tone-mapping curve.



## vignetteBlur
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/supplemental-passes.ts)

```ts
const vignetteBlur
```
Blends an independently blurred scene into the frame near the vignette edge. The renderer borrows `blurredTexture`; its creator retains ownership.



## ShaderPass
Re-exported from `@luma.gl/shadertools` through `joy-engine`. The underlying package owns its API.

## ShaderPassPipeline
Re-exported from `@luma.gl/shadertools` through `joy-engine`. The underlying package owns its API.

## DEFAULT_POST_PROCESSING
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/config.ts)

```ts
const DEFAULT_POST_PROCESSING
```


## POST_PROCESSING_SCHEMA
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/config.ts)

```ts
const POST_PROCESSING_SCHEMA
```
One source for validation, engine defaults, and live controls. Uniform-only controls retain resources; graph controls rebuild the effect chain.



## normalizePostProcessingConfig
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/config.ts)

```ts
function normalizePostProcessingConfig(config: Partial<PostProcessingConfig>): PostProcessingConfig
```
Validate a complete replacement or partial authored config without mutating it. Unknown keys and invalid values throw before the caller changes live state.

- **param** config
- **returns** A new, complete record owned by the caller.


## createPostProcessingUniforms
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/config.ts)

```ts
function createPostProcessingUniforms(config: PostProcessingConfig, width: number, height: number, timeSeconds: number)
```
Build the five vec4 values appended to a game's uniform block.

- **param** config
- **param** width Drawing-buffer width.
- **param** height Drawing-buffer height.
- **param** timeSeconds Animation clock in seconds.


## toneMapperIndex
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/config.ts)

```ts
function toneMapperIndex(toneMapper: PostProcessingConfig[typeof POST_PROCESSING_KEY.TONE_MAPPER])
```
Encode the named public option without exposing shader-specific constants.

- **param** toneMapper


## addPostProcessingShaders
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/config.ts)

```ts
function addPostProcessingShaders(sources: {
    wgsl: string;
    glsl: {
        vertex: string;
        fragment: string;
    };
}, {includeUniforms = true}: {
    includeUniforms?: boolean;
} = {})
```
Complete the engine-owned uniform tail in a scene shader while preserving linear scene color. Display effects are authored exclusively in the final resolve shaders; geometry shaders must never tone-map individual draws. Without an offscreen pass, omit the uniform tail while still completing the fragment output. The returned sources are owned copies of the input template.

- **param** sources
- **param** [options] Match the renderer's uniform allocation.


## POST_PROCESSING_PRESETS
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/profile.ts)

```ts
const POST_PROCESSING_PRESETS
```
Original starting points shared by games; a profile stores only its own overrides.



## validatePostProcessingProfile
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/profile.ts)

```ts
function validatePostProcessingProfile(value: unknown): PostProcessingProfile
```
Validate serialized data and return a detached, canonically ordered profile. Unknown versions are rejected until a deliberate migration exists.

- **param** value


## resolvePostProcessingProfile
Import from `joy-engine`. [Source](../../src/rendering/postprocessor/profile.ts)

```ts
function resolvePostProcessingProfile(value: unknown): PostProcessingConfig
```
Resolve engine defaults, preset, then game overrides into a new settings record.

- **param** value Serialized authored profile; never retained or modified.


## OrbitCamera
Import from `joy-engine`. [Source](../../src/rendering/camera/orbit-camera.ts)

Y-up orbit camera. Distances are world units; angles are radians.

### constructor
```ts
constructor()
```

### frame
```ts
frame(positions: number[])
```
Fit packed XYZ positions in world units; empty input uses a default centered volume. Mutates target, radius and camera distance.

- **param** positions Borrowed flat position array.

### basis
```ts
basis()
```
Return fresh world-space basis vectors and eye position for the current orbit.


### matrix
```ts
matrix(aspect: number, webgpu: boolean)
```
Build a caller-owned column-major perspective view/projection matrix.

- **param** aspect Logical viewport width divided by height.
- **param** webgpu Use zero-to-one clip depth instead of WebGL minus-one-to-one.

### viewMatrix
```ts
viewMatrix(): Float32Array<ArrayBuffer>
```
Caller-owned world-to-view matrix, right handed with camera forward along -Z.


### nearPlane
```ts
get nearPlane(): number
```

### farPlane
```ts
get farPlane(): number
```

### projectionMatrix
```ts
projectionMatrix(aspect: number, webgpu: boolean): Float32Array<ArrayBuffer>
```
Caller-owned view-to-clip matrix using the same clip planes as matrix().



## crossVectors3
Import from `joy-engine`. [Source](../../src/rendering/camera/orbit-camera.ts)

```ts
function crossVectors3(a: number[], b: number[])
```
- **param** a
- **param** b


## sortCameraTransparentTriangles
Import from `joy-engine`. [Source](../../src/rendering/camera/orbit-camera.ts)

```ts
function sortCameraTransparentTriangles(vertices: ArrayLike<number>, backward: readonly number[]): number[]
```
Back-to-front centroid sorting for any camera orientation; leaves input untouched. Intersecting transparent surfaces remain an approximation, not order-independent transparency.

- **param** vertices Interleaved XYZ/RGBA triangles (number arrays or typed arrays).
- **param** backward Unit vector from target toward camera.
- **returns** A new caller-owned array in back-to-front order; equal depths keep input order.


## EditorGrid
Import from `joy-engine`. [Source](../../src/rendering/editor-grid.ts)

Own a small cached editor aid, without DOM, GPU resources or frame instrumentation. Camera-facing ribbons keep lines readable across zoom and orbit. World units are never rescaled: minor spacing follows a 1/2/5 sequence and major lines mark five divisions. The origin axes stay at world zero while the finite grid follows pan. All colors are linear and all triangles are transparent, so grids never become lit surfaces or write depth over authored geometry. Discard this owner on teardown.

### constructor
```ts
constructor()
```

### frame
```ts
frame({target, backward, distance, viewportHeightPixels, height = 0}: EditorGridView): EditorGridFrame
```
Return a borrowed frame; unchanged cameras reuse the same bounded array.



## LIT_TRIANGLE_SHADERS
Import from `joy-engine`. [Source](../../src/rendering/lighting/point-lights.ts)

```ts
const LIT_TRIANGLE_SHADERS
```
Flat, double-sided diffuse lighting for XYZ/RGBA opaque triangles. Normals come from surface derivatives. SceneRenderer adds WebGPU shadows by default. Linear HDR output feeds the renderer's optional scene post processing.



## createPointLightUniforms
Import from `joy-engine`. [Source](../../src/rendering/lighting/point-lights.ts)

```ts
function createPointLightUniforms(matrix: ArrayLike<number>, lights: readonly PointLight[], ambient: number = 0.3, environment: EnvironmentFrame | null = null)
```
Own a camera/tint/light uniform snapshot for one frame. Distances are world units; RGB is linear irradiance. A light smoothly reaches zero at its range. Supply as the scene uniforms with LIT_TRIANGLE_SHADERS as opaqueShaderSources. The first 80 bytes also match the unlit particle and textured sprite shaders.

- **param** matrix Column-major view/projection matrix.
- **param** lights Borrowed records, at most 32.
- **param** [ambient] Nonnegative ambient irradiance, added to optional skylight.
- **param** [environment] Borrowed immutable Y-up atmosphere lighting. Omitted means no directional light or colored skylight.
- **returns** A packed snapshot of the camera and lights for one frame, ready to upload to the GPU.


## POINT_LIGHT_UNIFORM_BYTES
Import from `joy-engine`. [Source](../../src/rendering/lighting/point-lights.ts)

```ts
const POINT_LIGHT_UNIFORM_BYTES
```


## DEFAULT_PBR_LIGHTING
Import from `joy-engine`. [Source](../../src/rendering/materials/pbr/pbr-material-pass.ts)

```ts
const DEFAULT_PBR_LIGHTING
```
Default inspection lighting in world space; consumers may override every field per frame.



## MeshAsset
Import from `joy-engine`. [Source](../../src/rendering/scene/mesh-asset.ts)

Immutable CPU geometry shared by any number of scenes and instances. Owns copies, never GPU resources. Drop references to release an asset.

### constructor
```ts
constructor(data: MeshAssetData)
```
- **param** data Local Y-up geometry.


## TerrainInstance
Import from `joy-engine`. [Source](../../src/rendering/scene/terrain-instance.ts)

Scene-owned resident geometry node. Authored heights and GPU resources remain borrowed.

### constructor
```ts
constructor(asset: TerrainAsset, options: TerrainInstanceOptions = {}, scene: RenderScene | null = null)
```

### set
```ts
override set(options: TerrainInstanceOptions)
```
Update local transforms and pass/LOD policy atomically, preserving borrowed geometry.



## SceneNode
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-node.ts)

Scene-owned transform node. Mutable local arrays support direct edits; world data is read-only.

### constructor
```ts
constructor(options: SceneNodeOptions = {}, scene: RenderScene | null = null)
```

### identity
```ts
get identity(): SceneNode
```
Stable identity shared by geometry and temporal-history adapters.


### parent
```ts
get parent(): SceneNode | null
```

### children
```ts
get children(): ReadonlySet<SceneNode>
```

### removed
```ts
get removed(): boolean
```

### transformRevision
```ts
get transformRevision(): number
```
Current own-transform revision, including direct local array edits.


### worldRevision
```ts
get worldRevision(): number
```

### worldTransform
```ts
get worldTransform(): AffineTransform
```
Current world affine transform; translation is at indices 9–11. Removed nodes are invalid.


### worldVisible
```ts
get worldVisible(): boolean
```

### set
```ts
set(options: SceneNodeOptions)
```
Copy local settings. Failed parent validation leaves all settings and hierarchy unchanged.


### setParent
```ts
setParent(parent: SceneNode | null)
```
Reparent within the owning scene, preserving local transforms; reject cycles atomically.


### updateWorldTransform
```ts
updateWorldTransform()
```
- **internal** Update after the parent during the scene's single hierarchy traversal.

### preparedWorldState
```ts
preparedWorldState()
```
- **internal** Read cached world data after the scene traversal, without walking ancestors again.

### invalidate
```ts
invalidate()
```
- **internal** The scene removes descendants before releasing this node's hierarchy references.


## prepareSceneGeometry
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-geometry.ts)

```ts
function prepareSceneGeometry(scene: RenderScene): PreparedSceneGeometry[]
```
Traverse world transforms once, then collect visible geometry independently of camera culling.



## MeshInstance
Import from `joy-engine`. [Source](../../src/rendering/scene/mesh-instance.ts)

Geometry-bearing node borrowing an immutable MeshAsset. No simulation or GPU state.

### constructor
```ts
constructor(asset: MeshAsset, options: MeshInstanceOptions = {}, scene: RenderScene | null = null)
```

### set
```ts
override set(options: MeshInstanceOptions)
```
Copy supplied local and presentation settings; omitted fields retain their previous values.



## RenderScene
Import from `joy-engine`. [Source](../../src/rendering/scene/render-scene.ts)

Owns a transform hierarchy and presentation instances; all geometry assets remain borrowed.

### nodes
```ts
get nodes(): ReadonlySet<SceneNode>
```

### roots
```ts
get roots(): ReadonlySet<SceneNode>
```

### instances
```ts
get instances(): ReadonlySet<MeshInstance>
```

### terrainInstances
```ts
get terrainInstances(): ReadonlySet<TerrainInstance>
```

### createNode
```ts
createNode(options: SceneNodeOptions = {}, parent: SceneNode | null = options.parent ?? null)
```
Create a transform-only node; the explicit parent overrides options.parent when provided.


### createMesh
```ts
createMesh(asset: MeshAsset, options: MeshInstanceOptions = {})
```
Create scene-owned presentation state borrowing the supplied asset.


### createTerrain
```ts
createTerrain(asset: TerrainAsset, options: TerrainInstanceOptions = {})
```
Create a resident terrain node; the renderer owns its device-specific adapter resources.


### remove
```ts
remove(node: SceneNode)
```
Remove an owned node and its full subtree; repeated or foreign removal is harmless.


### createCollection
```ts
createCollection()
```
Create a scene-owned keyed collection for synchronizing simulation views.


### clear
```ts
clear()
```
Invalidate owned nodes and release references without releasing borrowed assets.


### destroy
```ts
destroy()
```
Idempotent disposal. The scene owns no GPU resources or simulation objects.


### registerNode
```ts
registerNode(node: SceneNode)
```
- **internal** Register a fully constructed geometry node owned by this scene.

### updateRootNode
```ts
updateRootNode(node: SceneNode)
```
- **internal** Maintain roots after a validated parent change.

### updateWorldTransforms
```ts
updateWorldTransforms()
```
- **internal** Update ancestors before descendants in one iterative scene traversal.

### assertAlive
```ts
assertAlive()
```
- **internal**


## SceneRenderer
Import from `joy-engine`. [Source](../../src/rendering/scene/scene-renderer.ts)

Owns GPU frame preparation and disposal; borrows camera, scenes and transient geometry. Standard mode is flat-lit vertex color for opaque meshes and unlit transparency. Authored PBR channels remain on assets; this adapter does not interpret their materials.

### constructor
```ts
constructor(canvas: HTMLCanvasElement, options: SceneRendererOptions = {})
```
Configure a scene renderer without allocating GPU resources.

- **param** canvas Borrowed canvas; its CSS dimensions determine the visible view.
- **param** options Borrowed settings copied into this owner; camera remains borrowed.
- **throws** If requested effects are invalid or the offscreen display path is disabled.

### initialize
```ts
async initialize()
```
Initialize once; destruction during startup releases the late GPU result.

- **returns** Completion of the shared initialization attempt.
- **throws** If this owner was destroyed or GPU initialization fails.

### ready
```ts
get ready()
```
- **returns** Whether an initialized GPU renderer is available and this owner is alive.

### backend
```ts
get backend()
```
- **returns** The active backend, or null before initialization and after disposal.

### colorFormat
```ts
get colorFormat()
```
- **returns** The offscreen color format, or null when no offscreen target is available.

### postProcessingConfig
```ts
get postProcessingConfig(): Readonly<PostProcessingConfig> | null
```
Read-only display settings snapshot for opt-in controls; null on a direct canvas path.


### sceneEffectsCapabilities
```ts
get sceneEffectsCapabilities()
```
Backend requirements and requested effect availability; null before initialization.


### sceneVolumeStats
```ts
get sceneVolumeStats()
```
- **returns** Borrowed clustered-lighting statistics, or null while that pass is inactive.

### setSceneEffectsConfig
```ts
setSceneEffectsConfig(config: SceneEffectsOptions)
```
Replace individual opt-ins atomically; preserves the working graph if validation fails.

- **param** config Borrowed effect settings copied after successful graph replacement.
- **throws** If initialization is incomplete or effect validation/allocation fails.

### resetHistory
```ts
resetHistory()
```
Call after teleports, discontinuous animation or content edits that retain instance identities.


### getDiagnostics
```ts
getDiagnostics(options: RendererDiagnosticOptions = {}): RendererDiagnostics | null
```
Copy requested global luma counters and this scene's last culling counts. No sampling runs unless called; returns null before initialization or after disposal.

- **param** options Selects optional global luma counters for this sample.
- **returns** An owned diagnostic snapshot, or null when the GPU renderer is unavailable.

### render
```ts
render(scene: RenderScene, frame: SceneFrameOptions = {}): boolean
```
Render in seconds. Canvas dimensions are CSS pixels; backing density never changes view. Atmosphere must be enabled at construction. A borrowed environment frame drives both background and directional/skylight irradiance; omitting it hides the sky. Transient triangles are a borrowed escape hatch for bespoke procedural feedback/particles.

- **param** scene Borrowed scene; preparing its queues may refresh instance geometry caches.
- **param** frame Borrowed per-frame geometry, lighting, environment, and timing inputs.
- **returns** Whether the frame was rendered; false if the renderer is not ready or the canvas is hidden.

### screenRay
```ts
screenRay(clientX: number, clientY: number): WorldRay | null
```
Client CSS pixels to a world ray; independent of backing pixel density.

- **param** clientX Horizontal client position in CSS pixels.
- **param** clientY Vertical client position in CSS pixels.
- **returns** World ray or null if the canvas is hidden or has no area.

### pickGround
```ts
pickGround(x: number, y: number, height: number = 0): readonly number[] | null
```
Pick a horizontal plane at world-space height.

- **param** x Horizontal client position in CSS pixels.
- **param** y Vertical client position in CSS pixels.
- **param** height Horizontal plane height in world units.
- **returns** World position or null if the canvas is hidden, has no area, or the ray is parallel to the plane.

### project
```ts
project(position: readonly number[]): Vector2 | null
```
World position to canvas-local CSS pixels, or null behind camera/when hidden.

- **param** position Borrowed XYZ position in world units; never modified.
- **returns** Canvas-local CSS pixels or null if the position is behind the camera or the canvas is hidden.

### setPostProcessingConfig
```ts
setPostProcessingConfig(config: Partial<PostProcessingConfig>)
```
Set postprocessing configuration. Must be called after initialization and before rendering.

- **param** config Borrowed replacement display settings; omitted fields reset to defaults.
- **throws** If initialization is incomplete or settings are invalid.

### destroy
```ts
destroy()
```
Idempotent. Does not destroy borrowed scene instances or camera state.



## projectToViewport
Import from `joy-engine`. [Source](../../src/rendering/camera/projection.ts)

```ts
function projectToViewport(position: readonly number[], matrix: ArrayLike<number>, viewport: ViewportRect)
```
Project a world position to canvas-local CSS pixels. Behind-camera points return null.

- **param** position
- **param** matrix
- **param** viewport


## screenRay
Import from `joy-engine`. [Source](../../src/rendering/camera/projection.ts)

```ts
function screenRay(clientX: number, clientY: number, matrix: ArrayLike<number>, viewport: ViewportRect, webgpu: boolean = false): WorldRay | null
```
Construct a normalized world ray from client CSS coordinates using the actual projection. Origin lies on the near plane; supports perspective and orthographic matrices.

- **param** clientX
- **param** clientY
- **param** matrix
- **param** viewport
- **param** [webgpu] Zero-to-one clip depth when true.


## intersectRayPlane
Import from `joy-engine`. [Source](../../src/rendering/camera/projection.ts)

```ts
function intersectRayPlane(ray: WorldRay | null, normal: readonly number[], offset: number)
```
Intersect a forward ray with dot(normal, point) + offset = 0 in world units.

- **param** ray
- **param** normal
- **param** offset


## createFrameView
Import from `joy-engine`. [Source](../../src/rendering/scene/frame-view.ts)

```ts
function createFrameView(options: FrameViewOptions): FrameView
```
Copy borrowed configuration once, compute shared camera transforms, and freeze all CPU data. Plain frozen arrays deliberately replace typed arrays: a readonly typed-array wrapper still exposes mutating methods and a writable backing buffer. GPU upload buffers remain pass-owned.

- **param** options Borrowed camera, viewport, lighting, environment, and shadow configuration.
- **returns** An owned immutable view; singular camera transforms have null inverse matrices.


## loadTerrain
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-asset.ts)

```ts
async function loadTerrain(url: string | URL): Promise<TerrainAsset>
```
Load a caller-owned resident heightfield. Binary is uint16 little endian, row-major; source row zero maps to negative world Z, columns to positive X. Scales are explicit world units; camera LOD never changes this data.

- **param** url URL of the terrain manifest, resolved against the current page when relative.
- **returns** A caller-owned terrain asset containing copied height samples.
- **throws** When either request fails or the downloaded asset is invalid.


## decodeTerrain
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-asset.ts)

```ts
function decodeTerrain(manifest: TerrainManifest, binary: ArrayBuffer): TerrainAsset
```
Validate before GPU allocation and decode portable little-endian samples. Copies binary input; consumers must treat returned data as immutable.

- **param** manifest Borrowed manifest describing the heightfield and patch hierarchy.
- **param** binary Borrowed little-endian height payload; it is not retained or mutated.
- **returns** A caller-owned terrain asset with copied height samples.
- **throws** When dimensions, metadata, patches, or binary length are invalid.


## sampleTerrain
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-asset.ts)

```ts
function sampleTerrain(asset: TerrainAsset, worldX: number, worldZ: number)
```
Stable collision/gameplay sample, independent of camera and render topology. Clamps to landscape bounds. Uses the same diagonal triangle surface at stride 1.

- **param** asset Borrowed resident terrain data.
- **param** worldX World-space X coordinate.
- **param** worldZ World-space Z coordinate.
- **returns** Interpolated world-space height at the clamped coordinate.


## selectTerrain
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-selection.ts)

```ts
function selectTerrain(asset: TerrainAsset, view: TerrainView): TerrainSelection[]
```
Select fixed quadtree leaves using conservative AABB distance and prebuilt geometric errors. Projection scale is CSS viewport height divided by `2*tan(vertical FOV/2)`. Target distance is measured in horizontal world units. No authored heights are scanned or mutated.

- **param** asset Borrowed resident terrain and its immutable patch hierarchy.
- **param** view Borrowed camera and protected-target state.
- **returns** New bounded descriptors with cells limited to 32 CSS pixels.


## reconcileTerrainEdges
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-selection.ts)

```ts
function reconcileTerrainEdges(asset: TerrainAsset, details: number[]): TerrainSelection[]
```
Reconcile each patch with the coarser fractional lattice of its neighbors. Endpoints remain authored tile corners at every supported level, so four-way corners agree.

- **param** asset Borrowed terrain whose patches use complete row-major ordering.
- **param** details Borrowed fractional LOD values in patch order.
- **returns** New per-patch descriptors with edges ordered -X, +X, -Z, +Z.


## sampleMorphedHeight
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-selection.ts)

```ts
function sampleMorphedHeight(
  asset: TerrainAsset,
  patchIndex: number,
  x: number,
  z: number,
  selection: TerrainSelection): number
```
Sample the CPU reference surface used for shader parity and seam validation.

- **param** asset Borrowed resident terrain data.
- **param** patchIndex Row-major patch index.
- **param** x Patch-local X coordinate in samples.
- **param** z Patch-local Z coordinate in samples.
- **param** selection Borrowed fractional LOD and reconciled edge values.
- **returns** World-space height on the continuously morphed triangle surface.


## TerrainRenderer
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-renderer.ts)

TerrainRenderer Class Definition. Resident indexed terrain pass. Borrows device, asset, and caller's depth-tested render pass. Call prepare before starting the render pass; WebGPU writes and ordered compute must precede draws. Destroy before the borrowed device.

### constructor
```ts
constructor(device: Device, asset: TerrainAsset, colorFormat?: TextureFormatColor)
```
Create resident terrain resources and roll back all completed allocations on failure.

- **param** device Borrowed rendering device that must outlive this renderer.
- **param** asset Borrowed immutable terrain asset that must outlive this renderer.
- **param** colorFormat Optional color attachment format for the render pipeline.

### ready
```ts
async ready(): Promise<void>
```
Await pipeline validation before first use; native WebGPU pipeline creation validates shader entry points. Poll its status rather than requesting duplicate compilation-info promises (the adapter already owns those diagnostics).


### prepare
```ts
prepare(
    matrix: ArrayLike<number>,
    view: TerrainView,
    diagnostic: number = TERRAIN_DIAGNOSTIC.MATERIAL): void
```
Update selection/morphs and enqueue compute on WebGPU. Never reads GPU data. Matrix uses backend clip convention; projectionScale uses CSS pixels, so DPI does not change render quality. CPU timing includes selection/submission only.

- **param** matrix Borrowed column-major view-projection matrix using the active backend's clip convention.
- **param** view Borrowed camera and target state; projection scale is measured in CSS pixels.
- **param** diagnostic Diagnostic mode: 0 material, 1 LOD colors, or 2 grid overlay.
- **throws** When the renderer is disposed or frame inputs are invalid.

### draw
```ts
draw(pass: RenderPass): void
```
Draw into a borrowed pass; does not clear/submit or own its framebuffer.

- **param** pass Borrowed active depth-tested render pass; it is not ended or submitted here.

### drawGeometry
```ts
drawGeometry(pass: RenderPass, pipeline: RenderPipeline, bindings: Record<string, Binding>): void
```
Record resident indexed geometry with a borrowed scene or shadow pipeline.


### destroy
```ts
destroy(): void
```
Release only owned resources; repeated disposal is harmless.



## PbrEnvironment
Import from `joy-engine`. [Source](../../src/rendering/environment/pbr-environment.ts)

Owns luma-generated irradiance, filtered reflection, and BRDF textures. Source textures and the device are borrowed. Prepare submits integration work; call during setup or environment replacement, never from ordinary frame rendering.

### constructor
```ts
constructor(device: Device)
```

### bindings
```ts
get bindings()
```
Current borrowed texture bindings; null before preparation or after destruction.


### prepare
```ts
prepare(source: Texture, options: PbrEnvironmentOptions = {})
```
Atomically replace a prepared environment. Failure retains the accepted textures.


### preparePixels
```ts
preparePixels(pixels: PbrEnvironmentPixels, options: PbrEnvironmentOptions = {})
```
Prepare original pixels, owning their uploaded source texture as well as generated maps.


### destroy
```ts
destroy()
```
Release generated textures and integration resources, retaining the borrowed source.
