// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

export type { Vector2 } from './core/types.ts';
export type { Vector3 } from './core/types.ts';
export type { Velocity2 } from './core/types.ts';
export type { MovingBody } from './core/types.ts';
export type { RgbaColor } from './core/types.ts';
export type { IndexedMesh3D } from './core/types.ts';
export type { ShadedTriangle3D } from './core/types.ts';
export type { ShadedMesh3D } from './core/types.ts';
export { createSeededRandom } from './core/random.ts';
export { createConfigDocumentValues, createConfigSchema, createConfigValues } from './core/config-values.ts';
export type { ConfigField } from './core/config-values.ts';
export { AudioBank } from './browser/audio-bank.ts';
export type { SoundDefinition } from './browser/audio-bank.ts';
export type { AudioBankOptions } from './browser/audio-bank.ts';
export { startGameLoop } from './runtime/game-loop.ts';
export { EnvironmentSystem } from './environment/environment-system.ts';
export { createEnvironmentLighting, createEnvironmentUniforms, ENVIRONMENT_UNIFORM_BYTES } from './rendering/environment/environment-uniforms.ts';
export { ATMOSPHERE_RENDER_UNIFORM_BYTES, ATMOSPHERE_SHADERS, createAtmosphereRenderUniforms } from './rendering/environment/atmosphere-rendering.ts';
export type { EnvironmentOptions } from './environment/environment-system.ts';
export type { EnvironmentFrame } from './environment/environment-system.ts';
export type { AtmosphereLight } from './environment/environment-system.ts';
export { Entity } from './gameplay/entity.ts';
export { EntityComponent } from './gameplay/component.ts';
export { ComponentRegistry } from './gameplay/component-registry.ts';
export type { ComponentFactory } from './gameplay/component-registry.ts';
export { EntitySystem } from './gameplay/entity-system.ts';
export type { EntitySystemOptions } from './gameplay/entity-system.ts';
export { Character, PlayerCharacter } from './gameplay/character.ts';
export { Controller } from './gameplay/controller.ts';
export { PlayerController, AIController } from './gameplay/player-controller.ts';
export { EntityRegistry } from './gameplay/entity-registry.ts';
export { EntityWorld } from './gameplay/entity-world.ts';
export type { EntityWorldOptions } from './gameplay/entity-world.ts';
export { copyEntityDefinition } from './gameplay/entity-definition.ts';
export { LevelDocument, parseLevel, serializeLevel } from './gameplay/level-document.ts';
export { LevelScene } from './gameplay/level-scene.ts';
export { LevelRuntime } from './gameplay/level-runtime.ts';
export { PhysicsWorld } from './physics/physics-world.ts';
export { PhysicsBody } from './physics/physics-body.ts';
export { PhysicsVehicle } from './physics/physics-vehicle.ts';
export type { EntityDefinition } from './gameplay/types.ts';
export type { EntityOptions } from './gameplay/types.ts';
export type { EntityTransform } from './gameplay/types.ts';
export type { ControlIntent } from './gameplay/types.ts';
export type { JsonValue } from './gameplay/types.ts';
export type { JsonRecord } from './gameplay/types.ts';
export type { LevelData } from './gameplay/level-document.ts';
export type { PhysicsWorldOptions } from './physics/physics-world.ts';
export type { PhysicsBodyOptions } from './physics/physics-body.ts';
export type { PhysicsVehicleOptions } from './physics/physics-vehicle.ts';
export type { PhysicsWheelOptions } from './physics/physics-vehicle.ts';
export type { PhysicsVehicleInput } from './physics/physics-vehicle.ts';
export type { PhysicsTransform } from './physics/physics-body.ts';
export { InputController } from './browser/input-controller.ts';
export { addCircle, addMesh, addQuad, addTriangle, addPolygon, addRadialPolygon, addGradientRadialPolygon, addRing, addLine } from './rendering/geometry/geometry.ts';
export { loadGlbMesh, parseGlbMesh } from './dcc/glb-mesh-loader.ts';
export { requireElement } from './browser/dom.ts';
export { DevTools } from './browser/diagnostics/dev-tools.ts';
export type { DevToolsOptions } from './browser/diagnostics/dev-tools.ts';
export type { RendererDiagnostics, RendererDiagnosticOptions, RendererDiagnosticSource } from './rendering/gpu/gpu-diagnostics.ts';
export { GpuTriangleRenderer } from './rendering/renderers/gpu-triangle-renderer.ts';
export { calculateBackingPixelRatio } from './rendering/display.ts';
export { createTopDownOrthographicMatrix, sortTopDownTransparentTriangles } from './rendering/camera/top-down-camera.ts';
export { DynamicGpuBuffer } from './rendering/gpu/dynamic-gpu-buffer.ts';
export { createGpuCanvasDevice, createPortableShaders, gpuBackendFromQuery } from './rendering/gpu/gpu-device.ts';
export * from './rendering/scene/effects.ts';
export { ShaderPassRenderer } from './rendering/gpu/shader-pass-renderer.ts';
export { GLSLShaderAssembler, WGSLShaderAssembler } from '@luma.gl/shadertools';
export { SceneEffects } from './rendering/scene/scene-effects.ts';
export { agxToneMapping, animatedGrain, antialias, linearExposure, vignetteBlur } from './rendering/postprocessor/supplemental-passes.ts';
export type { SceneEffectsOptions } from './rendering/scene/scene-effect-config.ts';
export type { SceneEffectsFrame } from './rendering/scene/scene-effects.ts';
export type { GpuBackend } from './rendering/gpu/gpu-device.ts';
export type { ShaderPass } from '@luma.gl/shadertools';
export type { ShaderPassPipeline } from '@luma.gl/shadertools';
export {
  DEFAULT_POST_PROCESSING,
  POST_PROCESSING_SCHEMA,
  normalizePostProcessingConfig,
  createPostProcessingUniforms,
  toneMapperIndex,
  addPostProcessingShaders,
} from './rendering/postprocessor/config.ts';
export { POST_PROCESSING_PRESETS, validatePostProcessingProfile, resolvePostProcessingProfile } from './rendering/postprocessor/profile.ts';
export type { PostProcessingProfile } from './rendering/postprocessor/profile.ts';
export type { PostProcessingConfig } from './rendering/postprocessor/config.ts';
export {
  TAU,
  clamp,
  directionBetween,
  distanceBetween,
  distanceToRectangleEdge,
  randomInteger,
  randomRange,
} from './core/math.ts';
export { parseHexColor, withAlpha } from './core/color.ts';

