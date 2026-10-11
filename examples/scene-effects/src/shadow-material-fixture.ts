// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import {GpuTriangleRenderer, OrbitCamera} from 'joy-engine';
import type {PbrMaterial, PbrMesh} from 'joy-engine';
import wgsl from 'joy-engine/shaders/rendering/lit-triangles/lit-triangles.wgsl?raw';
import vertex from 'joy-engine/shaders/rendering/lit-triangles/lit-triangles.vert.glsl?raw';
import fragment from 'joy-engine/shaders/rendering/lit-triangles/lit-triangles.frag.glsl?raw';

/** Browser regression fixture: only the floor is camera-visible; the high cutout casts vertically. */
export async function captureMaskShadow(canvas: HTMLCanvasElement, source: string, masked: boolean, coloredBlocker: boolean = false) {
  const camera = new OrbitCamera();
  camera.target = [0,0,0];
  camera.radius = 6;
  camera.distance = 18;
  camera.pitch = 0.9;
  camera.yaw = 0;
  camera.nearClip = 0.1;
  camera.farClip = 100;
  const renderer = await GpuTriangleRenderer.create(canvas, {pbr:true, shaderSources:{wgsl, glsl:{vertex,fragment}},
    uniformBufferSize:1168, device:{backend:'webgpu',fallbackToWebGl:false}});
  try {
    const material: PbrMaterial = {baseColor:[0.6,0.6,0.6,1], emissive:[0,0,0], metallic:0, roughness:1,
      alphaMode:'OPAQUE', alphaCutoff:0.5, textures:{baseColor:null,normal:null,emissive:null,metallic:null,roughness:null,ao:null}};
    const positions = [-6,0,-6, 6,0,-6, 6,0,6, -6,0,6, -2,20,-2, 2,20,-2, 2,20,2, -2,20,2];
    const mesh: PbrMesh = {positions, normals:Array.from({length:8}, () => [0,1,0]).flat(),
      colors:Array(24).fill(1), alphas:Array(8).fill(1), uvs:[0,0,1,0,1,1,0,1, 0,0,1,0,1,1,0,1],
      triangles:[0,2,1,0,3,2,4,6,5,4,7,6], triangleMaterials:[0,0,1,1],
      materials:[material, {...material, alphaMode:masked ? 'MASK' : 'OPAQUE', textures:{...material.textures,baseColor:source}}]};
    await renderer.setMaterialMesh(mesh);
    renderer.resize(640,640,1);
    const basis = camera.basis();
    const uniforms = new Float32Array(292);
    uniforms.set(camera.matrix(1,true));
    const frame = {viewProjection:camera.matrix(1,true), eye:basis.eye, backward:basis.backward,
      lightDirection:[0,1,0], lightColor:[4,4,4], ambientColor:[0.02,0.02,0.02],
      shadowCamera:{viewMatrix:camera.viewMatrix(),projectionMatrix:camera.projectionMatrix(1,true),nearPlane:0.1,farPlane:100},
      shadows:{quality:'balanced' as const,shadowDistance:40,normalBias:0.01}};
    // Read the same submitted frame before the canvas drawing buffer is cleared.
    return await new Promise<{samples:number[]; image:string}>(resolve => requestAnimationFrame(() => {
      const casterVertices: number[] = [];
      if(coloredBlocker) {
        for(const index of [0,2,1,0,3,2]) {
          casterVertices.push(positions[index*3], 10, positions[index*3 + 2], 1,1,1,1);
        }
      }
      renderer.render([],uniforms,[0,0,0,1],[],0,[],frame,{},null,null, coloredBlocker ? {
        camera:frame.shadowCamera, lights:[], directional:{kind:'sun',direction:[0,1,0],color:[1,1,1],intensity:4,angularRadius:0.00465},
        casters:casterVertices
      } : null);
      const copy = document.createElement('canvas');
      copy.width = copy.height = 640;
      const context = copy.getContext('2d')!;
      context.drawImage(canvas,0,0);
      const pixels = context.getImageData(0,0,640,640).data;
      const samples: number[] = [];
      const matrix = frame.viewProjection;
      // Samples are inside alternating texture cells, clear of checker boundaries.
      for(const x of [-1.5,-0.5,0.5,1.5]) {
        const z = 0.5;
        const w = matrix[3]*x + matrix[11]*z + matrix[15];
        const px = Math.round(((matrix[0]*x + matrix[8]*z + matrix[12])/w*0.5 + 0.5)*640);
        const py = Math.round((0.5 - (matrix[1]*x + matrix[9]*z + matrix[13])/w*0.5)*640);
        samples.push(pixels[(py*640 + px)*4]);
      }
      resolve({samples,image:copy.toDataURL()});
    }));
  } finally {
    renderer.destroy();
  }
}
