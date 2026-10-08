// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {readFile, writeFile, mkdir, rm} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {createServer} from 'vite';
import {chromium, expect} from '@playwright/test';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const output = new URL('../../../artifacts/level-editor/', import.meta.url);
const name = `editor-capture-${randomUUID()}`;
const playgroundAsset = new URL('../../../projects/vehicle-playground/assets/playground.joylevel', import.meta.url);
const originalPlayground = await readFile(playgroundAsset, 'utf8');
const file = new URL(`../../../projects/god-game/assets/${name}.joylevel`, import.meta.url);
await mkdir(output, {recursive: true});
const server = await createServer({configFile: `${root}scripts/development/vite-site.config.mjs`, server: {host: '127.0.0.1', port: 0, open: false}});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({executablePath: process.env.CAPTURE_EXECUTABLE, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', ...(process.env.CAPTURE_SINGLE_PROCESS ? ['--no-zygote', '--single-process'] : [])]});
  const page = await browser.newPage({viewport: {width: 1680, height: 1050}});
  page.setDefaultTimeout(30000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => Object.defineProperty(navigator, 'gpu', {value: undefined}));
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/editor/?project=god-game`);
  await expect(page.locator('#project')).toHaveValue('god-game');
  await page.locator('#play-mode').selectOption('docked');
  await page.locator('#new-asset').click();
  await page.locator('#new-asset-type').selectOption('joylevel');
  await page.locator('#new-asset-name').fill(name);
  await page.locator('#new-asset-form button[type="submit"]').click();
  const frame = page.frameLocator(`iframe[title="Edit assets/${name}.joylevel"]`);
  await expect(frame.locator('#viewport')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await frame.locator('#add-box').click();
  await expect(frame.locator('#entity-count')).toHaveText('1 ENTITIES');
  await frame.locator('#entity-name').fill('Authored obstacle');
  await frame.locator('#entity-name').press('Tab');
  await frame.locator('#position-0').fill('3');
  await frame.locator('#position-0').press('Tab');
  await frame.locator('#scale-0').fill('3');
  await frame.locator('#scale-0').press('Tab');
  await frame.locator('.advanced-source > summary').click();
  const components = JSON.parse(await frame.locator('#components').inputValue());
  components.custom = {nested: {author: 'preserved'}, values: [1, true, 'yes']};
  await frame.locator('#components').fill(JSON.stringify(components, null, 2));
  await frame.locator('#apply-components').click();
  await frame.locator('#duplicate').click();
  await expect(frame.locator('#entity-count')).toHaveText('2 ENTITIES');
  await frame.locator('#undo').click();
  await expect(frame.locator('#entity-count')).toHaveText('1 ENTITIES');
  await frame.locator('#redo').click();
  await expect(frame.locator('#entity-count')).toHaveText('2 ENTITIES');
  await frame.locator('#entities .entity-row').first().click();
  await frame.locator('#focus').click();
  const canvas = frame.locator('#viewport');
  const bounds = await canvas.boundingBox();
  assert.ok(bounds);
  assert.ok(bounds.width > 400 && bounds.height > 300, 'Viewport must have a usable authoring area.');
  const outliner = await frame.locator('.outliner').boundingBox();
  const inspector = await frame.locator('.inspector').boundingBox();
  assert.ok(outliner && inspector);
  assert.ok(outliner.x + outliner.width <= bounds.x + 1 && inspector.x >= bounds.x + bounds.width - 1, 'Outliner and inspector must flank the viewport.');
  // Focus centers this primitive; a viewport click must recover the same selection.
  await canvas.click({position: {x: bounds.width/2, y: bounds.height/2}});
  await expect(frame.locator('#entity-name')).toHaveValue('Authored obstacle');
  await frame.locator('#move-mode').click();
  await canvas.click({position: {x: bounds.width*0.35, y: bounds.height*0.65}});
  const placedX = Number(await frame.locator('#position-0').inputValue());
  assert.ok(Number.isInteger(placedX) && Math.abs(placedX) < 100);
  const validComponents = await frame.locator('#components').inputValue();
  await frame.locator('#components').fill('{in-progress component source');
  await frame.locator('#level-name').fill('Rename while editing components');
  await frame.locator('#level-name').press('Tab');
  await expect(frame.locator('#components')).toHaveValue('{in-progress component source');
  await frame.locator('#components').fill(validComponents);
  await frame.locator('#apply-components').click();
  await frame.locator('#position-2').fill('2');
  // Saving a still-focused field must commit it without relying on a blur event.
  await frame.locator('#position-2').press('ControlOrMeta+s');
  await expect(page.locator('#document-state')).toHaveText('SAVED TO PROJECT');
  const saved = JSON.parse(await readFile(file, 'utf8'));
  assert.equal(saved.entities.length, 2);
  assert.equal(saved.entities[0].transform.position[2], 2);
  assert.deepEqual(saved.entities[0].components.custom, components.custom);
  await page.screenshot({path: fileURLToPath(new URL('01-level-authoring.png', output)), timeout: 60000});
  const committedComponents = await frame.locator('#components').inputValue();
  await frame.locator('#components').fill('{unsaved invalid JSON');
  await expect(page.locator('#document-state')).toHaveText('SAVED TO PROJECT');
  const dialogPromise = page.waitForEvent('dialog');
  const reload = page.reload().catch(() => {});
  const dialog = await dialogPromise;
  assert.equal(dialog.type(), 'beforeunload');
  await dialog.dismiss();
  await reload;
  await expect(frame.locator('#components')).toHaveValue('{unsaved invalid JSON');
  await frame.locator('#components').fill(committedComponents);
  await frame.locator('#apply-components').click();


  await frame.locator('#entity-name').fill('Unsaved local edit');
  await frame.locator('#entity-name').press('Tab');
  await expect(page.locator('#save')).toBeEnabled();
  saved.name = 'External track';
  await writeFile(file, JSON.stringify(saved));
  await expect(page.locator('#conflict')).toBeVisible();
  page.once('dialog', dialog => dialog.accept());
  await page.locator('#reload-disk').click();
  await expect(frame.locator('#level-name')).toHaveValue('External track');
  await writeFile(file, '{broken level');
  const repair = page.locator(`textarea[aria-label="Repair assets/${name}.joylevel"]`);
  await expect(repair).toBeVisible();
  await repair.fill(JSON.stringify(saved));
  await page.locator('#save').click();
  await expect(repair).not.toBeVisible();
  await expect(frame.locator('#level-name')).toHaveValue('External track');
  await expect(page.locator('#document-state')).toHaveText('SAVED TO PROJECT');
  // Atomic workspace saves must stage content without Vite reloading a paused game.
  await page.locator('#project').selectOption('vehicle-playground');
  await expect(page.locator('#workspace')).toHaveClass(/preview-collapsed/);
  await page.setViewportSize({width: 1920, height: 1080});
  await page.locator('button[title="assets/playground.joylevel"]').dblclick();
  const levelTool = page.frameLocator('iframe[title="Edit assets/playground.joylevel"]');
  await expect(levelTool.locator('#viewport')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await page.locator('#run').click();
  const game = page.frameLocator('#game-stage iframe');
  await expect(game.locator('#world')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await expect(page.locator('#connection')).toHaveText('READY');
  const divider = await page.locator('#preview-divider').boundingBox();
  assert.ok(divider);
  await page.mouse.move(divider.x + divider.width/2, divider.y + 100);
  await page.mouse.down();
  await page.mouse.move(1350, divider.y + 100);
  await page.mouse.up();
  await page.locator('#pause').click();
  await expect(game.locator('#session-state')).toHaveText('PAUSED');
  const frozen = await game.locator('#world').getAttribute('data-time');
  await levelTool.locator('[data-id="kart-coral"]').click();
  await levelTool.locator('#entity-name').fill('Coral / editor integration');
  await levelTool.locator('#entity-name').press('Tab');
  await expect(page.locator('#save')).toBeEnabled();
  await page.locator('#save').click();
  await expect(page.locator('#document-state')).toHaveText('SAVED TO PROJECT');
  await expect(game.locator('#preview-status')).toBeVisible();
  assert.equal(await game.locator('#world').getAttribute('data-time'), frozen);
  await expect(game.locator('#driver')).toHaveText('CORAL / 01');
  await game.locator('#apply-preview').click();
  await expect(game.locator('#driver')).toHaveText('CORAL / EDITOR INTEGRATION');
  await page.locator('#pause').click();
  await expect(game.locator('#session-state')).toHaveText('PAUSED');
  await page.screenshot({path: fileURLToPath(new URL('02-level-and-vehicle.png', output)), timeout: 60000});
  await page.locator('#stop').click();
  await page.locator('#run').click();
  await expect(game.locator('#world')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await expect(game.locator('#driver')).toHaveText('CORAL / EDITOR INTEGRATION');
  console.log('PASS editor Save stages without reload; Apply and Stop/Run consume saved data');
  // A direct game tab at the shared dev entry still receives ordinary file HMR.
  const directGame = await browser.newPage();
  directGame.on('pageerror', error => errors.push(error.message));
  await directGame.addInitScript(() => Object.defineProperty(navigator, 'gpu', {value: undefined}));
  await directGame.goto(`http://127.0.0.1:${server.httpServer.address().port}/games/vehicle-playground/`);
  await expect(directGame.locator('#world')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  // Allow the explicitly bounded editor-write watcher window to settle.
  await directGame.waitForTimeout(1100);
  const externalLevel = JSON.parse(await readFile(playgroundAsset, 'utf8'));
  externalLevel.entities.find(entity => entity.id === 'kart-coral').name = 'Coral / direct edit';
  await writeFile(playgroundAsset, JSON.stringify(externalLevel, null, 2));
  await expect(directGame.locator('#preview-status')).toBeVisible();
  await directGame.locator('#apply-preview').click();
  await expect(directGame.locator('#driver')).toHaveText('CORAL / DIRECT EDIT');
  await directGame.close();
  assert.deepEqual(errors, []);
  await writeFile(new URL('index.html', output), '<!doctype html><title>Level editor capture</title><style>body{background:#151918;color:#eee;font:16px sans-serif;padding:24px}img{max-width:100%}</style><h1>Level authoring</h1><p>Create, select, transform, components, undo/redo, ground placement, save, conflict and malformed repair passed.</p><img src="01-level-authoring.png" alt="Level authoring workspace"><h2>Saved content in the vehicle preview</h2><p>Atomic save preserves paused simulation, explicit Apply restarts, and Stop/Run reads the latest level.</p><img src="02-level-and-vehicle.png" alt="Level authoring and vehicle preview">');
  console.log(`PASS level creation, selection, snapping, history, save, conflicts and repair. Gallery: ${fileURLToPath(output)}index.html`);
} finally {
  await browser?.close();
  await server.close();
  await writeFile(playgroundAsset, originalPlayground);
  await rm(file, {force: true});
  // Remove only recovery inodes for this process-specific capture document.
  const {readdir} = await import('node:fs/promises');
  const recovery = new URL('../../../projects/god-game/assets/.joy-editor-recovery/', import.meta.url);
  for(const entry of await readdir(recovery).catch(() => [])) {
    if(entry.startsWith(`${name}.joylevel.`)) {
      await rm(new URL(entry, recovery), {force: true});
    }
  }
}
