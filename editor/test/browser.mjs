// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {createServer} from 'vite';
import {chromium, expect} from '@playwright/test';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const output = new URL('../../../artifacts/joy-editor/', import.meta.url);
await mkdir(output, {recursive: true});
const server = await createServer({configFile: `${root}scripts/development/vite-site.config.mjs`, server: {host: '127.0.0.1', port: 0, open: false}});
const assets = new Map();
for(const name of ['form/villager.form', 'form/world.form', 'joyfx/fire.joyfx']) {
  const file = new URL(`../../../projects/god-game/assets/${name}`, import.meta.url);
  assets.set(name, {file, text: await readFile(file, 'utf8')});
}
let browser;
const captures = [];
try {
  await server.listen();
  const url = `http://127.0.0.1:${server.httpServer.address().port}/editor/`;
  browser = await chromium.launch({executablePath: process.env.CAPTURE_EXECUTABLE, args: ['--no-sandbox', '--use-angle=swiftshader', '--enable-unsafe-swiftshader', ...(process.env.CAPTURE_SINGLE_PROCESS ? ['--no-zygote', '--single-process'] : [])]});
  const page = await browser.newPage({viewport: {width: 1680, height: 1050}});
  page.setDefaultTimeout(30000);
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.addInitScript(() => Object.defineProperty(navigator, 'gpu', {value: undefined}));
  const capture = async name => {
    await page.screenshot({path: fileURLToPath(new URL(`${name}.png`, output)), timeout: 60000});
    captures.push(name);
  };
  const open = async path => {
    await page.locator(`button[title="assets/${path}"]`).dblclick();
    return page.frameLocator(`iframe[title="Edit assets/${path}"]`);
  };
  const replaceSource = async (frame, text) => {
    const source = frame.locator('.cm-content').first();
    await source.click();
    await source.press('ControlOrMeta+a');
    await page.keyboard.insertText(text);
  };
  await page.goto(url);
  await expect(page.locator('#stop')).toBeDisabled();
  await page.locator('#play-mode').selectOption('docked');
  const form = await open('form/villager.form');
  await expect(form.locator('#diagnostic-text')).toContainText('Built successfully', {timeout: 60000});
  await expect(form.locator('#gpu-error')).toBeHidden();
  await page.locator('#run').click();
  const game = page.frameLocator('#game-stage iframe');
  await expect(game.locator('#world')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await expect(page.locator('#connection')).toHaveText('CONNECTED');
  // Pause reports the authoritative simulation tick before the HUD's throttled refresh.
  const [pausedTicks] = await Promise.all([
    page.evaluate(() => new Promise(resolve => {
      const receive = event => {
        const preview = document.querySelector('#game-stage iframe');
        if(event.source === preview?.contentWindow && event.origin === location.origin && event.data?.channel === 'joy-editor-runtime' && event.data.paused) {
          window.removeEventListener('message', receive);
          resolve(event.data.ticks);
        }
      };
      window.addEventListener('message', receive);
    })),
    page.locator('#pause').click()
  ]);
  await expect(page.locator('#pause')).toHaveText('Resume');
  const ticks = String(pausedTicks);
  await expect(game.locator('#world')).toHaveAttribute('data-ticks', ticks);
  const source = assets.get('form/villager.form');
  const modified = source.text.replace('0.25, 0.55, 0.68', '0.8, 0.25, 0.15');
  await replaceSource(form, modified);
  await expect(page.locator('#save')).toBeEnabled();
  await page.locator('#save').click();
  await expect(page.locator('#document-state')).toHaveText('SAVED TO PROJECT');
  assert.equal(await readFile(source.file, 'utf8'), modified);
  await expect(page.locator('#runtime-note')).toContainText('villager updated');
  assert.equal(await game.locator('#world').getAttribute('data-ticks'), ticks);
  await capture('01-form-and-game');
  console.log('PASS FORM save updates game without resetting simulation');

  const particle = await open('joyfx/fire.joyfx');
  await expect(particle.locator('#emitter-list')).not.toBeEmpty({timeout: 60000});
  await expect(particle.locator('#asset-name')).toHaveValue(JSON.parse(assets.get('joyfx/fire.joyfx').text).name);
  await particle.locator('#frame-effect').click();
  await capture('02-particle-and-game');
  await open('form/villager.form');
  await expect(form.locator('.cm-content')).toContainText('0.8, 0.25, 0.15');
  console.log('PASS persistent FORM and particle document tabs');

  await replaceSource(form, modified + '\n// local unsaved edit\n');
  await expect(page.locator('#save')).toBeEnabled();
  await writeFile(source.file, source.text + '\n// external edit\n');
  await expect(page.locator('#conflict')).toBeVisible();
  await capture('03-disk-conflict');
  page.once('dialog', dialog => dialog.accept());
  await page.locator('#reload-disk').click();
  await expect(page.locator('#conflict')).toBeHidden();
  await expect(form.locator('.cm-content')).toContainText('external edit');
  await writeFile(source.file, source.text);
  await expect(form.locator('.cm-content')).not.toContainText('external edit');
  console.log('PASS external refresh and dirty conflict resolution');

  const world = await open('form/world.form');
  const worldSource = assets.get('form/world.form');
  await expect(world.locator('.cm-content')).not.toBeEmpty();
  await replaceSource(world, worldSource.text + '\n// workspace world edit\n');
  await expect(page.locator('#save')).toBeEnabled();
  await page.locator('#save').click();
  await expect(page.locator('#regenerate')).toBeVisible();
  assert.equal(await game.locator('#world').getAttribute('data-ticks'), ticks);
  await page.locator('#regenerate').click();
  await expect(page.locator('#regenerate')).toBeHidden();
  await expect(page.locator('#runtime-note')).toContainText('World regenerated');
  console.log('PASS world changes wait for explicit regeneration');

  await open('joyfx/fire.joyfx');
  const effectSource = assets.get('joyfx/fire.joyfx');
  const invalid = JSON.parse(effectSource.text);
  invalid.emitters[0].spawn = 'invalid('; 
  await writeFile(effectSource.file, JSON.stringify(invalid));
  const repair = page.getByRole('textbox', {name: 'Repair assets/joyfx/fire.joyfx'});
  await expect(repair).toBeVisible();
  await repair.fill(effectSource.text);
  await page.locator('#save').click();
  await expect(repair).toHaveCount(0);
  await expect(page.locator('#document-state')).toHaveText('SAVED TO PROJECT');
  console.log('PASS invalid particle source can be repaired and saved');

  await open('form/villager.form');
  await page.reload();
  await expect(page.locator('#tabs [aria-selected="true"]')).toContainText('villager.form');
  await expect(page.locator('[role="tab"][aria-selected="true"]')).toHaveText('villager.form');
  await page.setViewportSize({width: 1000, height: 800});
  await page.locator('#run').click();
  await expect(page.locator('#preview-panel')).toBeVisible();
  await expect(page.frameLocator('#game-stage iframe').locator('#world')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await expect(page.frameLocator('iframe[title="Edit assets/form/villager.form"]').locator('#diagnostic-text')).toContainText('Built successfully', {timeout: 60000});
  await capture('04-compact-preview');
  await page.locator('#toggle-preview').click();
  await expect(page.locator('#preview-panel')).toBeHidden();
  await page.locator('#preview-mode').click();
  await expect(page.locator('#preview-panel')).toBeVisible();
  console.log('PASS restored active tab and compact preview drawer');
  await page.setViewportSize({width: 1680, height: 1050});
  await replaceSource(form, source.text + '\n// unsaved while stopping preview\n');
  await expect(page.locator('#save')).toBeEnabled();
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Resume');
  await page.locator('#stop').click();
  await expect(page.locator('#game-stage iframe')).toHaveCount(0);
  await expect(page.locator('#preview-panel')).toBeHidden();
  await expect(page.locator('#connection')).toHaveText('PREVIEW STOPPED');
  await expect(page.locator('#pause')).toBeDisabled();
  await expect(page.locator('#pause')).toHaveText('Pause');
  await expect(page.locator('#restart')).toBeDisabled();
  await expect(page.locator('#stop')).toBeDisabled();
  await expect(page.locator('#regenerate')).toBeHidden();
  await expect(page.locator('#save')).toBeEnabled();
  await expect(form.locator('.cm-content')).toContainText('unsaved while stopping preview');
  await capture('05-stopped-preview');
  await page.locator('#run').click();
  await expect(game.locator('#world')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await expect(page.locator('#connection')).toHaveText('CONNECTED');
  await expect(page.locator('#stop')).toBeEnabled();
  await expect(page.locator('#save')).toBeEnabled();
  await page.locator('#stop').click();
  await expect(page.locator('#game-stage iframe')).toHaveCount(0);
  console.log('PASS stopping paused/running previews unloads the game and preserves unsaved documents');
  assert.deepEqual(errors, []);
  console.log('PASS no uncaught browser errors');
} finally {
  await browser?.close();
  for(const {file, text} of assets.values()) {
    await writeFile(file, text);
  }
  await server.close();
  await writeFile(new URL('index.html', output), `<!doctype html><meta charset="utf-8"><title>Joy Editor review</title><style>body{background:#101722;color:#e8edf4;font:16px system-ui;max-width:1500px;margin:40px auto}img{width:100%;border:1px solid #34445a}h1{color:#ed6c39}figure{margin:30px 0}figcaption{margin:12px 0}</style><h1>Joy Editor · Project workspace</h1><p>God Game content, persistent document tabs, direct project saves, conflict handling and connected preview.</p>${captures.map(name => `<figure><figcaption>${name}</figcaption><img src="${name}.png" alt="${name}"></figure>`).join('')}`);
}
