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
function addCircle(vertices: number[], x: number, y: number, radius: number, color: RgbaColor, segments: number = 12, z: number = 0)
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
function addMesh(vertices: number[], mesh: ShadedMesh3D, x: number, y: number, scale: number, rotation: number, color: RgbaColor, z: number = 0)
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
function addQuad(vertices: number[], x: number, y: number, radius: number, color: RgbaColor, rotation: number = Math.PI / 4, z: number = 0)
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
function addTriangle(vertices: number[], x: number, y: number, radius: number, color: RgbaColor, rotation: number = 0, z: number = 0)
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
function addPolygon(vertices: number[], x: number, y: number, radius: number, color: RgbaColor, sides: number, rotation: number, z: number)
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
function addRadialPolygon(vertices: number[], x: number, y: number, radius: number, color: RgbaColor, shape: readonly number[], rotation: number, z: number)
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
  vertices: number[], x: number, y: number, radius: number, centerColor: RgbaColor, edgeColor: RgbaColor, shape: readonly number[], rotation: number, z: number,
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
function addRing(vertices: number[], x: number, y: number, radius: number, width: number, color: RgbaColor, segments: number, z: number)
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
function addLine(vertices: number[], ax: number, ay: number, bx: number, by: number, width: number, color: RgbaColor, z: number)
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
Import from `joy-engine`. [Source](../../src/rendering/renderers/gpu-triangle-renderer.ts)

Depth-tested 3D triangle rendering through luma.gl's portable Device API. Games provide world-space vertices, camera uniforms, and matching shaders. Opaque triangles write depth. Translucent triangles use straight-alpha source-over blending and test against opaque depth without writing it. The caller supplies translucent triangles already sorted back-to-front for its camera. Both batches share one framebuffer and one clear per frame.

### create
```ts
static async create(canvas: HTMLCanvasElement, options: RendererOptions): Promise<GpuTriangleRenderer>
```
- **param** canvas Canvas dedicated to GPU rendering.
- **param** options Backend-specific shader sources.

### constructor
```ts
constructor(device: Device, canvasContext: CanvasContext, { shaderSources, texturedShaderSources, opaqueShaderSources, uniformBufferSize = 16, initialVertexBufferSize = 64*1024, postProcessing, sceneEffects, pbr = false, atmosphere = false }: RendererOptions)
```
- **param** device
- **param** canvasContext
- **param** options

### getDiagnostics
```ts
getDiagnostics(options: RendererDiagnosticOptions = {}): RendererDiagnostics | null
```
Copy requested luma counters on demand. Scope is all luma devices in this realm, not this renderer alone. Returns null after disposal; performs no GPU readback.


### setMaterialMesh
```ts
async setMaterialMesh(mesh: PbrMesh): Promise<boolean>
```
Upload a world-space material mesh; await readiness before displaying new document data. Requires pbr at creation. The renderer owns buffers/textures and copies input data.

- **param** mesh

### setPostProcessingConfig
```ts
setPostProcessingConfig(config: Partial<PostProcessingConfig>)
```
Replace authored settings for the next frame without reallocating GPU resources. Requires postProcessing at creation; omitted fields reset to engine defaults. Invalid settings throw before modifying the last working configuration.

- **param** config Borrowed settings, copied on success.

### setSceneEffectsConfig
```ts
setSceneEffectsConfig(config: SceneEffectsOptions)
```
Replace the optional scene graph atomically; failed configuration retains the previous graph.


### resetSceneHistory
```ts
resetSceneHistory()
```

### sceneVolumeStats
```ts
get sceneVolumeStats()
```

### bindVertexBuffer
```ts
bindVertexBuffer(buffer: Buffer = this.vertexBuffer?.buffer)
```
Bind the interleaved stream using each backend's vertex-array semantics.

- **param** [buffer]

### resize
```ts
resize(width: number, height: number, pixelRatio: number)
```
Resize the backing surface without changing the caller's logical geometry.

- **param** width Logical width in CSS pixels.
- **param** height Logical height in CSS pixels.
- **param** pixelRatio Backing pixels per CSS pixel.

### render
```ts
render(vertices: number[] | Float32Array, uniformData: Float32Array<ArrayBuffer>, clearColor: RgbaColor, transparentVertices: number[] | Float32Array = [], timeSeconds: number = 0, transparentDraws: readonly TransparentDraw[] = [], materialFrame: PbrFrame | null = null, sceneFrame: SceneEffectsFrame = {}, atmosphereUniforms: Float32Array<ArrayBuffer> | null = null, attachmentFrame: SceneAttachmentFrame | null = null)
```
Submit opaque geometry, then sorted translucent geometry; clear even if empty.

