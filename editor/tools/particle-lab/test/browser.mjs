// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium, expect } from '@playwright/test';
import { createServer, preview } from 'vite';

const root = fileURLToPath(new URL('../', import.meta.url));
const built = process.argv.includes('--built');
const directory = new URL(`../../../../../artifacts/captures/${Date.now()}-particle-lab/`, import.meta.url);
const previewPath = new URL('../../../../../platform/site/content/previews/particle-lab.png', import.meta.url);
await mkdir(directory, { recursive: true });

const server = built
  ? await preview({ root, configFile: false, preview: { host: '127.0.0.1', port: 0 } })
  : await createServer({configFile: `${root}vite.config.mjs`, server: {host: '127.0.0.1', port: 0}});
/** @type {import('@playwright/test').Browser | null} */
let browser = null;
/** @type {import('@playwright/test').Page | null} */
let page = null;
const captures = [];
const errors = [];

/** @param {string} name */
async function capture(name) {
  if(!page) {
    return;
  }
  await page.screenshot({ path: fileURLToPath(new URL(`${name}.png`, directory)), fullPage: true });
  captures.push(name);
}

/** Read complete authored state through the public export flow, independent of editor virtualization. */
async function exportedAsset() {
  if(!page) {
    throw new Error('Browser page is not ready.');
  }
  const download = page.waitForEvent('download');
  await page.locator('#save').click();
  const file = await download;
  const path = await file.path();
  if(!path) {
    throw new Error('Asset download did not produce a file.');
  }
  return JSON.parse(await readFile(path, 'utf8'));
}

/** Resolve the active stylesheet palette through the browser's color parser. @param {string} token */
async function paletteColor(token) {
  if(!page) {
    throw new Error('Browser page is not ready.');
  }
  return page.evaluate(name => {
    const probe = document.createElement('span');
    probe.style.color = `var(${name})`;
    probe.hidden = true;
    document.body.append(probe);
    const color = getComputedStyle(probe).color;
    probe.remove();
    return color;
  }, token);
}

