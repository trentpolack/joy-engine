// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import { MeshAsset, RenderScene } from 'joy-engine';
import type { MeshInstanceOptions, PointLight } from 'joy-engine';

const CUBE = new MeshAsset({
  positions: [-0.5,-0.5,-0.5, 0.5,-0.5,-0.5, 0.5,0.5,-0.5, -0.5,0.5,-0.5,
    -0.5,-0.5,0.5, 0.5,-0.5,0.5, 0.5,0.5,0.5, -0.5,0.5,0.5],
  indices: [0,2,1, 0,3,2, 4,5,6, 4,6,7, 0,1,5, 0,5,4,
    3,7,6, 3,6,2, 0,4,7, 0,7,3, 1,2,6, 1,6,5]
});

/** Deterministic Y-up geometry. The caller owns scene disposal; assets are borrowed. */
export function createRoom(alternate: boolean = false) {
  const scene = new RenderScene();
  function box(position: number[], scale: number[], tint: number[], sceneRoughness: number = 1) {
    const options: MeshInstanceOptions = {position, scale, tint, sceneRoughness};
    return scene.createMesh(CUBE, options);
  }

  // An open front and ceiling keep every diagnostic surface visible from the default camera.
  box([0,-0.2,0], [12,0.4,12], [0.62,0.65,0.7], 0.18);
  box([0,3,-5.8], [12,6,0.4], [0.7,0.72,0.76]);
  box([-5.8,3,0], [0.4,6,12], alternate ? [0.12,0.3,0.9] : [0.85,0.08,0.035]);
  box([5.8,3,0], [0.4,6,12], alternate ? [0.8,0.4,0.05] : [0.04,0.7,0.18]);
  box([-2.2,1.15,-1.8], [2.3,2.3,2.3], [0.76,0.78,0.81]).rotation[1] = 0.3;
  box([2.1,1.85,-2.4], [2,3.7,2], [0.62,0.7,0.87], 0.05).rotation[1] = -0.24;
  box([0,0.22,2.4], [2.1,0.44,1.6], [0.06,0.08,0.12], 0.05);
  const moving = box([0,1.1,1.2], [1.1,1.1,1.1], [0.95,0.55,0.06], 0.32);

  // Linear color exceeds one on purpose: bloom sees actual HDR scene radiance.
  box([0,4.9,-4.9], [2.8,0.16,0.15], [12,7,2]);
  box([-4.9,1.6,-3.4], [0.16,1.5,0.15], [3,0.12,0.08]);
  box([4.9,1.6,-3.4], [0.16,1.5,0.15], [0.12,3,0.4]);
  for(let index = 0; index < 14; index++) {
    box([-4.6 + index*0.34,1.8,-5.5], [0.035,2.7,0.15], [0.1,0.12,0.16]);
  }

  const lights: PointLight[] = [
    {x:0,y:4.5,z:-1.8,r:3.4,g:3,b:2.5,range:12},
    {x:-4.8,y:2,z:-1,r:1.3,g:0.08,b:0.035,range:6},
    {x:4.8,y:2,z:-1,r:0.04,g:1.2,b:0.18,range:6}
  ];
  for(let index = 0; index < 24; index++) {
    const angle = index*Math.PI/12;
    lights.push({x:Math.sin(angle)*4.8,y:0.8 + (index%3)*0.5,z:Math.cos(angle)*4.8,
      r:0.18 + (index%3)*0.06,g:0.16,b:0.25,range:3.5});
  }
  return {scene, moving, lights};
}
