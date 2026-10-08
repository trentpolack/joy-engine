// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {randomUUID} from 'node:crypto';
import {readFile, writeFile, mkdir, rm, readdir} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {createServer} from 'vite';
import {chromium, expect} from '@playwright/test';
import {createAssetText} from '../site/src/workspace/asset-types.ts';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const output = new URL('../../../artifacts/captures/editor-usability/', import.meta.url);
const assets = new URL('../../../projects/vehicle-playground/assets/', import.meta.url);
const name = `usability-${randomUUID()}`;
const paths = Object.fromEntries(['joylevel', 'joyobject', 'form', 'joyfx'].map(extension => [extension, `${name}.${extension}`]));
await mkdir(output, {recursive: true});
await writeFile(new URL(paths.joylevel, assets), createAssetText(paths.joylevel));
await writeFile(new URL(paths.form, assets), 'project meta(name="Beacon")\nparam width = 1 meta(min=0.25,max=4,step=0.25)\ncolor(0.93,0.42,0.22)\nbox(0,0.5,0,width,1,width)\n');
await writeFile(new URL(paths.joyfx, assets), createAssetText(paths.joyfx));
const server = await createServer({configFile: `${root}scripts/development/vite-site.config.mjs`, server: {host: '127.0.0.1', port: 0, open: false}});
let browser;
let page;
try {
  await server.listen();
  browser = await chromium.launch({executablePath: process.env.CAPTURE_EXECUTABLE, args: ['--no-sandbox', '--enable-unsafe-swiftshader']});
  page = await browser.newPage({viewport: {width: 1720, height: 1100}});
  page.setDefaultTimeout(15000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => Object.defineProperty(navigator, 'gpu', {value: undefined}));
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/editor/?project=vehicle-playground`);
  await page.locator('#play-mode').selectOption('docked');
  await page.locator(`button[title="assets/${paths.joylevel}"]`).dblclick();
  const frame = page.frameLocator(`iframe[title="Edit assets/${paths.joylevel}"]`);
  await expect(frame.locator('#viewport')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await frame.locator('#add-box').click();
  await frame.locator('#entity-name').fill('Beacon');
  await frame.locator('#entity-name').press('Tab');
  await frame.getByRole('button', {name: 'Remove render', exact: true}).click();
  await frame.locator('#component-kind').selectOption('mesh');
  await frame.locator('#add-component').click();
  await frame.getByLabel('render.asset', {exact: true}).selectOption(`assets/${paths.form}`);
  await frame.locator('#viewport').focus();
  const width = frame.getByLabel('render.parameters.width', {exact: true});
  await expect(width).toHaveValue('1');
  await width.fill('2');
  await width.press('Tab');
  await frame.locator('#undo').click();
  await expect(width).toHaveValue('1');
  await frame.locator('#redo').click();
  await expect(width).toHaveValue('2');
  console.log('PASS compiler parameter defaults and property undo/redo');
  await frame.locator('#component-kind').selectOption('effect');
  await frame.locator('#add-component').click();
  await frame.getByLabel('effect.asset', {exact: true}).selectOption(`assets/${paths.joyfx}`);
  await frame.locator('#viewport').focus();
  await frame.locator('#focus').click();
  page.once('dialog', dialog => dialog.accept(paths.joyobject));
  await frame.locator('#save-object').click();
  await expect(frame.locator('#instance-source')).toHaveText(`assets/${paths.joyobject}`);
  await frame.locator('#duplicate').click();
  await expect(frame.locator('#entity-count')).toHaveText('2 ENTITIES');
  await frame.locator('#instance-picker').selectOption(`assets/${paths.joyobject}`);
  await frame.locator('#add-instance').click();
  await expect(frame.locator('#entity-count')).toHaveText('3 ENTITIES');
  await width.fill('3');
  await width.press('Tab');
  await frame.locator('#reset-overrides').click();
  await expect(width).toHaveValue('2');
  await frame.locator('#undo').click();
  await expect(width).toHaveValue('3');
  await frame.locator('#detach-object').click();
  await expect(frame.locator('#instance-info')).toBeHidden();
  await frame.locator('#undo').click();
  await expect(frame.locator('#instance-info')).toBeVisible();
  console.log('PASS object creation, linking, duplication, instance overrides, reset and detach history');
  await width.fill('');
  await frame.getByRole('button', {name: 'Remove physics', exact: true}).click();
  await expect(width).toHaveValue('');
  await expect(frame.getByLabel('physics.mass', {exact: true})).toBeVisible();
  await frame.locator('#entities .entity-row').first().click();
  await expect(width).toHaveValue('');
  // Save may be disabled while an invalid draft blocks the first passive capture.
  // The explicit shortcut still exercises the save-capture guard deterministically.
  await width.press('ControlOrMeta+s');
  await expect(width).toHaveValue('');
  assert.equal(await width.evaluate(input => input.checkValidity()), false);
  await expect(page.locator('#status')).toContainText('before saving or closing');
  assert.equal((JSON.parse(await readFile(new URL(paths.joylevel, assets), 'utf8'))).entities.length, 0);
  await width.fill('3');
  await width.press('Tab');
  await page.locator('#save').click();
  await expect(page.locator('#document-state')).toHaveText('SAVED TO PROJECT');
  const saved = JSON.parse(await readFile(new URL(paths.joylevel, assets), 'utf8'));
  assert.equal(saved.entities.length, 3);
  assert.ok(saved.entities.every(entity => entity.template === `assets/${paths.joyobject}`));
  assert.equal(saved.entities[2].overrides.components.render.parameters.width, 3);
  await frame.locator('#edit-object').click();
  const objectFrame = page.frameLocator(`iframe[title="Edit assets/${paths.joyobject}"]`);
  await expect(objectFrame.locator('#authoring-kind')).toHaveText('OBJECT');
  await expect(page.locator('#return-owner')).toBeVisible();
  const sourceWidth = objectFrame.getByLabel('render.parameters.width', {exact: true});
  await sourceWidth.fill('1.5');
  await sourceWidth.press('Tab');
  await page.locator('#save').click();
  await expect(page.locator('#document-state')).toHaveText('SAVED TO PROJECT');
  await page.locator('#return-owner').click();
  await expect(width).toHaveValue('3');
  await frame.locator('#entities .entity-row').first().click();
  await expect(width).toHaveValue('1.5');
  await frame.locator('[data-path="render"] .property-row').filter({has: page.locator('select')}).getByRole('button', {name: 'EDIT SOURCE ↗'}).click();
  await expect(page.locator(`iframe[title="Edit assets/${paths.form}"]`)).toHaveAttribute('src', /form-lab/);
  await page.locator('#return-owner').click();
  await frame.locator('[data-path="effect"] .property-row').filter({has: page.locator('select')}).getByRole('button', {name: 'EDIT SOURCE ↗'}).click();
  await expect(page.locator(`iframe[title="Edit assets/${paths.joyfx}"]`)).toHaveAttribute('src', /particle-lab/);
  await page.locator('#return-owner').click();
  await frame.locator('#focus').click();
  await frame.locator('#viewport').hover();
  await page.mouse.wheel(0, 550);
  await frame.locator('.inspector').evaluate(node => { node.scrollTop = 0; });
  await page.screenshot({path: fileURLToPath(new URL('objects.png', output))});
  await page.setViewportSize({width: 1280, height: 900});
  await page.screenshot({path: fileURLToPath(new URL('compact.png', output))});
  assert.deepEqual(errors, []);
  await writeFile(new URL('index.html', output), '<!doctype html><title>Editor usability</title><style>body{background:#19273b;color:#eee;font:16px sans-serif;padding:24px}img{max-width:100%}</style><h1>Reusable object authoring</h1><p>FORM and JOYFX composition, declared parameter controls, linked instances, overrides, undo/redo, invalid draft preservation, source propagation and editor navigation passed in Chromium/WebGL2.</p><img src="objects.png" alt="Reusable objects"><img src="compact.png" alt="Compact editor layout">');
  console.log(`PASS save blocking, source propagation and specialist navigation. Gallery: ${fileURLToPath(output)}index.html`);
} catch(error) {
  await page?.screenshot({path: fileURLToPath(new URL('failure.png', output))}).catch(() => {});
  throw error;
} finally {
  await browser?.close();
  await server.close();
  for(const path of Object.values(paths)) {
    await rm(new URL(path, assets), {force: true});
  }
  const recovery = new URL('.joy-editor-recovery/', assets);
  for(const entry of await readdir(recovery).catch(() => [])) {
    if(entry.startsWith(name)) {
      await rm(new URL(entry, recovery), {force: true});
    }
  }
}