- **param** vertices Triangle list of x, y, z, r, g, b, a values.
- **param** uniformData Caller-defined values fitting the configured uniform buffer.
- **param** clearColor Frame background.
- **param** transparentVertices Back-to-front x/y/z/RGBA triangles.
- **param** timeSeconds Animation time used by temporal post effects.
- **param** transparentDraws Additional globally sorted material runs (XYZ/RGBA, or XYZ/RGBA/UV with texture). Borrowed textures must outlive submission. Requires texturedShaderSources for texture runs.
- **param** materialFrame Camera/lighting for the optional PBR mesh. Null hides it.
- **param** [sceneFrame] Borrowed motion/camera inputs for requested scene effects. Depth defaults to this scene's owned attachment. Callers must supply real velocities and projection jitter; reset history after camera cuts or scene replacement.
- **param** [atmosphereUniforms] Optional packed background snapshot. Requires atmosphere at creation; null hides it.

### destroy
```ts
destroy()
```
Release luma.gl resources when the owning game is disposed.



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

### render
```ts
render(sourceTexture: Texture, frame: SceneEffectsFrame = {}): Texture
```
Validate every attachment before resizing or mutating history, then execute scene-linear passes.


### resize
```ts
resize(width: number, height: number)
```
Resize owned targets and invalidate history. Drawing-buffer dimensions are positive integer pixels.


### resetHistory
```ts
resetHistory()
```
Invalidate history for camera cuts, visibility changes, or resumed rendering.


### destroy
```ts
destroy()
```
Release owned resources; borrowed source, attachments, buffers, and device remain alive.