export type { ParticleEffectAsset } from './particles/asset.ts';
export type { ParticleEmitterAsset } from './particles/asset.ts';
export type { ParticleParameter } from './particles/asset.ts';
export type { CompiledParticleEffect } from './particles/asset.ts';
export type { EffectParticle } from './particles/particle-effect.ts';
export type { ParticleEffectOptions } from './particles/particle-effect.ts';
export { compileParticleEffect } from './particles/asset.ts';
export { ParticleEffect, PARTICLE_STEP_SECONDS } from './particles/particle-effect.ts';
export { ParticleScriptError } from './particles/script.ts';
export { appendParticleEffect } from './particles/rendering.ts';

export { OrbitCamera, crossVectors3, sortCameraTransparentTriangles } from './rendering/camera/orbit-camera.ts';
export { EditorGrid } from './rendering/editor-grid.ts';
export type { EditorGridView, EditorGridFrame } from './rendering/editor-grid.ts';
export { PARTICLE_XY_BASIS } from './particles/rendering.ts';
export type { ParticleCameraBasis } from './particles/rendering.ts';

export { createParticleEffectBatches } from "./particles/rendering.ts";
export { ParticleTextureSet, PARTICLE_TEXTURE_SHADERS } from "./particles/texture-set.ts";
export { DEFAULT_PARTICLE_TEXTURE } from "./particles/texture-source.ts";

export { collectParticleLights } from './particles/lights.ts';
export { createParticleRenderMetrics } from './particles/rendering.ts';
export { LIT_TRIANGLE_SHADERS, createPointLightUniforms, POINT_LIGHT_UNIFORM_BYTES } from './rendering/pbr/point-lights.ts';

export type { ParticlePointLight } from './particles/lights.ts';
export type { ParticleProfile } from './particles/profile.ts';
export type { ParticleRenderMetrics } from './particles/rendering.ts';
export type { ParticleLight } from './particles/asset.ts';

export type { PbrMaterial } from './core/types.ts';
export type { PbrMesh } from './core/types.ts';
export type { PbrFrame } from './core/types.ts';

export { DEFAULT_PBR_LIGHTING } from './rendering/pbr/pbr-triangles.ts';

export { MeshAsset } from './rendering/scene/mesh-asset.ts';
export { MeshInstance } from './rendering/scene/mesh-instance.ts';
export { RenderScene } from './rendering/scene/render-scene.ts';
export type {PointLight} from './rendering/pbr/point-lights.ts';
export { SceneRenderer } from './rendering/scene/scene-renderer.ts';
export { projectToViewport, screenRay, intersectRayPlane } from './rendering/camera/projection.ts';
export type { MeshAssetData } from './core/types.ts';
export type { MeshInstanceOptions } from './core/types.ts';
export type { SceneRendererOptions } from './rendering/scene/scene-renderer.ts';
export type { SceneFrameOptions } from './rendering/scene/scene-renderer.ts';

export { InputManager } from './browser/input/input-manager.ts';
export type { InputAction } from './browser/input/input-manager.ts';
export type { InputAxis } from './browser/input/input-manager.ts';
export type { InputBinding } from './browser/input/input-manager.ts';
export type { InputSnapshot } from './browser/input/input-manager.ts';
export { assetPath } from './gameplay/asset-path.ts';
export { parseObject, serializeObject, createObjectInstance, resolveObjectInstance, resolveLevelObjects } from './gameplay/object-document.ts';
export { AssetScene } from './gameplay/asset-scene.ts';

// Resident terrain contract and indexed CLOD pass; independent of scene triangles.
export { loadTerrain, decodeTerrain, sampleTerrain } from './rendering/terrain/terrain-asset.ts';
export { selectTerrain, reconcileTerrainEdges, sampleMorphedHeight } from './rendering/terrain/terrain-selection.ts';
export { TerrainRenderer } from './rendering/terrain/terrain-renderer.ts';
export type { TerrainAsset } from './rendering/terrain/terrain-asset.ts';
export type { TerrainManifest } from './rendering/terrain/terrain-asset.ts';
export type { TerrainView } from './rendering/terrain/terrain-selection.ts';
export type { TerrainSelection } from './rendering/terrain/terrain-selection.ts';
