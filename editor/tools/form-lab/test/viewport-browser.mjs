// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
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
    const page = await browser.newPage({viewport: {width: 1480, height: 900}});
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
    const name = `${backend}.png`;
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