try {
  if(!built) {
    await server.listen();
  }
  const address = server.httpServer.address();
  if(!address || typeof address === 'string') {
    throw new Error('Preview server did not bind.');
  }
  const url = `http://127.0.0.1:${address.port}/?backend=webgl`;

  browser = await chromium.launch({
    executablePath: process.env.CAPTURE_EXECUTABLE,
    channel: process.env.CAPTURE_CHANNEL,
    args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  page = await browser.newPage({ viewport: { width: 1480, height: 900 } });
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if(message.type() === 'error') {
      const location = message.location().url;
      errors.push(location ? `${message.text()} (${location})` : message.text());
    }
  });

  await page.goto(url);
  await expect(page.locator('#diagnostic')).toContainText('LIVE', { timeout: 20_000 });
  await expect(page.locator('#diagnostic')).toHaveAttribute('data-state', 'live');
  await expect(page.locator('#diagnostic')).toHaveCSS('color', await paletteColor('--green'));
  await expect(page.locator('#backend')).toContainText('WEBGL 2');
  await expect(page.locator('#gpu-error')).toBeHidden();
  await expect(page.locator('#particle-count')).not.toHaveText('0');
  await page.evaluate(() => document.fonts.ready);
  await capture('01-impact');
  const previewBeforeFullscreen = await page.locator('#viewport').boundingBox();
  await page.locator('.layout-maximize').click();
  await expect(page.locator('.layout-maximize')).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(async () => (await page.locator('#viewport').boundingBox()).width).toBeCloseTo(previewBeforeFullscreen.width, 0);
  await capture('preview-fullscreen');
  await page.keyboard.press('Escape');
  await expect(page.locator('.layout-maximize')).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(async () => (await page.locator('#viewport').boundingBox()).width).toBeCloseTo(previewBeforeFullscreen.width, 0);
  await expect(page.locator('.layout-maximize')).toBeFocused();

  // The default example exercises scalar, vector, and color properties together.
  const wind = page.getByRole('region', { name: 'wind property', exact: true });
  const tint = page.getByRole('region', { name: 'tint property', exact: true });
  const beforeCollapse = await exportedAsset();
  await expect(page.locator('#diagnostic')).toHaveText('LIVE');
  for(const name of ['power', 'wind', 'tint']) {
    await page.getByRole('button', { name: `Collapse ${name} values`, exact: true }).click();
    await expect(page.getByRole('button', { name: `Expand ${name} values`, exact: true })).toHaveAttribute('aria-expanded', 'false');
  }
  await expect(wind.getByLabel('wind X slider', { exact: true })).toBeHidden();
  await expect(wind.locator('.parameter-bounds')).toBeHidden();
  await expect(wind.getByLabel('wind current value', { exact: true })).toHaveText('X 8 · Y 0 · Z 0');
  await expect(page.getByLabel('power current value', { exact: true })).toHaveText('1');
  await expect(tint.getByLabel('tint color preview', { exact: true })).toBeVisible();
  await expect(tint.getByLabel('tint A value', { exact: true })).toBeHidden();
  assert.deepEqual(await exportedAsset(), beforeCollapse, 'collapse is UI state, not an asset edit');
  await expect(page.locator('#diagnostic')).toHaveText('LIVE');
  await capture('12-collapsed-properties');
  // Rebuilding the property list must preserve each property's collapsed state.
  await page.locator('#add-emitter').click();
  await page.locator('[data-preset="spray"]').click();
  await expect(wind.getByLabel('wind X slider', { exact: true })).toBeHidden();
  await page.locator('#delete-emitter').click();
  await page.locator('.emitter-item').first().click();
  for(const name of ['power', 'wind', 'tint']) {
    const toggle = page.getByRole('button', { name: `Expand ${name} values`, exact: true });
    await toggle.focus();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('button', { name: `Collapse ${name} values`, exact: true })).toBeFocused();
  }
  await expect(wind.getByLabel('wind X value', { exact: true })).toHaveValue('8');
  await expect(wind.getByLabel('wind X minimum', { exact: true })).toBeHidden();
  await wind.locator('summary').click();
  await expect(wind.getByLabel('wind X minimum', { exact: true })).toBeVisible();
  await wind.getByLabel('wind X maximum', { exact: true }).fill('4');
  await wind.getByLabel('wind X maximum', { exact: true }).press('Tab');
  await expect(wind.getByLabel('wind X value', { exact: true })).toHaveValue('4');
  await expect(wind.getByLabel('wind X slider', { exact: true })).toHaveAttribute('max', '4');
  assert.equal((await exportedAsset()).parameters.wind.value[0], 4);
  await page.locator('#live').uncheck();
  await tint.getByLabel('tint A value', { exact: true }).fill('0.5');
  await tint.getByLabel('tint A value', { exact: true }).press('Tab');
  await expect(page.locator('#diagnostic')).toHaveText('PENDING CHANGES');
  await expect(page.locator('#diagnostic')).toHaveAttribute('data-state', 'pending');
  await expect(page.locator('#diagnostic')).toHaveCSS('color', await paletteColor('--action-orange'));
  // Hosted tools share the status semantics but intentionally use the editor palette.
  await page.evaluate(() => document.documentElement.classList.add('joy-hosted'));
  try {
    await expect(page.locator('#diagnostic')).toHaveText('PENDING CHANGES');
    await expect(page.locator('#diagnostic')).toHaveAttribute('data-state', 'pending');
    await expect(page.locator('#diagnostic')).toHaveCSS('color', await paletteColor('--action-orange'));
  } finally {
    await page.evaluate(() => document.documentElement.classList.remove('joy-hosted'));
  }
  await expect(tint.getByLabel('tint A slider', { exact: true })).toHaveValue('0.5');
  assert.equal((await exportedAsset()).parameters.tint.value[3], 0.5);
  await tint.getByLabel('tint A value', { exact: true }).fill('2');
  await tint.getByLabel('tint A value', { exact: true }).press('Tab');
  await expect(tint.getByLabel('tint A value', { exact: true })).toHaveValue('2');
  await expect(tint.getByLabel('tint A value', { exact: true })).toHaveAttribute('aria-invalid', 'true');
  await expect(tint.locator('.numeric-error').first()).toBeVisible();
  const invalidDownload = page.waitForEvent('download', {timeout: 500}).then(() => true, () => false);
  await page.locator('#save').click();
  assert.equal(await invalidDownload, false, 'An invalid draft must block exporting the previous value.');
  await page.locator('#apply').click();
  await expect(page.locator('#diagnostic')).toHaveAttribute('data-state', 'pending');
  await expect(tint.getByLabel('tint A value', { exact: true })).toHaveValue('2');
  await tint.getByLabel('tint A value', { exact: true }).fill('0.5');
  await tint.getByLabel('tint A value', { exact: true }).press('Tab');
  await expect(tint.getByLabel('tint A value', { exact: true })).toHaveAttribute('aria-invalid', 'false');
  assert.equal((await exportedAsset()).parameters.tint.value[3], 0.5);
  await page.locator('#apply').click();
  await expect(page.locator('#error')).toBeHidden();
  await expect(page.locator('#diagnostic')).toHaveText('LIVE');
  await expect(page.locator('#diagnostic')).toHaveAttribute('data-state', 'live');
  await expect(page.locator('#diagnostic')).toHaveCSS('color', await paletteColor('--green'));
  await page.locator('#add-parameter').click();
  await page.getByLabel('parameter1 type', { exact: true }).selectOption('vector3');
  await expect(page.getByLabel('parameter1 Z value', { exact: true })).toBeVisible();
  await page.getByLabel('parameter1 type', { exact: true }).selectOption('color');
  await expect(page.getByLabel('parameter1 A value', { exact: true })).toHaveValue('1');
  await page.getByLabel('parameter1 type', { exact: true }).selectOption('scalar');
  await expect(page.getByLabel('parameter1 value', { exact: true })).toHaveValue('1');
  await page.getByRole('button', { name: 'Delete parameter1', exact: true }).click();
  await expect(page.getByLabel('parameter1 type', { exact: true })).toHaveCount(0);
  await page.locator('#add-emitter').hover();
  await expect(page.locator('#add-emitter')).toHaveCSS('background-color', await paletteColor('--action-orange'));
  await expect(page.locator('#add-emitter')).toHaveCSS('color', await paletteColor('--viewport'));
  await page.locator('#delete-emitter').hover();
  await expect(page.locator('#delete-emitter')).toHaveCSS('background-color', await paletteColor('--danger'));
  await page.locator('#asset-name').hover();
  await expect(page.locator('#delete-emitter')).toHaveCSS('border-color', await paletteColor('--danger'));
  await wind.locator('summary').click();
  await page.locator('.library-panel').evaluate(element => {
    element.scrollTop = 0;
  });
  await capture('10-property-bounds');
  const propertyAsset = await exportedAsset();
  await page.locator('#file-input').setInputFiles({
    name: 'properties.joyfx', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(propertyAsset)),
  });
  assert.deepEqual((await exportedAsset()).parameters, propertyAsset.parameters);
  await page.locator('#examples').selectOption('0');
  await page.locator('#live').check();
  await page.locator('#apply').click();
  await expect(page.locator('#diagnostic')).toHaveText('LIVE');

  // Camera movement must change the view while simulation stays paused.
  await page.locator('#play').click();
  await page.locator('#timeline').evaluate(element => {
    const input = /** @type {HTMLInputElement} */ (element);
    input.value = '0.35';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await expect(page.locator('#time')).toHaveText('0.35 s');
  const canvas = page.locator('#viewport');
  const beforeOrbit = await canvas.screenshot();
  const pausedCameraTime = await page.locator('#time').textContent();
  await canvas.focus();
  await page.keyboard.press('ArrowRight');
  const afterOrbit = await canvas.screenshot();
  assert.notDeepEqual(beforeOrbit, afterOrbit, 'orbit must change the perspective view');
  assert.equal(await page.locator('#time').textContent(), pausedCameraTime);
  await capture('05-perspective-orbit');
  await page.locator('#frame-effect').click();
  await expect(page.locator('#gpu-error')).toBeHidden();
  await page.locator('#reset-camera').click();
  const resetView = await canvas.screenshot();
  assert.deepEqual(resetView, beforeOrbit, 'reset must restore the same paused camera view');
  await page.locator('#play').click();

  await page.setViewportSize({ width: 900, height: 900 });
  // Panel widths settle through ResizeObserver after the viewport resize.
  await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await capture('06-tablet');
  await page.setViewportSize({ width: 1480, height: 900 });

  await page.locator('#help').click();
  await expect(page.locator('#reference-dialog')).toBeVisible();
  await expect(page.locator('#reference-dialog')).toContainText('not JavaScript');
  await expect(page.locator('#reference-dialog')).toContainText('four custom numeric storage slots');
  await page.locator('#close-reference').click();

  const emitterCount = await page.locator('.emitter-item').count();
  await page.locator('#add-emitter').click();
  await page.locator('[data-preset="spray"]').click();
  await expect(page.locator('.emitter-item')).toHaveCount(emitterCount + 1);
  await expect(page.locator('.emitter-item').last()).toContainText('emitter');
  await page.locator('#duplicate-emitter').click();
  await expect(page.locator('.emitter-item')).toHaveCount(emitterCount + 2);
  await expect(page.locator('.emitter-item').last()).toContainText('_copy');

  await page.locator('#examples').selectOption('1');
  await expect(page.locator('#asset-name')).toHaveValue('Solar Swirl');
  await expect(page.locator('#seed')).toHaveValue('7777');
  await expect(page.locator('[data-tab="json"]')).toHaveCount(0);
  const originalAsset = await exportedAsset();
  const jsonAsset = structuredClone(originalAsset);
  jsonAsset.name = 'JSON Imported Swirl';
  jsonAsset.emitters[0].id = 'json_orbit';
  await page.locator('#file-input').setInputFiles({
    name: 'import.joyfx', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(jsonAsset)),
  });
  await expect(page.locator('#asset-name')).toHaveValue('JSON Imported Swirl');
  await expect(page.locator('.emitter-item').first()).toContainText('json_orbit');
  await page.locator('#asset-name').fill('Edited Imported Name');
  assert.equal((await exportedAsset()).name, 'Edited Imported Name');
  await page.locator('#file-input').setInputFiles({
    name: 'invalid.json', mimeType: 'application/json', buffer: Buffer.from('{ invalid asset'),
  });
  await expect(page.locator('#error')).toBeVisible();
  assert.equal((await exportedAsset()).name, 'Edited Imported Name');

  const longChildAsset = {
    version: 1,
    name: 'Long Child Horizon',
    maxParticles: 16,
    parameters: {},
    emitters: [
      { id: 'root', enabled: true, root: true, duration: 0, rate: 0, bursts: [{ time: 0, count: 1 }], lifetime: 0.1, renderer: 'soft', spawn: '', update: '', death: { emitter: 'child', count: 1 } },
      { id: 'child', enabled: true, root: false, duration: 0, rate: 0, bursts: [], lifetime: 10, renderer: 'soft', spawn: '', update: '' },
    ],
  };
  await page.locator('#file-input').setInputFiles({
    name: 'long-child.joyfx', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify(longChildAsset)),
  });
  await expect(page.locator('#asset-name')).toHaveValue('Long Child Horizon');
  await expect(page.locator('#timeline')).toHaveAttribute('max', '30');
  await expect(page.locator('#diagnostic')).toHaveAttribute('data-state', 'live');
  await page.locator('#speed').selectOption('4');
  await expect.poll(async () => Number.parseFloat(await page.locator('#time').textContent() ?? '0'), {
    message: 'Looping must wait for long-lived child particles to drain.', timeout: 10_000
  }).toBeGreaterThan(1.1);

  await page.locator('#examples').selectOption('1');
  await expect(page.locator('#asset-name')).toHaveValue('Solar Swirl');

  await page.locator('#play').click();
  await expect(page.locator('#play')).toContainText('Play');
  const pausedTime = await page.locator('#time').textContent();
  await page.locator('#step').click();
  await expect(page.locator('#time')).not.toHaveText(pausedTime ?? '');
  await page.locator('#play').click();
  await expect(page.locator('#play')).toContainText('Pause');

  await page.locator('[data-tab="spawn"]').click();
  const spawnEditor = page.locator('#spawn-source .cm-content');
  await expect(page.locator('#spawn-source .cm-lineNumbers')).toBeVisible();
  await expect(page.locator('#spawn-tab .editor-status')).toContainText('Ln 1, Col 1');
  await spawnEditor.fill('missing = 4;');
  await expect(page.locator('#error')).toBeVisible();
  await expect(page.locator('#particle-count')).not.toHaveText('0');
  await capture('02-error-retains-preview');
  await spawnEditor.fill('size = 8;\nr = 2; g = 0.2; b = 0.8;');
  await expect(page.locator('#error')).toBeHidden();
  await expect(page.locator('#spawn-tab .editor-status')).toContainText('2 lines');
  await spawnEditor.press('End');
  const playbackBeforeTyping = await page.locator('#play').textContent();
  await spawnEditor.press('Space');
  await expect(page.locator('#play')).toHaveText(playbackBeforeTyping ?? '');
  await spawnEditor.press('ControlOrMeta+z');
  await spawnEditor.press('ControlOrMeta+Enter');
  await expect(page.locator('#diagnostic')).toContainText('LIVE');

  await page.locator('#seed').fill('9876');
  await page.locator('#seed').press('Tab');
  const downloadPromise = page.waitForEvent('download');
  await page.locator('#save').click();
  const saved = await downloadPromise;
  const savePath = fileURLToPath(new URL('roundtrip.joyfx', directory));
  await saved.saveAs(savePath);
  assert.equal(JSON.parse(await readFile(savePath, 'utf8')).version, 1);
  await page.locator('#file-input').setInputFiles(savePath);
  await expect(page.locator('#diagnostic')).toContainText('LIVE');
  await expect(page.locator('#seed')).toHaveValue('9876');

  await page.reload();
  await expect(page.locator('#asset-name')).toHaveValue('Solar Swirl');
  await capture('03-swirl-edited');

  await page.locator('#speed').selectOption('1');
  await page.locator('#examples').selectOption('2');
  await expect(page.locator('#asset-name')).toHaveValue('Prism Sprite Garden');
  await page.locator('[data-tab="emitter"]').click();
  await expect(page.locator('#diagnostic')).toContainText('LIVE');
  await expect(page.locator('#gpu-error')).toBeHidden();
  const materialAsset = await exportedAsset();
  const texturedIndex = materialAsset.emitters.findIndex(emitter => emitter.renderer === 'textured');
  assert.ok(texturedIndex >= 0);
  await page.locator('.emitter-item').nth(texturedIndex).click();
  await expect(page.locator('.texture-preview img')).toBeVisible();
  const pngSource = materialAsset.emitters.find(emitter => emitter.renderer === 'flipbook').texture.source;
  assert.notEqual(pngSource, materialAsset.emitters[texturedIndex].texture.source);
  await page.locator('#texture-input').setInputFiles({
    name: 'roundtrip-sprite.png', mimeType: 'image/png',
    buffer: Buffer.from(pngSource.split(',')[1], 'base64'),
  });
  await expect(page.locator('.texture-preview img')).toHaveAttribute('src', pngSource);
  await page.locator('#apply').click();
  await expect(page.locator('#error')).toBeHidden();
  assert.equal((await exportedAsset()).emitters[texturedIndex].texture.source, pngSource);
  await page.locator('#examples').selectOption('2');
  await page.locator('.emitter-item').nth(texturedIndex).click();
  await expect(page.locator('#error')).toBeHidden();
  await page.locator('[data-field="light-enabled"]').check();
  await expect(page.locator('[data-field="light-intensity"]')).toHaveValue('2');
  await expect(page.locator('[data-field="light-range"]')).toHaveValue('220');
  assert.deepEqual((await exportedAsset()).emitters[texturedIndex].light, { intensity: 2, range: 220 });
  await page.locator('[data-field="light-enabled"]').uncheck();
  assert.equal((await exportedAsset()).emitters[texturedIndex].light, undefined);
  await page.locator('#grid-toggle').uncheck();
  await page.locator('#preview-background').selectOption('cream');
  await expect(page.locator('#gpu-error')).toBeHidden();
  await page.locator('#preview-background').selectOption('midnight');
  await page.locator('#grid-toggle').check();
  await expect(page.locator('#render-range')).not.toHaveText('—');
  await page.locator('#performance-mode').selectOption('detailed');
  await expect(page.locator('#emitter-performance tr')).not.toHaveCount(0);
  await capture('07-materials');
  await page.locator('[data-tab="spawn"]').click();
  await expect(page.locator('#spawn-source .cm-lineNumbers')).toBeVisible();
  await page.locator('#spawn-source .cm-content').click();
  await capture('08-scripteditor');

  await page.locator('#examples').selectOption('3');
  await expect(page.locator('#asset-name')).toHaveValue('Orbiting Lights');
  await expect(page.locator('#diagnostic')).toContainText('LIVE');
  await page.locator('[data-tab="emitter"]').click();
  await page.locator('#play').click();
  await expect(page.locator('#play')).toContainText('Play');
  await page.locator('#timeline').evaluate(element => {
    const input = /** @type {HTMLInputElement} */ (element);
    input.value = '0.9';
    input.dispatchEvent(new Event('input', { bubbles: true }));
  });
  await expect(page.locator('#time')).toHaveText('0.90 s');
  await expect(page.locator('#preview-fps')).toHaveText('Paused');
  await expect(page.locator('#light-count')).not.toHaveText('0');
  await page.locator('#light-receiver').check();
  const lightCanvas = page.locator('#viewport');
  const litView = await lightCanvas.screenshot();
  await page.locator('#light-receiver').uncheck();
  assert.notDeepEqual(await lightCanvas.screenshot(), litView, 'receiver toggle changes the light receiving surface');
  await page.locator('#light-receiver').check();
  await page.locator('#grid-toggle').uncheck();
  assert.notDeepEqual(await lightCanvas.screenshot(), litView, 'grid toggle changes paused preview');
  await page.locator('#grid-toggle').check();
  await page.locator('#preview-background').selectOption('cream');
  assert.notDeepEqual(await lightCanvas.screenshot(), litView, 'background control changes paused preview');
  await page.locator('#preview-background').selectOption('midnight');
  await expect(page.locator('#emitter-performance tr')).toHaveCount(2);
  await capture('09-orbiting-lights-performance');

  const desktopPage = page;
  const context = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2 });
  page = await context.newPage();
  await page.goto(url);
  await expect(page.locator('#diagnostic')).toContainText('LIVE', { timeout: 20_000 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  await capture('04-mobile');
  await page.setViewportSize({ width: 360, height: 800 });
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  await capture('11-small-mobile');
  for(const name of ['power', 'wind', 'tint']) {
    await page.getByRole('button', { name: `Collapse ${name} values`, exact: true }).click();
  }
  assert.ok(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth));
  await capture('13-collapsed-mobile');
  await context.close();
  page = desktopPage;

  assert.deepEqual(errors, []);
  await page.setViewportSize({ width: 1280, height: 720 });
  const previewCapture = new URL('preview.png', directory);
  await page.screenshot({ path: fileURLToPath(previewCapture) });
  await copyFile(previewCapture, previewPath);
  await writeFile(new URL('results.json', directory), JSON.stringify({
    passed: true,
    captures,
    backend: 'webgl',
    built,
  }, null, 2));
  console.log(`PARTICLE LAB browser checks passed. Gallery: ${fileURLToPath(new URL('index.html', directory))}`);
} catch(error) {
  await capture('failure').catch(() => {});
  await writeFile(new URL('results.json', directory), JSON.stringify({
    passed: false,
    error: String(error),
    errors,
  }, null, 2));
  throw error;
} finally {
  const sections = captures.map(name => `<section><h2>${name}</h2><img src="${name}.png"></section>`).join('');
  await writeFile(new URL('index.html', directory), `<!doctype html><title>PARTICLE LAB validation</title><style>body{background:#101a2a;color:#f8edcf;font:16px sans-serif;padding:30px}img{width:100%;border:1px solid #ed6c39}section{margin:30px 0}</style><h1>PARTICLE LAB · Browser validation</h1>${sections}`);
  await browser?.close();
  await server.close();
}
