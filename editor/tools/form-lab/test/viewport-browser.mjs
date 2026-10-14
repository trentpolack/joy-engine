// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {mkdir, writeFile, readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {chromium, expect} from '@playwright/test';
import {createServer} from 'vite';

const root = fileURLToPath(new URL('../', import.meta.url));
const output = new URL(`../../../../../artifacts/captures/${Date.now()}-form-viewport/`, import.meta.url);
const server = process.env.CAPTURE_URL ? null : await createServer({
  configFile: `${root}vite.config.mjs`,
  server: {host: '127.0.0.1', port: 0, open: false}
});
let browser;
try {
  await mkdir(output, {recursive: true});
  await server?.listen();
  const url = process.env.CAPTURE_URL ?? `http://127.0.0.1:${server.httpServer.address().port}/`;
  browser = await chromium.launch({channel: process.env.CAPTURE_CHANNEL, executablePath: process.env.CAPTURE_EXECUTABLE});
  const captures = [];
  for(const backend of ['webgpu', 'webgl']) {
    const page = await browser.newPage({viewport: {width: 1480, height: 900}, deviceScaleFactor: 2});
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
      if(message.type() === 'error') {
        errors.push(message.text());
      }
    });
    await page.goto(`${url}?backend=${backend}`);
    await expect(page.locator('#diagnostic')).toContainText('Built successfully', {timeout: 20000});
    await expect(page.locator('#backend')).toContainText(backend === 'webgpu' ? 'WEBGPU' : 'WEBGL 2');
    await expect(page.locator('#gpu-error')).toBeHidden();
    await expect.poll(() => page.locator('#viewport').evaluate(canvas =>
      Boolean(canvas.dataset.renderedRevision) && canvas.dataset.requestedRevision === canvas.dataset.renderedRevision
    )).toBe(true);
    const colors = await page.locator('#viewport').evaluate(async canvas => {
      // The viewport renders on demand. Queue an orbit redraw, then read its
      // submitted frame before the browser clears the drawing buffer.
      canvas.focus();
      canvas.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowLeft', bubbles: true}));
      await new Promise(resolve => requestAnimationFrame(resolve));
      const copy = document.createElement('canvas');
      copy.width = canvas.width;
      copy.height = canvas.height;
      const context = copy.getContext('2d');
      context.drawImage(canvas, 0, 0);
      const pixels = context.getImageData(0, 0, copy.width, copy.height).data;
      const unique = new Set();
      for(let offset = 0; offset < pixels.length; offset+= 4) {
        unique.add(`${pixels[offset]},${pixels[offset + 1]},${pixels[offset + 2]}`);
      }
      return unique.size;
    });
    assert.ok(colors > 32, `${backend}: viewport must contain rendered geometry, received ${colors} colors`);
    assert.deepEqual(errors, [], `${backend}: shader compilation and rendering must succeed`);
    await page.locator('#examples').selectOption('assets/examples/material-studio.form');
    await expect(page.locator('#preview-status')).toHaveAttribute('data-state', 'generated');
    await expect(page.locator('#gpu-error')).toBeHidden();
    await page.locator('#frame').click();
    const studioName = `${backend}-material-studio.png`;
    await page.screenshot({path: fileURLToPath(new URL(studioName, output))});
    captures.push(studioName);
    async function savedDocument() {
      const downloading = page.waitForEvent('download');
      await page.locator('#save').click();
      await page.locator('#artifact-format').selectOption('formlab');
      await page.locator('#artifact-destination').selectOption('download');
      await page.locator('#artifact-write').click();
      return JSON.parse(await readFile(await (await downloading).path(), 'utf8'));
    }
    const beforeLighting = await savedDocument();
    await page.locator('#lighting-tab').click();
    await expect(page.locator('#preview-environmentIntensity')).toBeVisible();
    await page.locator('#preview-environmentIntensity').fill('0.25');
    await page.locator('#preview-environmentIntensity').press('Tab');
    await page.locator('#preview-castsShadow').uncheck();
    assert.deepEqual(await savedDocument(), beforeLighting, 'preview lighting must not mutate saved materials');
    await page.locator('#preview-reset-lighting').click();
    await expect(page.locator('#preview-environmentIntensity')).toHaveValue('1');
    await expect(page.locator('#preview-castsShadow')).toBeChecked();
    assert.deepEqual(errors, [], `${backend}: preview light edits must not produce GPU errors`);
    // Embedded, original maps exercise independent red-channel factors and masked alpha.
    const textures = await page.evaluate(() => {
      const result = {};
      for(const channel of ['baseColor', 'normal', 'emissive', 'metallic', 'roughness', 'ao']) {
        const canvas = document.createElement('canvas');
        canvas.width = canvas.height = 32;
        const context = canvas.getContext('2d');
        for(let y = 0; y < 32; y++) {
          for(let x = 0; x < 32; x++) {
            const checker = ((x >> 3) + (y >> 3))%2;
            context.fillStyle = channel === 'normal' ? (checker ? 'rgb(185,128,240)' : 'rgb(70,128,240)')
              : channel === 'baseColor' ? (checker ? '#e8a354' : '#347e9e')
              : channel === 'emissive' ? (checker ? '#201009' : '#000000')
              : checker ? '#ffffff' : '#777777';
            if(channel !== 'baseColor' || checker) { context.fillRect(x,y,1,1); }
          }
        }
        result[channel] = canvas.toDataURL();
      }
      return result;
    });
    const document = structuredClone(beforeLighting);
    document.source+= '\nmaterial(polished)\nsurface(1,1,3,3,3.5)';
    document.overrides.polished = {kind: 'material', material: {
      name: 'MappedMetal', baseColor: [1,1,1,1], emissive: [0.3,0.2,0.1], metallic: 1, roughness: 0.4,
      alphaMode: 'MASK', alphaCutoff: 0.5, textures
    }};
    const previousRevision = await page.locator('#viewport').getAttribute('data-requested-revision');
    await page.locator('#file-input').setInputFiles({name:'channels.formlab', mimeType:'application/json', buffer:Buffer.from(JSON.stringify(document))});
    await expect(page.locator('#preview-status')).toHaveAttribute('data-state', 'generated');
    await expect(page.locator('#gpu-error')).toBeHidden();
    await expect(page.locator('#diagnostic')).toContainText('Built successfully');
    await expect.poll(() => page.locator('#viewport').getAttribute('data-requested-revision')).not.toBe(previousRevision);
    await expect.poll(() => page.locator('#viewport').evaluate(canvas => canvas.dataset.requestedRevision === canvas.dataset.renderedRevision)).toBe(true);
    await page.setViewportSize({width:1280,height:900});
    await expect.poll(() => page.locator('#viewport').evaluate(canvas => canvas.width/canvas.clientWidth)).toBeGreaterThan(1.5);
    await page.locator('#frame').click();
    await page.waitForTimeout(250);
    assert.deepEqual(errors, [], `${backend}: all material channels, transparency, and high-DPI resize must render`);
    if(backend === 'webgpu') {
      const shadowed = await page.locator('#viewport').screenshot();
      await page.locator('#preview-castsShadow').uncheck();
      await page.waitForTimeout(250);
      const unshadowed = await page.locator('#viewport').screenshot();
      assert.notDeepEqual(shadowed, unshadowed, 'masked PBR casters must affect directional shadows');
      const disabledName = 'webgpu-mask-casting-disabled.png';
      await page.screenshot({path:fileURLToPath(new URL(disabledName, output))});
      captures.push(disabledName);
      await page.locator('#preview-castsShadow').check();
      await page.waitForTimeout(250);
    }
    const name = `${backend}-all-channels.png`;
    await page.screenshot({path: fileURLToPath(new URL(name, output)), fullPage: true});
    captures.push(name);
    console.log(`PASS FORM LAB ${backend}: rendered geometry without GPU errors`);
    await page.close();
  }
  await writeFile(new URL('index.html', output), captures.map(name => `<figure><img src="${name}"><figcaption>${name}</figcaption></figure>`).join('\n'));
  console.log(`Gallery: ${fileURLToPath(new URL('index.html', output))}`);
} finally {
  await browser?.close();
  await server?.close();
}
