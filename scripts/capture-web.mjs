// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

// Browser smoke checks for a built Rust game, tool, or example. Behavioral tests stay in Cargo;
// this checks real startup, GPU presentation, resize, and browser errors, and saves visual evidence.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { chromium, firefox, webkit } from '@playwright/test';
import { preview } from 'vite';

const engineRoot = fileURLToPath(new URL('../', import.meta.url));
const { values } = parseArgs({ options: {
  directory: { type: 'string', default: process.cwd() },
  slug: { type: 'string' },
  built: { type: 'boolean', default: false },
  'preview-directory': { type: 'string' },
  'artifacts-directory': { type: 'string', default: resolve(engineRoot, 'artifacts/captures') }
} });
const captureTimeoutMilliseconds = Number(process.env.CAPTURE_TIMEOUT_MS ?? 90000);
assert.ok(Number.isSafeInteger(captureTimeoutMilliseconds) && captureTimeoutMilliseconds > 0,
  'CAPTURE_TIMEOUT_MS must be a positive integer in milliseconds.');
const directory = resolve(values.directory);
const manifest = JSON.parse(await readFile(resolve(directory, 'package.json'), 'utf8'));
const slug = values.slug ?? basename(directory);
assert.match(slug, /^[a-zA-Z0-9_-]+$/, 'Capture slug must be a plain directory name.');
const output = resolve(values['artifacts-directory'], `${Date.now()}-${slug}`);
await mkdir(output, { recursive: true });

if(!values.built) {
  const build = spawnSync(process.platform === 'win32' ? 'npm.cmd' : 'npm', ['run', 'build', '--', '--dev'], {
    cwd: directory, stdio: 'inherit'
  });
  if(build.error || build.status !== 0) {
    throw build.error ?? new Error(`Browser development build failed with status ${build.status}.`);
  }
}

const browserName = process.env.CAPTURE_BROWSER ?? 'chromium';
const browserType = { chromium, firefox, webkit }[browserName];
assert.ok(browserType, `Unsupported CAPTURE_BROWSER: ${browserName}`);
const backend = process.env.CAPTURE_BACKEND ?? 'webgl';
assert.ok(['auto', 'webgl', 'webgpu', 'fallback'].includes(backend), `Unsupported CAPTURE_BACKEND: ${backend}`);
const server = await preview({
  root: directory, configFile: false, logLevel: 'error',
  preview: { host: '127.0.0.1', port: 0 }
});
const errors = [];
const captures = [];
const report = { passed: false, browser: browserName, requestedBackend: backend, built: values.built, errors, captures };
let browser;
let page;

