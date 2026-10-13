// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {chromium} from '@playwright/test';
import {createServer} from 'vite';

const server = await createServer({configFile: fileURLToPath(new URL('../vite.config.js', import.meta.url)), server: {host:'127.0.0.1',port:0}});
await server.listen();
const output = new URL(`../../../artifacts/captures/${Date.now()}-scene-graph/`, import.meta.url);
await mkdir(output, {recursive:true});
const browser = await chromium.launch({
  ...(process.env.CAPTURE_EXECUTABLE_PATH ? {executablePath:process.env.CAPTURE_EXECUTABLE_PATH} : {channel:process.env.CAPTURE_CHANNEL ?? 'chrome'}),
  args:JSON.parse(process.env.CAPTURE_ARGS ?? '[]')
});
const results = [];
try {
  for(const backend of (process.env.CAPTURE_BACKENDS ?? 'webgpu,webgl').split(',')) {
    const page = await browser.newPage({viewport:{width:900,height:700}});
    const errors = [];
    page.on('pageerror', error => { errors.push(error.message); console.error(error.message); });
    page.on('console', message => { if(message.type() === 'error') { errors.push(message.text()); console.error(message.text()); } });
    await page.route('**/__scene-graph', route => route.fulfill({contentType:'text/html',body:'<style>body{margin:0;background:#111}canvas{width:900px;height:700px;display:block}</style><canvas id="world"></canvas>'}));
    await page.goto(`${server.resolvedUrls.local[0]}__scene-graph`);
    const fixture = `/@fs${fileURLToPath(new URL('../src/scene-graph-fixture.ts', import.meta.url))}`;
    await page.evaluate(async ({fixture,backend}) => {
      const {createSceneGraphFixture} = await import(fixture);
      window.fixture = await createSceneGraphFixture(document.querySelector('canvas'), backend);
    }, {fixture,backend});
    async function render() {
      const snapshot = await page.evaluate(() => new Promise((resolve, reject) => requestAnimationFrame(() => {
        try {
          const snapshot = window.fixture.render();
          const canvas = document.querySelector('canvas');
          const copy = document.createElement('canvas');
          copy.width = canvas.width;
          copy.height = canvas.height;
          const context = copy.getContext('2d');
          context.drawImage(canvas, 0, 0);
          const floorSamples = [[750,350], [150,550], [450,550], [750,550]].map(([x,y]) =>
            Array.from(context.getImageData(x,y,1,1).data).slice(0,3));
          resolve({...snapshot, floorSamples});
        } catch(error) { reject(error); }
      })));
      return {...snapshot, image:await page.locator('canvas').screenshot()};
    }
    // Pipeline validation can complete asynchronously after the first recorded frame.
    await render();
    const initial = await render();
    assert.equal(initial.diagnostics.scene.visible, 2);
    assert.equal(initial.diagnostics.shadows.available, backend === 'webgpu');
    await writeFile(new URL(`${backend}-mixed.png`,output), initial.image);
    if(backend === 'webgpu') {
      await page.evaluate(() => { window.fixture.mesh.castsShadow = false; });
      const selfCasting = await render();
      await page.evaluate(() => { window.fixture.terrain.castsShadow = false; });
      const noSelfCasting = await render();
      assert.ok(noSelfCasting.floorSamples.every(sample => sample[1] > 10), 'floor samples must contain lit terrain');
      for(let index = 0; index < selfCasting.floorSamples.length; index++) {
        assert.ok(selfCasting.floorSamples[index].every((value, channel) =>
          Math.abs(value - noSelfCasting.floorSamples[index][channel]) <= 2),
        'offset point-shadow taps must not turn a planar receiver into its own blocker');
      }
      await page.evaluate(() => { window.fixture.mesh.castsShadow = true; window.fixture.terrain.castsShadow = true; });
    }
    await page.evaluate(() => { window.fixture.rig.position[0] = 2; });
    const moved = await render();
    assert.notDeepEqual(moved.transform, initial.transform);
    assert.ok(!moved.image.equals(initial.image), 'parent movement must reach color and depth');
    if(backend === 'webgpu') {
      assert.equal(initial.diagnostics.shadows.views, 6, 'terrain and meshes share one point-light cube');
      await page.evaluate(() => { window.fixture.mesh.castsShadow = false; });
      const noCast = await render();
      await writeFile(new URL('webgpu-shadowed.png',output),moved.image);
      await writeFile(new URL('webgpu-no-mesh-cast.png',output),noCast.image);
      assert.ok(!noCast.image.equals(moved.image), 'mesh casts onto native terrain');
      await page.evaluate(() => { window.fixture.mesh.castsShadow = true; window.fixture.terrain.receivesShadow = false; });
      const noReceive = await render();
      assert.ok(!noReceive.image.equals(moved.image), 'native terrain receiver policy changes shadow coverage');
      await page.evaluate(() => { window.fixture.terrain.receivesShadow = true; window.fixture.canopy.visible = true; });
      const canopyCast = await render();
      await page.evaluate(() => { window.fixture.canopy.castsShadow = false; });
      const canopyNoCast = await render();
      assert.ok(!canopyCast.image.equals(canopyNoCast.image), 'native terrain casts onto mesh and terrain receivers');
      await writeFile(new URL('webgpu-native-caster.png',output), canopyCast.image);
    }
    await page.evaluate(() => { window.fixture.world.visible = false; });
    const hidden = await render();
    assert.equal(hidden.diagnostics.scene.visible, 0);
    await page.evaluate(() => { window.fixture.world.visible = true; window.fixture.scene.remove(window.fixture.world); });
    const removed = await render();
    assert.equal(removed.diagnostics.scene.visible, 0);
    assert.ok(removed.image.equals(hidden.image), 'subtree removal releases every geometry representation');
    await page.evaluate(() => window.fixture.destroy());
    assert.deepEqual(errors, []);
    results.push({backend, shadows:initial.diagnostics.shadows, visible:initial.diagnostics.scene.visible});
    await page.close();
  }
  await writeFile(new URL('results.json',output),JSON.stringify(results,null,2));
  console.log(`PASS scene hierarchy, native terrain color/depth, shared shadows, policies, and subtree removal. Captures: ${fileURLToPath(output)}`);
} finally {
  await browser.close();
  await server.close();
}