### assertAlive
```ts
#assertAlive()
```


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
function sortCameraTransparentTriangles(vertices: readonly number[], backward: readonly number[]): number[]
```
Back-to-front centroid sorting for any camera orientation; leaves input untouched. Intersecting transparent surfaces remain an approximation, not order-independent transparency.

- **param** vertices Interleaved XYZ/RGBA triangles.
- **param** backward Unit vector from target toward camera.


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
Import from `joy-engine`. [Source](../../src/rendering/pbr/point-lights.ts)

```ts
const LIT_TRIANGLE_SHADERS
```
Flat, double-sided diffuse lighting for XYZ/RGBA opaque triangles. Normals come from surface derivatives; lights do not cast shadows. Linear HDR output feeds the renderer's optional scene post processing.



## createPointLightUniforms
Import from `joy-engine`. [Source](../../src/rendering/pbr/point-lights.ts)

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
Import from `joy-engine`. [Source](../../src/rendering/pbr/point-lights.ts)

```ts
const POINT_LIGHT_UNIFORM_BYTES
```


## DEFAULT_PBR_LIGHTING
Import from `joy-engine`. [Source](../../src/rendering/pbr/pbr-triangles.ts)

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


## MeshInstance
Import from `joy-engine`. [Source](../../src/rendering/scene/mesh-instance.ts)

Mutable presentation state borrowing an immutable MeshAsset. No simulation state.

### constructor
```ts
constructor(asset: MeshAsset, options: MeshInstanceOptions = {})
```
- **param** asset
- **param** [options]

### set
```ts
set(options: MeshInstanceOptions)
```
Copy supplied settings; omitted fields retain their previous value. Distances are world units; rotations use XYZ Euler radians. Zero scale is supported.

- **param** options


## RenderScene
Import from `joy-engine`. [Source](../../src/rendering/scene/render-scene.ts)

Owns presentation instances; assets are borrowed and remain valid after removal.

### constructor
```ts
constructor()
```

### createMesh
```ts
createMesh(asset: MeshAsset, options?: MeshInstanceOptions)
```
Create mutable presentation state owned by this scene and borrowing the supplied asset.

- **param** asset
- **param** [options]

### remove
```ts
remove(instance: MeshInstance)
```
Remove an owned instance; repeated removal is harmless.

- **param** instance

### createCollection
```ts
createCollection()
```
Create a scene-owned keyed collection for synchronizing simulation views.


### clear
```ts
clear()
```
Release instance references; collections remain reusable until scene destruction.


### destroy
```ts
destroy()
```
Idempotent disposal. The scene owns no GPU resources or simulation objects.


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
- **param** canvas
- **param** [options]

### initialize
```ts
async initialize()
```
Initialize once; destruction during startup releases the late GPU result.


### ready
```ts
get ready()
```

### backend
```ts
get backend()
```

### colorFormat
```ts
get colorFormat()
```

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

### setSceneEffectsConfig
```ts
setSceneEffectsConfig(config: SceneEffectsOptions)
```
Replace individual opt-ins atomically; preserves the working graph if validation fails.


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


### render
```ts
render(scene: RenderScene, frame: SceneFrameOptions = {}): boolean
```
Render in seconds. Canvas dimensions are CSS pixels; backing density never changes view. Atmosphere must be enabled at construction. A borrowed environment frame drives both background and directional/skylight irradiance; omitting it hides the sky. Transient triangles are a borrowed escape hatch for bespoke procedural feedback/particles.

- **param** scene
- **param** [frame]
- **returns** Whether the frame was rendered; false if the renderer is not ready or the canvas is hidden.

### screenRay
```ts
screenRay(clientX: number, clientY: number): WorldRay | null
```
Client CSS pixels to a world ray; independent of backing pixel density.

- **param** clientX
- **param** clientY
- **returns** World ray or null if the canvas is hidden or has no area.

### pickGround
```ts
pickGround(x: number, y: number, height: number = 0): readonly number[] | null
```
Pick a horizontal plane at world-space height.

- **param** x
- **param** y
- **param** [height]
- **returns** World position or null if the canvas is hidden, has no area, or the ray is parallel to the plane.

### project
```ts
project(position: readonly number[]): Vector2 | null
```
World position to canvas-local CSS pixels, or null behind camera/when hidden.

- **param** position
- **returns** Canvas-local CSS pixels or null if the position is behind the camera or the canvas is hidden.

### setPostProcessingConfig
```ts
setPostProcessingConfig(config: Partial<PostProcessingConfig>)
```
Set postprocessing configuration. Must be called after initialization and before rendering.

- **param** config

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


## loadTerrain
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-asset.ts)

```ts
async function loadTerrain(url: string | URL): Promise<TerrainAsset>
```
Load a caller-owned resident heightfield. Binary is uint16 little endian, row-major; source row zero maps to negative world Z, columns to positive X. Scales are explicit world units; camera LOD never changes this data.

- **param** url


## decodeTerrain
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-asset.ts)

```ts
function decodeTerrain(manifest: TerrainManifest, binary: ArrayBuffer): TerrainAsset
```
Validate before GPU allocation and decode portable little-endian samples. Copies binary input; consumers must treat returned data as immutable.

- **param** manifest
- **param** binary


## sampleTerrain
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-asset.ts)

```ts
function sampleTerrain(asset: TerrainAsset, worldX: number, worldZ: number)
```
Stable collision/gameplay sample, independent of camera and render topology. Clamps to landscape bounds. Uses the same diagonal triangle surface at stride 1.

- **param** asset
- **param** worldX
- **param** worldZ


## selectTerrain
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-selection.ts)

```ts
function selectTerrain(asset: TerrainAsset, view: TerrainView): TerrainSelection[]
```
Select fixed quadtree leaves using conservative AABB distance and prebuilt geometric errors. projectionScale is CSS viewport height/(2*tan(vertical FOV/2)). Max cell size is 32 CSS pixels; target distance is horizontal world units. Returns bounded descriptors; no authored heights are scanned here.

- **param** asset
- **param** view


## reconcileTerrainEdges
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-selection.ts)

```ts
function reconcileTerrainEdges(asset: TerrainAsset, details: number[]): TerrainSelection[]
```
Both sides use the coarser fractional lattice, including its continuous morph. Endpoints are authored tile corners at all supported levels, so four-way corners remain invariant. Arbitrary neighbor ratios are supported.

- **param** asset
- **param** details


## sampleMorphedHeight
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-selection.ts)

```ts
function sampleMorphedHeight(asset: TerrainAsset, patchIndex: number, x: number, z: number, selection: TerrainSelection)
```
CPU reference for shader parity and seam validation; local coordinates in samples.

- **param** asset
- **param** patchIndex
- **param** x
- **param** z
- **param** selection


## TerrainRenderer
Import from `joy-engine`. [Source](../../src/rendering/terrain/terrain-renderer.ts)

Resident indexed terrain pass. Borrows device, asset and caller's depth-tested render pass. Call prepare before starting the render pass; WebGPU writes and ordered compute must precede draws. Destroy before the borrowed device.

### constructor
```ts
constructor(device: Device, asset: TerrainAsset, colorFormat?: TextureFormatColor)
```
- **param** device
- **param** asset
- **param** [colorFormat]

### ready
```ts
async ready()
```
Await pipeline validation before first use; native WebGPU pipeline creation validates shader entry points. Poll its status rather than requesting duplicate compilation-info promises (the adapter already owns those diagnostics).


### prepare
```ts
prepare(matrix: ArrayLike<number>, view: TerrainView, diagnostic: number = 0)
```
Update selection/morphs and enqueue compute on WebGPU. Never reads GPU data. Matrix uses backend clip convention; projectionScale uses CSS pixels, so DPI does not change render quality. CPU timing includes selection/submission only.

- **param** matrix
- **param** view
- **param** [diagnostic] 0 material, 1 LOD colors, 2 grid overlay.

### draw
```ts
draw(pass: RenderPass)
```
Draw into a borrowed pass; does not clear/submit or own its framebuffer.

- **param** pass

### destroy
```ts
destroy()
```
Release only owned resources; repeated disposal is harmless.
