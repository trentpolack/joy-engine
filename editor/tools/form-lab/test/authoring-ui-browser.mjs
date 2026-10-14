// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import {mkdir, readFile, writeFile, rm} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {chromium, webkit, expect} from '@playwright/test';
import {createServer} from 'vite';
import {FORM_LIBRARY_FIXTURES} from './form-library-fixtures.js';

const directory = fileURLToPath(new URL(`../../../../../artifacts/captures/${Date.now()}-form-authoring/`, import.meta.url));
await mkdir(directory, {recursive: true});
const server = await createServer({
  configFile: fileURLToPath(new URL('../vite.config.mjs', import.meta.url)),
  server: {host: '127.0.0.1', port: 0}
});
await server.listen();
const browserName = process.env.CAPTURE_BROWSER ?? 'chromium';
const browser = await (browserName === 'webkit' ? webkit : chromium).launch({
  ...(browserName === 'webkit' ? {} : {channel: process.env.CAPTURE_CHANNEL}),
  args: browserName === 'webkit' ? [] : ['--use-angle=swiftshader', '--enable-unsafe-swiftshader']
});
const page = await browser.newPage({viewport: {width: 1440, height: 900}});
const errors = [];
const captures = [];
let libraryFixture = null;
page.on('pageerror', error => errors.push(error.message));
page.on('dialog', dialog => dialog.accept());
await page.addInitScript(() => Object.defineProperty(window, 'showSaveFilePicker', {value: undefined, writable: true, configurable: true}));

async function capture(name) {
  await page.screenshot({path: `${directory}/${name}.png`, fullPage: true});
  captures.push(name);
}

async function downloadFromDialog(format) {
  await page.locator('#artifact-format').selectOption(format);
  if(await page.locator('#artifact-destination').isVisible()) {
    await page.locator('#artifact-destination').selectOption('download');
  }
  const pending = page.waitForEvent('download');
  await page.locator('#artifact-write').click();
  const download = await pending;
  return {name: download.suggestedFilename(), bytes: await readFile(await download.path())};
}

