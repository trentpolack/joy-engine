// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {chromium, expect} from '@playwright/test';
import {createServer} from 'vite';

const output = new URL(`../../../artifacts/captures/${Date.now()}-light-shadows/`, import.meta.url);
await mkdir(output, {recursive:true});
const server = process.env.CAPTURE_URL ? null : await createServer({
  configFile: fileURLToPath(new URL('../vite.config.js', import.meta.url)), server: {host: '127.0.0.1', port: 0}
});
await server?.listen();
const url = process.env.CAPTURE_URL ?? `${server.resolvedUrls.local[0]}?backend=webgpu`;
const browser = await chromium.launch({
  ...(process.env.CAPTURE_EXECUTABLE_PATH ? {executablePath: process.env.CAPTURE_EXECUTABLE_PATH} : {channel: process.env.CAPTURE_CHANNEL ?? 'chrome'}),
  args: JSON.parse(process.env.CAPTURE_ARGS ?? '[]')
});
try {
  const page = await browser.newPage({viewport:{width:1480,height:900}});
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => { if(message.type() === 'error') { errors.push(message.text()); } });
  await page.goto(url);
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true');
  await expect.poll(() => page.evaluate(() => window.sceneEffectsExample.snapshot().diagnostics.shadows.castingPointLights)).toBe(27);
  const captures = [];
  async function capture(name) {
    await page.screenshot({path:fileURLToPath(new URL(`${name}.png`, output))});
    captures.push(name);
  }
  await capture('27-point-lights');
  await page.evaluate(() => window.sceneEffectsExample.shadowFixture(true));
  await expect.poll(() => page.evaluate(() => window.sceneEffectsExample.snapshot().diagnostics.shadows.castingPointLights)).toBe(5);
  const snapshot = await page.evaluate(() => window.sceneEffectsExample.snapshot());
  assert.equal(snapshot.diagnostics.shadows.batches, 2);
  assert.equal(snapshot.diagnostics.shadows.views, 30);
  assert.ok(snapshot.diagnostics.scene.culled >= 1, 'high blocker must be outside the camera frustum');
  await capture('off-camera-caster');
  const shadowed = await page.locator('#world').screenshot();
  await page.evaluate(() => window.sceneEffectsExample.setLightCasting(0, false));
  await expect.poll(() => page.evaluate(() => window.sceneEffectsExample.snapshot().diagnostics.shadows.castingPointLights)).toBe(4);
  assert.equal((await page.evaluate(() => window.sceneEffectsExample.snapshot())).lights, 5);
  const unshadowed = await page.locator('#world').screenshot();
  assert.notDeepEqual(shadowed, unshadowed, 'casting opt-out must change visible coverage while keeping the light');
  await capture('primary-light-casting-disabled');
  await page.evaluate(() => {
    for(let index = 0; index < 5; index++) { window.sceneEffectsExample.setLightColor(index, [0,0,0]); }
  });
  await page.waitForTimeout(100);
  const noDirect = await page.locator('#world').screenshot();
  await page.evaluate(() => window.sceneEffectsExample.setLightColor(4, [8,8,8]));
  await page.waitForTimeout(100);
  const fifthOnly = await page.locator('#world').screenshot();
  assert.notDeepEqual(noDirect, fifthOnly, 'fifth light must illuminate visible surfaces through batch two');
  await capture('fifth-light-only');
  // Keep fixture imports on the same Vite server as the example when a host
  // mounts it under a route prefix, avoiding a second copy of luma.gl.
  const exampleBase = new URL('./', page.url()).pathname;
  const fixturePath = `${exampleBase}@fs${fileURLToPath(new URL('../src/shadow-material-fixture.ts', import.meta.url))}`;
  // Each fixture owns its device. Close the main example and each fixture page
  // before requesting another adapter, including on software WebGPU backends.
  const fixtureUrl = new URL('__shadow-material', page.url()).href;
  await page.close();
  const maskPixels = {};
  for(const [name, masked, coloredBlocker, coloredReceiver] of [
    ['solid', false, false, false], ['masked', true, false, false],
    ['colored', true, true, false], ['coloredReceiver', false, false, true]
  ]) {
    const fixturePage = await browser.newPage({viewport:{width:640,height:640}});
    fixturePage.on('pageerror', error => errors.push(error.message));
    fixturePage.on('console', message => { if(message.type() === 'error') { errors.push(message.text()); } });
    await fixturePage.route(fixtureUrl, route => route.fulfill({contentType:'text/html',body:'<canvas></canvas>'}));
    await fixturePage.goto(fixtureUrl);
    maskPixels[name] = await fixturePage.evaluate(async ({path, masked, coloredBlocker, coloredReceiver}) => {
    const {captureMaskShadow} = await import(path);
    const source = document.createElement('canvas');
    source.width = source.height = 32;
    const context = source.getContext('2d');
    context.fillStyle = '#ffffff';
    for(let y = 0; y < 4; y++) {
      for(let x = 0; x < 4; x++) {
        if((x + y)%2) { context.fillRect(x*8,y*8,8,8); }
      }
    }
    return captureMaskShadow(document.querySelector('canvas'), source.toDataURL(), masked, coloredBlocker, coloredReceiver);
    }, {path:fixturePath, masked, coloredBlocker, coloredReceiver});
    await fixturePage.close();
  }
  for(const index of [0,2]) {
    assert.ok(maskPixels.masked.samples[index] > maskPixels.solid.samples[index] + 60,
      `alpha holes must illuminate receiver samples: ${JSON.stringify({solid:maskPixels.solid.samples,masked:maskPixels.masked.samples})}`);
  }
  for(const index of [1,3]) {
    assert.ok(maskPixels.masked.samples[index] <= maskPixels.solid.samples[index] + 20, 'opaque alpha cells retain shadow');
  }
  assert.ok(maskPixels.colored.samples.every(value => value < 30), 'colored caster queue must shadow PBR receivers');
  assert.ok(maskPixels.coloredReceiver.samples.slice(0,4).every(value => value < 30), 'PBR caster must shadow colored receivers');
  assert.ok(maskPixels.coloredReceiver.samples[4] > Math.max(...maskPixels.coloredReceiver.samples.slice(0,4)) + 100,
    `colored receiver normal bias must preserve unoccluded direct light: ${JSON.stringify(maskPixels.coloredReceiver.samples)}`);
  for(const [name, result] of Object.entries(maskPixels)) {
    await writeFile(new URL(`${name}-caster.png`, output), Buffer.from(result.image.split(',')[1], 'base64'));
    captures.push(`${name}-caster`);
  }
  assert.deepEqual(errors, []);
  await writeFile(new URL('index.html', output), captures.map(name => `<figure><img src="${name}.png"><figcaption>${name}</figcaption></figure>`).join('\n'));
  console.log(`PASS point shadows, 27 lights, five-light batching, camera-culled caster, opt-out, and colored receiver lighting. Gallery: ${fileURLToPath(new URL('index.html', output))}`);
} finally {
  await browser.close();
  await server?.close();
}
