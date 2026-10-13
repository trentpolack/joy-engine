# Scene Graph V1 Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans for inline work or superpowers:subagent-driven-development for delegated tasks. Track completion with the checkboxes below.

**Goal:** Centralize geometry ownership and pass participation under a real scene hierarchy.

**Architecture:** Scene nodes produce world-space snapshots; render adapters consume those snapshots and share frame-owned shadow maps. Keep borrowed assets independent from scene nodes and device resources.

**Tech Stack:** TypeScript, luma.gl 9.4.2, WGSL/GLSL, Node tests, Vite, and browser captures.

**Spec:** ../architecture/scene-graph-v1.md

## Global Constraints

- Preserve existing mesh/FORM call sites and default appearance.
- Parent changes preserve local transforms; reject cycles and cross-scene ownership before mutation.
- World transforms must preserve affine composition, including shear and negative/zero scales.
- Opaque shadow candidates are independent from the main-camera visibility list.
- Keep resident terrain indexed; no per-frame landscape triangle expansion.
- No new production dependencies. Retain WebGPU-first/WebGL 2 fallback and explicit resource cleanup.

## Review Focus

Nonuniform/negative scale; invalid or removed parents; direct transform-array mutations; off-camera shadow casters; removal/replacement while GPU resources or temporal history still reference geometry. Each owning task extends the existing behavioral tests for these cases.

## Task 1: Scene Hierarchy And Geometry Snapshots

Files: src/rendering/scene/scene-node.ts, scene-geometry.ts, render-scene.ts, mesh-instance.ts, mesh-frame.ts, mesh-transform.ts, prepare-frame.ts, mesh-collection.ts; src/core/types.ts; src/index.ts; test/mesh-scene.test.mjs; test/scene-frame.test.mjs.

Interfaces: SceneNodeOptions; SceneNode with parent, children, local transforms, inherited world visibility, setParent, and worldTransform; RenderScene.createNode; MeshInstance extends SceneNode; prepared geometry records carry identity, world bounds/transform, coverage, castsShadow, and receivesShadow. Keep existing mesh frame outputs for downstream renderer migration.

- [x] Extend behavioral tests for transform composition, parenting failure, subtree lifecycle, and independent pass participation; run and observe failures.
- [x] Implement scene ownership and one traversal to prepare geometry snapshots. Preserve collection semantics and existing public exports.
- [x] Run focused scene tests and type checks, and commit the coherent scene foundation.

## Task 2: Shared Shadow Submission

Files: src/rendering/shadows/, src/rendering/pbr/pbr-triangles.ts, src/rendering/renderers/gpu-triangle-renderer.ts, shaders/rendering/shadows/lit-shadow.wgsl; focused shadow/frame tests.

Interfaces: one owner prepares PreparedShadowBatch records; PBR and colored receivers borrow the same bindings; caster submissions use scene policy and independent light visibility.

- [x] Add a behavioral regression exposing duplicated shadow preparation and per-node opt-outs.
- [x] Centralize shadow ownership and bind shared results to existing material adapters. Preserve masked material coverage and ambient/emissive contributions.
- [x] Run focused renderer tests and commit the shared pass change.

## Task 3: Native Geometry Integration And Verification

Files: scene native-geometry contracts/adapter, terrain renderer/shaders, scene renderer, terrain or scene-effects example, docs/scenes.md, docs/terrain.md, CHANGELOG.md, and generated docs/api.

- [x] Connect native terrain to scene node ownership, inherited transforms, main depth, and shared shadow submission without CPU mesh expansion; validate allocation lifecycle and capability boundaries.
- [x] Exercise the feature in an existing example with nested nodes and mixed geometry. Preserve its normal controls and public engine imports.
- [x] Update guides, API reference, and changelog; run engine/workspace checks/tests and affected builds.
- [x] Capture available browser behavior, obtain an independent whole-branch review, address substantive findings, and prepare feature/scene-graph-v1 for delivery without merging.

## Verification Results
The implementation passed the engine check and all 210 engine tests, all workspace checks and 102 workspace tests, and production builds for Scene Effects and Terrain. The API reference was regenerated, and `git diff --check` passed. The actual Scene Effects `npm run dev` entrypoint served development modules; `npm run preview` started WebGPU successfully and emitted no routine production console logs or browser errors. Scene Effects retains its existing Vite chunk-size advisory.

Chromium 153 with SwiftShader exercised both WebGPU and WebGL. The mixed scene regression checked hierarchy movement, resident terrain color/depth, HDR, shared cube maps, caster/receiver opt-outs, planar receiver filtering, inherited hiding, and subtree removal with no shader/page errors. The material regression checked 27 lights, five-light batching, off-camera casters, alpha cutouts, colored/PBR cross-casting, and illuminated receiver normals. Captures are under ignored `artifacts/captures/`.

Independent review found and resolved two issues: native terrain incorrectly rejecting WebGL's effects fallback, and point-shadow filtering self-occluding planar receivers. Additional browser findings corrected WebGPU point reference depth and cube-face orientation. No actionable review findings remain. Related defects are recorded in #62, #63, and #64.

Safari, Edge, and physical GPU hardware were unavailable in this environment. These software-rendered captures establish rendering behavior, not world-scale performance. Scene-owned PBR materials, spatial indexing/instancing, streaming/runtime terrain edits, native normal/motion attachments, and shadowed volumetrics remain outside this first scene-graph increment; #60 and #61 track the larger rendering and landscape work.