try {
  await page.goto(process.env.CAPTURE_URL ?? `http://127.0.0.1:${server.httpServer.address().port}/?backend=webgl`);
  await expect(page.locator('#diagnostic')).toContainText('Built successfully', {timeout: 20000});
  await expect(page.locator('.app-footer')).toHaveCount(0);
  await expect(page.locator('.app-header select')).toHaveCount(0);
  await expect(page.locator('#examples optgroup[label="assets/examples"]')).toHaveCount(1);
  await expect(page.locator('#examples optgroup[label="assets/form/structures"]')).toHaveCount(1);
  assert.deepEqual(await page.locator('#examples option').evaluateAll(options => options.map(option => option.value).filter(Boolean)), FORM_LIBRARY_FIXTURES.map(asset => asset.path));
  await page.locator('#examples').selectOption('assets/form/structures/cottage.formlab');
  await expect(page.getByRole('spinbutton', {name: 'height value', exact: true})).toHaveValue('3.5');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state', 'generated');
  await capture('00-library-document');
  await page.locator('#examples').selectOption('assets/examples/wave-study.form');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state', 'generated');
  if(process.env.CAPTURE_LIBRARY_WATCH) {
    const folder = `library-capture-${process.pid}`;
    libraryFixture = new URL(`../../../../assets/examples/${folder}/`, import.meta.url);
    await mkdir(libraryFixture);
    const source = '# Copyright (c) 2026 Trent Polack. All Rights Reserved.\n# Licensed under the MIT License.\nparam width = 1 meta(min=0,max=10,step=1)\npoint(width,0,0)\n';
    await writeFile(new URL('new.form', libraryFixture), source, {flag: 'wx'});
    await writeFile(new URL('stored.formlab', libraryFixture), JSON.stringify({version: 1, name: 'Stored asset', source, overrides: {width: 7}, variants: [{name: 'Wide', overrides: {width: 9}}]}), {flag: 'wx'});
    await expect(page.locator(`#examples option[value="assets/examples/${folder}/new.form"]`)).toHaveCount(1, {timeout: 20000});
    await expect(page.locator(`#examples option[value="assets/examples/${folder}/stored.formlab"]`)).toHaveCount(1);
    await page.locator('#examples').selectOption(`assets/examples/${folder}/stored.formlab`);
    await expect(page.getByRole('spinbutton', {name: 'width value', exact: true})).toHaveValue('7');
    await page.locator('#variants-tab').click();
    await expect(page.locator('#variant-select')).toContainText('Wide');
    await page.locator('#parameters-tab').click();
    await rm(libraryFixture, {recursive: true});
    libraryFixture = null;
    await expect(page.locator(`#examples option[value="assets/examples/${folder}/stored.formlab"]`)).toHaveCount(0, {timeout: 20000});
    await page.locator('#examples').selectOption('assets/examples/wave-study.form');
    await expect(page.locator('#preview-status')).toHaveAttribute('data-state', 'generated');
  }
  await capture('01-workspace');
  const toolsBounds = await page.locator('.joy-editor-switcher summary').boundingBox();
  const viewBounds = await page.locator('#panel-visibility summary').boundingBox();
  assert.ok(viewBounds.x >= toolsBounds.x + toolsBounds.width && viewBounds.x < toolsBounds.x + toolsBounds.width + 30, 'View belongs beside Tools.');
  for(const selector of ['#save', '#export', '#help', '#panel-visibility summary', '.joy-editor-switcher summary']) {
    await page.mouse.move(1, 1);
    const normal = await page.locator(selector).evaluate(element => getComputedStyle(element).borderTopColor);
    await page.locator(selector).hover();
    assert.notEqual(await page.locator(selector).evaluate(element => getComputedStyle(element).borderTopColor), normal, `${selector} retains a hover state.`);
  }
  const design = page.locator('.joy-design-toggle');
  await expect(design).toBeVisible();
  await page.mouse.move(1, 1);
  const designBackground = await design.evaluate(element => getComputedStyle(element).backgroundColor);
  assert.notEqual(designBackground, await page.locator('#save').evaluate(element => getComputedStyle(element).backgroundColor), 'Design retains its primary styling.');
  await design.hover();
  assert.notEqual(await design.evaluate(element => getComputedStyle(element).backgroundColor), designBackground);
  await capture('01-header-hover');
  await design.click();
  await page.keyboard.press('Escape');
  await expect(design).toHaveAttribute('aria-expanded', 'false');
  assert.equal(await design.evaluate(element => getComputedStyle(element).backgroundColor), designBackground, 'Escape clears hover even while the pointer stays over Design.');
  await expect(page.locator('.joy-design-mode-label')).toBeHidden();
  await page.mouse.move(1, 1);
  await design.hover();
  assert.notEqual(await design.evaluate(element => getComputedStyle(element).backgroundColor), designBackground, 'Design hover returns on the next pointer entry.');
  await page.mouse.move(1, 1);
  await page.locator('.joy-editor-switcher summary').click();
  const switcherBounds = await page.locator('.joy-editor-switcher summary').boundingBox();
  const menuBounds = await page.locator('.joy-editor-menu').boundingBox();
  assert.ok(Math.abs(menuBounds.x - switcherBounds.x) <= 1, 'Tool menu opens to the right of its left-aligned button.');
  assert.ok(menuBounds.x >= 0 && menuBounds.x + menuBounds.width <= 1440, 'Tool menu stays inside the viewport.');
  await capture('01-tools-menu');
  await page.keyboard.press('Escape');

  // Raw source must survive exactly, while a project save must retain inspector-only data.
  const visibleSource = await page.locator('.cm-content').textContent();
  await page.getByLabel('tint R value', {exact: true}).fill('0.25');
  await page.getByLabel('tint R value', {exact: true}).press('Tab');
  const source = await page.evaluate(() => JSON.parse(localStorage.getItem('joy-form-lab-v1')).source);
  await page.locator('#save').click();
  await expect(page.locator('#artifact-dialog')).toBeVisible();
  await expect(page.locator('#artifact-format')).toHaveValue('form');
  await expect(page.locator('#artifact-description')).toContainText('overrides');
  await expect(page.locator('#artifact-destination')).toBeHidden();
  await expect(page.locator('#artifact-location-note')).toBeHidden();
  await capture('02-save-script');
  const raw = await downloadFromDialog('form');
  assert.ok(raw.name.endsWith('.form'));
  assert.equal(raw.bytes.toString(), source);
  await page.locator('#save').click();
  const project = await downloadFromDialog('formlab');
  const document = JSON.parse(project.bytes.toString());
  assert.equal(document.source, source);
  assert.equal(document.overrides.tint[0], 0.25);
  assert.ok(project.name.endsWith('.formlab'));

  await page.locator('#export').click();
  await expect(page.locator('#artifact-dialog')).toBeVisible();
  await expect(page.locator('#artifact-format')).toHaveValue('glb');
  await capture('03-export');
  const mesh = await downloadFromDialog('glb');
  assert.equal(mesh.bytes.subarray(0, 4).toString(), 'glTF');
  await page.locator('#export').click();
  const obj = await downloadFromDialog('obj');
  assert.match(obj.bytes.toString(), /^v /m);
  await page.locator('#export').click();
  await page.keyboard.press('Escape');
  await expect(page.locator('#artifact-dialog')).toBeHidden();
  await expect(page.locator('#export')).toBeFocused();

  // Visibility controls remain outside every panel and survive reload without replacing source.
  await page.locator('#panel-visibility summary').click();
  for(const [name, selector] of [['script', '.editor-panel'], ['preview', '.preview-panel'], ['inspector', '.inspector']]) {
    await page.getByRole('button', {name: `Hide ${name} panel`, exact: true}).click();
    await expect(page.locator(selector)).toBeHidden();
    await page.getByRole('button', {name: `Show ${name} panel`, exact: true}).click();
    await expect(page.locator(selector)).toBeVisible();
  }
  await page.getByRole('button', {name: 'Hide preview panel', exact: true}).click();
  await page.getByRole('button', {name: 'Hide inspector panel', exact: true}).click();
  await expect(page.getByRole('button', {name: 'Hide script panel', exact: true})).toBeDisabled();
  await page.locator('#panel-visibility summary').click();
  await capture('04-script-focus');
  await page.reload();
  await expect(page.locator('.preview-panel')).toBeHidden();
  await expect(page.locator('.inspector')).toBeHidden();
  await expect(page.locator('.cm-content')).toHaveText(visibleSource);
  await page.locator('#panel-visibility summary').click();
  await page.getByRole('button', {name: 'Show preview panel', exact: true}).click();
  await page.getByRole('button', {name: 'Show inspector panel', exact: true}).click();
  await page.locator('#panel-visibility summary').click();

  // The native picker is an OS boundary: verify its file contract and cancellation through the real UI.
  await page.evaluate(() => {
    window.pickerWrites = [];
    window.showSaveFilePicker = async options => {
      window.pickerOptions = options;
      if(window.cancelPicker) {
        throw new DOMException('Cancelled', 'AbortError');
      }
      return {createWritable: async () => ({
        write: async contents => {
          if(window.failWrite) {
            throw new Error('Disk write failed');
          }
          window.pickerWrites.push(contents);
        },
        close: async () => {window.pickerClosed = true;},
        abort: async () => {window.pickerAborted = true;}
      })};
    };
  });
  await page.locator('#save').click();
  await page.locator('#artifact-destination').selectOption('picker');
  await page.locator('#artifact-name').fill('my-recipe.form');
  await page.locator('#artifact-write').click();
  await expect(page.locator('#artifact-dialog')).toBeHidden();
  assert.equal(await page.evaluate(() => window.pickerWrites[0]), source);
  assert.equal(await page.evaluate(() => window.pickerOptions.suggestedName), 'my-recipe.form');
  assert.equal(await page.evaluate(() => window.pickerClosed), true);
  await page.evaluate(() => {window.cancelPicker = true;});
  await page.locator('#save').click();
  await page.locator('#artifact-write').click();
  await expect(page.locator('#artifact-dialog')).toBeVisible();
  assert.equal(await page.evaluate(() => window.pickerWrites.length), 1);
  await page.evaluate(() => {window.cancelPicker = false; window.failWrite = true;});
  await page.locator('#artifact-write').click();
  await expect(page.locator('#artifact-error')).toContainText('Disk write failed');
  assert.equal(await page.evaluate(() => window.pickerAborted), true);
  assert.equal(await page.evaluate(() => window.pickerWrites.length), 1);
  await page.keyboard.press('Escape');

  for(const width of [1100, 820, 600]) {
    await page.setViewportSize({width, height: 900});
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    await page.locator('.joy-editor-switcher summary').click();
    const menu = await page.locator('.joy-editor-menu').boundingBox();
    assert.ok(menu.x >= 0 && menu.x + menu.width <= width, 'Tool menu stays on-screen at narrower widths.');
    await page.keyboard.press('Escape');
    await page.locator('#panel-visibility summary').click();
    await page.getByRole('button', {name: 'Hide inspector panel', exact: true}).click();
    await expect(page.locator('.inspector')).toBeHidden();
    await page.getByRole('button', {name: 'Show inspector panel', exact: true}).click();
    await page.locator('#panel-visibility summary').click();
    await capture(`05-width-${width}`);
  }
  if(process.env.CAPTURE_WORKSPACE_URL) {
    await page.setViewportSize({width: 1440, height: 900});
    await page.goto(`${process.env.CAPTURE_WORKSPACE_URL}/editor/?project=luna`);
    await page.locator('.file-row[title="assets/form/pillar.form"]').click();
    const frame = page.frameLocator('iframe[title="Edit assets/form/pillar.form"]');
    await expect(frame.locator('#preview-status')).toHaveAttribute('data-state', 'generated', {timeout: 30000});
    await expect(frame.locator('#panel-visibility')).toBeVisible();
    await frame.locator('.layout-maximize').click();
    await frame.locator('#panel-visibility summary').click();
    await frame.getByRole('button', {name: 'Hide preview panel', exact: true}).click();
    await expect(frame.locator('.preview-panel')).toBeHidden();
    await capture('06-hosted-preview-hidden');
    await frame.getByRole('button', {name: 'Show preview panel', exact: true}).click();
    await frame.locator('#panel-visibility summary').click();
    await frame.locator('.layout-maximize').click();
    await expect(page.locator('#workspace')).not.toHaveClass(/lab-maximized/);
    for(const route of ['/editor/', '/joy-editor/tools/particle-lab/']) {
      await page.goto(`${process.env.CAPTURE_WORKSPACE_URL}${route}`);
      const toggle = page.locator('.joy-design-toggle');
      await expect(toggle).toBeVisible();
      await page.mouse.move(1, 1);
      const background = await toggle.evaluate(element => getComputedStyle(element).backgroundColor);
      await toggle.hover();
      assert.notEqual(await toggle.evaluate(element => getComputedStyle(element).backgroundColor), background);
      await toggle.click();
      await page.keyboard.press('Escape');
      await expect(toggle).toHaveAttribute('aria-expanded', 'false');
      assert.equal(await toggle.evaluate(element => getComputedStyle(element).backgroundColor), background);
      await capture(route.includes('particle') ? '07-particle-design-restored' : '07-editor-design-restored');
    }
  }
  assert.deepEqual(errors, []);
  console.log(JSON.stringify({passed: true, browser: browserName, directory}));
} finally {
  await writeFile(`${directory}/index.html`, `<!doctype html><meta charset="utf-8"><title>FORM LAB authoring</title><style>body{background:#1b293d;color:#fff3cf;font:16px system-ui;margin:24px}img{max-width:100%;border:1px solid #ed6c39}</style><h1>FORM LAB authoring · ${browserName}</h1>${captures.map(name => `<h2>${name}</h2><img src="${name}.png" alt="${name}">`).join('')}`);
  await browser.close();
  await server.close();
  if(libraryFixture) {
    await rm(libraryFixture, {recursive: true, force: true});
  }
}
