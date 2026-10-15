// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import type {
  PbrMaterial,
  PbrMesh,
  SceneLightFrame
} from 'joy-engine';

import {
  GpuTriangleRenderer,
  OrbitCamera
} from 'joy-engine';
import wgsl from 'joy-engine/shaders/rendering/lit-triangles/lit-triangles.wgsl?raw';
import vertex from 'joy-engine/shaders/rendering/lit-triangles/lit-triangles.vert.glsl?raw';
import fragment from 'joy-engine/shaders/rendering/lit-triangles/lit-triangles.frag.glsl?raw';

const CAPTURE_SIZE = 640;
const ALPHA_OPAQUE = 'OPAQUE';
const ALPHA_MASK = 'MASK';
const WEBGPU_BACKEND = 'webgpu';
const SUN_LIGHT = 'sun';

/**
 * Render a shadow regression fixture: only the floor is camera-visible; a high cutout casts vertically.
 * @param canvas Borrowed destination canvas; the fixture owns and destroys its temporary renderer.
 * @param source Base-color texture URL used by the elevated PBR blocker.
 * @param masked Whether the blocker discards texels below its alpha cutoff.
 * @param coloredBlocker Whether to add a vertex-color blocker above the floor.
 * @param coloredReceiver Whether the floor uses the vertex-color receiver path.
 * @returns Owned red-channel samples and a PNG data URL captured from the same submitted frame.
 * @throws If renderer initialization or material upload fails; completed renderer resources are released.
 */
