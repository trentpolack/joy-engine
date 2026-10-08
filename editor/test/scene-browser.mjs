// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {chromium} from '@playwright/test';
import {createServer} from 'vite';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const output = new URL(`../../../artifacts/captures/${Date.now()}-editor-scene/`, import.meta.url);
const server = await createServer({configFile: `${root}scripts/development/vite-site.config.mjs`, server: {host: '127.0.0.1', port: 0, open: false}});
let browser;
try {
  await mkdir(output, {recursive: true});
  await server.listen();
  browser = await chromium.launch({channel: process.env.CAPTURE_CHANNEL, executablePath: process.env.CAPTURE_EXECUTABLE});
  const captures = [];
  for(const backend of ['webgpu', 'webgl']) {
    for(const postProcessing of [false, true]) {
      const page = await browser.newPage({viewport: {width: 640, height: 480}});
      const errors = [];
      page.on('pageerror', error => errors.push(error.message));
      page.on('console', message => {
        if(message.type() === 'error') {
          errors.push(message.text());
        }
      });
      await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/editor/`);
      const result = await page.evaluate(async ({backend, postProcessing, root}) => {
        const {SceneRenderer, RenderScene, MeshAsset} = await import(`/@fs/${root}joy-engine/src/index.ts`);
        const canvas = document.createElement('canvas');
        canvas.style.cssText = 'width:640px;height:480px;display:block';
        document.body.replaceChildren(canvas);
        document.body.style.margin = '0';
        const renderer = new SceneRenderer(canvas, {
          pixelRatio: 1,
          device: {backend, fallbackToWebGl: false},
          postProcessing: postProcessing ? {toneMapper: 'none', bloomStrength: 0, filmGrain: 0} : false
        });
        const scene = new RenderScene();
        // A transparent triangle forces the default fragment pipeline to produce color.
        scene.createMesh(new MeshAsset({positions: [-4, -4, 0, 4, -4, 0, 0, 4, 0], indices: [0, 1, 2]}), {tint: [1, 0, 0], opacity: 0.5});
        renderer.camera.target = [0, 0, 0];
        renderer.camera.yaw = 0;
        renderer.camera.pitch = 0;
        renderer.camera.distance = 6;
        await renderer.initialize();
        await new Promise(resolve => requestAnimationFrame(resolve));
        renderer.render(scene, {clearColor: [0, 0, 0, 1]});
        const copy = document.createElement('canvas');
        copy.width = 640;
        copy.height = 480;
        const context = copy.getContext('2d');
        context.drawImage(canvas, 0, 0);
        const pixel = Array.from(context.getImageData(320, 240, 1, 1).data);
        // Retain the rendered pixels for capture after releasing the GPU owners.
        canvas.replaceWith(copy);
        renderer.destroy();
        scene.destroy();
        return {pixel};
      }, {backend, postProcessing, root});
      await page.waitForTimeout(100);
      assert.deepEqual(errors, [], `${backend}: shader compilation and submission must succeed`);
      assert.ok(result.pixel[0] > 80 && result.pixel[1] < 10 && result.pixel[2] < 10, `${backend}: expected red transparent geometry, received ${result.pixel}`);
      const name = `${backend}-${postProcessing ? 'post' : 'direct'}.png`;
      await page.screenshot({path: fileURLToPath(new URL(name, output))});
      captures.push(name);
      console.log(`PASS ${backend}, post-processing ${postProcessing}: pixel ${result.pixel}`);
      await page.close();
    }
  }
  await writeFile(new URL('index.html', output), captures.map(name => `<figure><img src="${name}"><figcaption>${name}</figcaption></figure>`).join('\n'));
  console.log(`Gallery: ${fileURLToPath(new URL('index.html', output))}`);
} finally {
  await browser?.close();
  await server.close();
}