try {
  // Keep normal capability detection: forcing unsafe WebGPU can select an unusable software backend.
  const args = [];
  if(browserName === 'chromium' && process.env.CAPTURE_SOFTWARE === '1') {
    args.push('--use-angle=swiftshader', '--enable-unsafe-swiftshader');
  }
  browser = await browserType.launch({
    headless: process.env.CAPTURE_HEADED !== '1',
    executablePath: process.env.CAPTURE_EXECUTABLE,
    ...(browserName === 'chromium' ? { channel: process.env.CAPTURE_CHANNEL } : {}),
    args
  });
  page = await browser.newPage({
    viewport: { width: 1440, height: 960 }, deviceScaleFactor: Number(process.env.CAPTURE_DPR ?? 1)
  });
  page.setDefaultTimeout(captureTimeoutMilliseconds);
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if(message.type() === 'error') {
      errors.push(message.text());
    }
  });
  if(backend === 'fallback') {
    await page.addInitScript(() => Object.defineProperty(navigator, 'gpu', { get: () => undefined }));
  }
  const url = new URL(server.resolvedUrls.local[0]);
  if(backend === 'webgl' || backend === 'webgpu') {
    url.searchParams.set('backend', backend);
  }
  await page.goto(url.href);
  await page.waitForFunction(() => document.querySelector('canvas[data-ready="true"]')
    || document.querySelector('#joy-failure')
    || document.querySelector('#joy-loading[data-state="failed"]'), undefined, { timeout: captureTimeoutMilliseconds });
  const failurePanel = page.locator('#joy-failure');
  const failure = await failurePanel.count() ? await failurePanel.textContent() : null;
  assert.equal(failure, null, failure ?? 'Application started.');
  const loadingFailure = page.locator('#joy-loading[data-state="failed"] [data-field="detail"]');
  const startupFailure = await loadingFailure.count() ? await loadingFailure.textContent() : null;
  assert.equal(startupFailure, null, startupFailure ?? 'Bindings initialized.');
  // The loading screen fades out over the first frame; capture the canvas once it is gone.
  await page.locator('#joy-loading').waitFor({ state: 'hidden' });
  const canvas = page.locator('canvas[data-ready="true"]').first();
  await canvas.waitFor({ state: 'visible' });
  report.backend = await canvas.getAttribute('data-backend');
  if(backend === 'webgpu') {
    assert.equal(report.backend, 'WebGPU');
  } else if(backend === 'webgl' || backend === 'fallback') {
    assert.equal(report.backend, 'WebGL 2');
  }
  await page.waitForTimeout(500);
  const canvasBounds = await canvas.boundingBox();
  assert.ok(canvasBounds, 'The presented canvas has visible bounds.');
  // Software rendering can delay animation frames even when canvas bounds are fixed; no actionability wait is needed.
  const screenshot = await page.screenshot({ clip: canvasBounds });
  report.distinctColors = await page.evaluate(async encoded => {
    const pixels = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
    const image = await createImageBitmap(new Blob([pixels], { type: 'image/png' }));
    const surface = document.createElement('canvas');
    surface.width = image.width;
    surface.height = image.height;
    const context = surface.getContext('2d');
    context.drawImage(image, 0, 0);
    image.close();
    const data = context.getImageData(0, 0, surface.width, surface.height).data;
    const colors = new Set();
    for(let offset = 0; offset < data.length; offset+= 64) {
      colors.add(`${data[offset]},${data[offset + 1]},${data[offset + 2]}`);
      if(colors.size >= 32) {
        break;
      }
    }
    return colors.size;
  }, screenshot.toString('base64'));
  assert.ok(report.distinctColors > 1, 'The presented canvas contains more than a uniform clear color.');
  await capture('overview');
  await page.setViewportSize({ width: 1024, height: 768 });
  await page.waitForTimeout(200);
  const size = await canvas.evaluate(element => ({
    width: element.width, height: element.height, cssWidth: element.clientWidth, cssHeight: element.clientHeight
  }));
  assert.equal(size.cssWidth, 1024);
  assert.equal(size.cssHeight, 768);
  assert.ok(size.width > 0 && size.height > 0, 'The resized drawing surface has pixels.');
  await capture('resized');
  assert.deepEqual(errors, []);

  await page.setViewportSize({ width: 1280, height: 720 });
  await page.waitForTimeout(200);
  const previewPath = resolve(output, 'preview.png');
  await page.screenshot({ path: previewPath });
  if(values['preview-directory']) {
    const previewDirectory = resolve(values['preview-directory']);
    await mkdir(previewDirectory, { recursive: true });
    await copyFile(previewPath, resolve(previewDirectory, basename(manifest.gameSite?.screenshot ?? `${slug}.png`)));
  }
  report.passed = true;
  console.log(`Browser capture passed: ${resolve(output, 'index.html')}`);
} catch(error) {
  report.error = String(error);
  if(page) {
    await capture('failure').catch(() => {});
  }
  throw error;
} finally {
  await writeFile(resolve(output, 'results.json'), JSON.stringify(report, null, 2));
  const figures = captures.map(name => `<figure><img src="${name}.png"><figcaption>${name}</figcaption></figure>`).join('\n');
  await writeFile(resolve(output, 'index.html'), `<!doctype html><meta charset="utf-8"><title>${slug} browser capture</title>
<style>body{background:#10212b;color:#f2f2e8;font:16px system-ui;padding:24px}img{max-width:100%}</style>
<h1>${slug}</h1><p>${report.passed ? 'Passed' : 'Failed'} · ${browserName} · ${backend}</p>${figures}`);
  await browser?.close();
  await server.close();
}

/** Save the rendered page; failure captures are evidence even when startup never presents. */
async function capture(name) {
  await page.screenshot({ path: resolve(output, `${name}.png`) });
  captures.push(name);
}