export async function captureMaskShadow(
  canvas: HTMLCanvasElement,
  source: string,
  masked: boolean,
  coloredBlocker: boolean = false,
  coloredReceiver: boolean = false) {
  const camera = new OrbitCamera();
  camera.target = [0, 0, 0];
  camera.radius = 6;
  camera.distance = 18;
  camera.pitch = 0.9;
  camera.yaw = 0;
  camera.nearClip = 0.1;
  camera.farClip = 100;

  const renderer = await GpuTriangleRenderer.create(canvas, {
    pbr: true,
    shaderSources: {wgsl, glsl: {vertex, fragment}},
    uniformBufferSize: 1168,
    device: {backend: WEBGPU_BACKEND, fallbackToWebGl: false}
  });
  try {
    const material: PbrMaterial = {
      baseColor: [0.6, 0.6, 0.6, 1],
      emissive: [0, 0, 0],
      metallic: 0,
      roughness: 1,
      alphaMode: ALPHA_OPAQUE,
      alphaCutoff: 0.5,
      textures: {
        baseColor: null,
        normal: null,
        emissive: null,
        metallic: null,
        roughness: null,
        ao: null
      }
    };
    const positions = [
      -6, 0, -6, 6, 0, -6, 6, 0, 6, -6, 0, 6,
      -2, 20, -2, 2, 20, -2, 2, 20, 2, -2, 20, 2
    ];
    const mesh: PbrMesh = {
      positions,
      normals: Array.from({length: 8}, () => [0, 1, 0]).flat(),
      colors: Array(24).fill(1),
      alphas: Array(8).fill(1),
      uvs: [0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1],
      triangles: coloredReceiver ? [4, 6, 5, 4, 7, 6] : [0, 2, 1, 0, 3, 2, 4, 6, 5, 4, 7, 6],
      triangleMaterials: coloredReceiver ? [1, 1] : [0, 0, 1, 1],
      materials: [material, {
        ...material,
        alphaMode: masked ? ALPHA_MASK : ALPHA_OPAQUE,
        textures: {...material.textures, baseColor: source}
      }]
    };
    await renderer.setMaterialMesh(mesh);
    renderer.resize(CAPTURE_SIZE, CAPTURE_SIZE, 1);

    const basis = camera.basis();
    const uniforms = new Float32Array(292);
    uniforms.set(camera.matrix(1, true));
    uniforms.set([1, 1, 1, 1], 16);
    uniforms[21] = 0.02;
    uniforms.set([0, 1, 0, 4], 280);
    uniforms.set([1, 1, 1, 0], 284);

    const frame = {
      viewProjection: camera.matrix(1, true),
      eye: basis.eye,
      backward: basis.backward,
      lightDirection: [0, 1, 0],
      lightColor: [4, 4, 4],
      ambientColor: [0.02, 0.02, 0.02],
      shadowCamera: {
        viewMatrix: camera.viewMatrix(),
        projectionMatrix: camera.projectionMatrix(1, true),
        nearPlane: 0.1,
        farPlane: 100
      },
      shadows: {
        quality: 'balanced' as const,
        shadowDistance: 40,
        normalBias: 0.01
      }
    };

    // Read the same submitted frame before the canvas drawing buffer is cleared.
    return await new Promise<{samples: number[]; image: string}>(resolve => {
      requestAnimationFrame(renderAndCapture);

      /**
       * Submit the fixture and read its pixels synchronously inside one animation callback.
       * The enclosing renderer remains alive until the capture promise resolves.
       */
      function renderAndCapture(): void {
        const casterVertices: number[] = [];
        const receiverVertices: number[] = [];
        if(coloredReceiver) {
          for(const index of [0, 2, 1, 0, 3, 2]) {
            receiverVertices.push(positions[index*3], 0, positions[index*3 + 2], 0.6, 0.6, 0.6, 1);
          }
          casterVertices.push(...receiverVertices);
        }
        if(coloredBlocker) {
          for(const index of [0, 2, 1, 0, 3, 2]) {
            casterVertices.push(positions[index*3], 10, positions[index*3 + 2], 1, 1, 1, 1);
          }
        }

        let lightFrame: SceneLightFrame | null = null;
        if(coloredBlocker || coloredReceiver) {
          lightFrame = {
            view: renderer.createFrameView({
              camera: {...frame.shadowCamera, eye: basis.eye, backward: basis.backward},
              viewport: {cssWidth: CAPTURE_SIZE, cssHeight: CAPTURE_SIZE, pixelRatio: 1},
              ambient: 0.02,
              directional: {
                kind: SUN_LIGHT,
                direction: [0, 1, 0],
                color: [1, 1, 1],
                intensity: 4,
                angularRadius: 0.00465
              }
            }),
            casters: casterVertices
          };
        }
        renderer.render(receiverVertices, uniforms, [0, 0, 0, 1], [], 0, [], frame, {}, null, null, lightFrame);

        const copy = document.createElement('canvas');
        copy.width = CAPTURE_SIZE;
        copy.height = CAPTURE_SIZE;
        const context = copy.getContext('2d')!;
        context.drawImage(canvas, 0, 0);
        const pixels = context.getImageData(0, 0, CAPTURE_SIZE, CAPTURE_SIZE).data;
        const samples: number[] = [];
        const matrix = frame.viewProjection;

        // Samples are inside alternating texture cells, clear of checker boundaries.
        // The last sample lies beyond the blocker, so a colored floor must receive
        // direct light there while the original four samples remain shadowed.
        const samplePositions = coloredReceiver ? [-1.5, -0.5, 0.5, 1.5, 4] : [-1.5, -0.5, 0.5, 1.5];
        for(const x of samplePositions) {
          const z = 0.5;
          const w = matrix[3]*x + matrix[11]*z + matrix[15];
          const px = Math.round(((matrix[0]*x + matrix[8]*z + matrix[12])/w*0.5 + 0.5)*CAPTURE_SIZE);
          const py = Math.round((0.5 - (matrix[1]*x + matrix[9]*z + matrix[13])/w*0.5)*CAPTURE_SIZE);
          samples.push(pixels[(py*CAPTURE_SIZE + px)*4]);
        }
        resolve({samples, image: copy.toDataURL()});
      }
    });
  } finally {
    renderer.destroy();
  }
}
