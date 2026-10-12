// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import {copyFile, mkdir, writeFile} from 'node:fs/promises';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {chromium, webkit, expect} from '@playwright/test';
import {createServer, preview} from 'vite';

const built = process.argv.includes('--built');
const previewArgument = process.argv.indexOf('--preview-directory');
const previewDirectory = previewArgument < 0 ? null : process.argv[previewArgument + 1];
const configFile = fileURLToPath(new URL('../vite.config.js', import.meta.url));
const directory = new URL(`../../../../artifacts/captures/${Date.now()}-form-destruction/`, import.meta.url);
const browserName = process.env.CAPTURE_BROWSER ?? 'chromium';
const backend = process.env.CAPTURE_BACKEND ?? 'webgl';
const captures = [];
const errors = [];
const routineLogs = [];
let browser;
let page;
let success = false;

await mkdir(directory, {recursive: true});
const server = built
  ? await preview({configFile, preview: {host: '127.0.0.1', port: 0}})
  : await createServer({configFile, server: {host: '127.0.0.1', port: 0}});

try {
  if(!built) {
    await server.listen();
  }
  const address = server.httpServer.address();
  const browserType = browserName === 'webkit' ? webkit : chromium;
  browser = await browserType.launch({
    executablePath: process.env.CAPTURE_EXECUTABLE,
    channel: process.env.CAPTURE_CHANNEL,
    headless: process.env.CAPTURE_HEADED !== '1',
    args: browserName === 'webkit' ? [] : ['--enable-unsafe-webgpu']
  });
  page = await browser.newPage({
    viewport: {width: 1440, height: 900},
    deviceScaleFactor: Number(process.env.CAPTURE_DPR ?? 1)
  });
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if(message.type() === 'error') {
      errors.push(message.text());
    }
    if(['log', 'info', 'debug'].includes(message.type())) {
      routineLogs.push(message.text());
    }
  });
  if(backend === 'fallback') {
    await page.addInitScript(() => Object.defineProperty(navigator, 'gpu', {value: undefined}));
  }
  const query = backend === 'fallback' ? '' : `?backend=${backend}`;
  await page.goto(`http://127.0.0.1:${address.port}/${query}`);
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true', {timeout: 30000});
  await expect(page.locator('#failure')).toBeHidden();
  const initial = await snapshot();
  assert.equal(initial.backend, backend === 'fallback' ? 'webgl' : backend);
  assert.ok(initial.activeChunks > 0);
  await capture('01-intact');

  // Two screen locations must hit different world-space chunks, independent of DPI.
  await page.mouse.click(580, 430);
  await page.evaluate(() => window.formDestruction.sample(2));
  const firstHit = await snapshot();
  assert.ok(firstHit.lastImpact.point[0] < -1, 'Left click must land on the left of the structure');
  assert.equal(firstHit.activeChunks, initial.activeChunks, 'Damage accumulates before chunks break');
  assert.equal(firstHit.impactDebug.markers, 1);
  const targetHealth = state => state.integrity.chunks.find(chunk => chunk.chunkId === firstHit.lastImpact.chunkId);
  assert.equal(targetHealth(firstHit).remainingHitPoints, targetHealth(initial).maximumHitPoints - firstHit.integrity.damagePerHit);
  await expect(page.locator('#integrity-target')).toContainText('5 hits left');
  await capture('02-first-impact');
  const beforeToggle = firstHit.integrity.chunks;
  await page.locator('#integrity-toggle').click();
  assert.equal((await snapshot()).integrity.enabled, false);
  assert.deepEqual((await snapshot()).integrity.chunks, beforeToggle, 'View changes must not alter simulation');
  await expect(page.locator('#integrity-panel')).toBeHidden();
  await capture('03-material-view');
  await page.locator('#integrity-toggle').click();
  for(let hit = 1; hit < targetHealth(initial).maximumHitPoints - 1; hit++) {
    await page.mouse.click(580, 430);
    await page.evaluate(() => window.formDestruction.sample(2));
  }
  assert.equal((await snapshot()).activeChunks, initial.activeChunks, 'An intact wall requires sustained impacts');
  await expect(page.locator('#integrity-target')).toContainText('1 hit left');
  await capture('04-critical-integrity');
  await page.mouse.click(580, 430);
  await expect.poll(async () => (await snapshot()).activeChunks).toBeLessThan(initial.activeChunks);
  const leftBreach = await snapshot();
  await page.mouse.click(920, 430);
  const rightHit = await snapshot();
  assert.ok(rightHit.lastImpact.point[0] > 1, 'Right click must land on the right of the structure');
  assert.notEqual(firstHit.lastImpact.chunkId, rightHit.lastImpact.chunkId);
  for(let hit = 1; hit < targetHealth(initial).maximumHitPoints; hit++) {
    await page.mouse.click(920, 430);
    await page.evaluate(() => window.formDestruction.sample(2));
  }
  await expect.poll(async () => (await snapshot()).activeChunks).toBeLessThan(leftBreach.activeChunks);
  await page.evaluate(() => window.formDestruction.sample(30));
  await capture('05-impacts');
  const supported = await snapshot();
  assert.ok(supported.support.detachedChunks.length > 0, 'Healthy unsupported chunks must detach when the opening becomes too wide');
  assert.ok(supported.integrity.chunks.some(chunk => !chunk.active && chunk.remainingHitPoints > 0));
  assert.equal(supported.debrisPhysics.staticChunks, supported.activeChunks);
  await page.locator('#support-toggle').click();
  assert.equal((await snapshot()).support.enabled, true);
  assert.ok((await snapshot()).support.chunks.some(chunk => chunk.supported && chunk.lateralDistance > 0));
  await capture('05-support');
  await page.locator('#integrity-toggle').click();

  // Empty space and orbit gestures must not accidentally fire at a center reticle.
  const damaged = await snapshot();
  await page.mouse.click(1350, 100);
  assert.equal((await snapshot()).lastImpact, null);
  await page.mouse.move(700, 450);
  await page.mouse.down();
  await page.mouse.move(850, 510, {steps: 30});
  await page.mouse.up();
  assert.equal((await snapshot()).activeChunks, damaged.activeChunks);
  await capture('06-orbit');
  await expect.poll(async () => {
    const pieces = (await snapshot()).debrisPhysics.pieces;
    return pieces.filter(piece => piece.position[1] < 1 && Math.hypot(...piece.velocity) < 0.3).length;
  }, {timeout: 6000}).toBeGreaterThan(0);
  const settled = await snapshot();
  assert.ok(settled.debrisPhysics.pieces.every(piece => piece.position[1] >= settled.debrisPhysics.groundTop));
  await capture('07-ground-collision');
  // Cross the former eight-second expiry using actual animation time, without advancing the solver artificially.
  await page.waitForTimeout(9000);
  assert.equal((await snapshot()).debris, settled.debris, 'Every released chunk persists until reset');
  await capture('08-retained-debris');
  await page.locator('#config').click();
  await expect(page.locator('#config-panel')).toBeVisible();
  await page.locator('#impact-debug').uncheck();
  assert.deepEqual((await snapshot()).impactDebug, {enabled: false, markers: 0});
  await page.locator('#impact-debug').check();
  await page.locator('#close-config').click();
  await page.locator('#reset').click();
  await expect.poll(async () => (await snapshot()).activeChunks).toBe(initial.activeChunks);
  assert.equal((await snapshot()).debris, 0);
  assert.equal((await snapshot()).impactDebug.markers, 0);
  assert.ok((await snapshot()).integrity.chunks.every(chunk => chunk.remainingHitPoints === chunk.maximumHitPoints));
  assert.ok((await snapshot()).support.chunks.every(chunk => chunk.supported));
  assert.deepEqual((await snapshot()).support.detachedChunks, []);
  assert.equal((await snapshot()).debrisPhysics.staticChunks, initial.activeChunks);

  // Reproduce a broad floating slab by cutting the lower front wall while leaving the top untouched.
  await page.reload();
  await expect(page.locator('body')).toHaveAttribute('data-ready', 'true', {timeout: 30000});
  await page.locator('#config').click();
  await page.locator('#radius').focus();
  await page.keyboard.press('End');
  await page.locator('#damage').focus();
  await page.keyboard.press('End');
  await page.locator('#close-config').click();
  await page.locator('#support-toggle').click();
  for(const x of [310, 510, 710, 910, 1110, 1200]) {
    for(let hit = 0; hit < 2; hit++) {
      await page.mouse.click(x, 550);
      await page.evaluate(() => window.formDestruction.sample(2));
    }
  }
  const undercut = await snapshot();
  const roofId = 'front:6:5';
  const roofHealth = undercut.integrity.chunks.find(chunk => chunk.chunkId === roofId);
  assert.equal(roofHealth.remainingHitPoints, roofHealth.maximumHitPoints, 'The upper slab was never directly hit');
  assert.equal(roofHealth.active, false, 'The upper slab cannot float after losing its supports');
  assert.ok(undercut.support.detachedChunks.includes(roofId));
  await expect.poll(async () => (await snapshot()).debrisPhysics.pieces.find(piece => piece.chunkId === roofId).position[1], {timeout: 6000}).toBeLessThan(4);
  await capture('09-support-collapse');
  await page.locator('#reset').click();
  assert.equal((await snapshot()).debris, 0);

  await page.setViewportSize({width: 800, height: 600});
  await capture('10-resize');
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  assert.deepEqual(errors, []);
  if(built) {
    assert.deepEqual(routineLogs, [], 'Production emits no routine logs');
  } else {
    assert.ok(routineLogs.some(message => message.startsWith('FORM destruction ready:')));
  }
  if(built && previewDirectory) {
    await mkdir(previewDirectory, {recursive: true});
    await copyFile(new URL('09-support-collapse.png', directory), path.join(previewDirectory, 'example-form-destruction.png'));
  }
  success = true;
} finally {
  if(page && !success) {
    await capture('failure').catch(() => {});
    console.error('Browser errors:', errors);
  }
  await writeFile(new URL('index.html', directory), `<!doctype html><html lang="en"><meta charset="utf-8"><title>FORM destruction capture</title><style>body{background:#0a0d11;color:#eee;font:16px system-ui;max-width:1400px;margin:30px auto}img{width:100%}figure{margin:30px 0}</style><h1>FORM Destruction · ${built ? 'Production' : 'Development'}</h1><p>${success ? 'Passed' : 'Failed'} · ${browserName} / ${backend} · ${new Date().toISOString()}</p>${captures.map(name => `<figure><figcaption>${name}</figcaption><img src="${name}.png" alt="${name}"></figure>`).join('')}</html>`);
  await browser?.close();
  await server.close();
  console.log(`Gallery: ${fileURLToPath(new URL('index.html', directory))}`);
}

async function snapshot() {
  return page.evaluate(() => window.formDestruction.snapshot());
}

async function capture(name) {
  await page.screenshot({path: fileURLToPath(new URL(`${name}.png`, directory)), fullPage: true});
  captures.push(name);
}
