// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

import assert from 'node:assert/strict';
import {mkdir, readFile, writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {chromium, expect} from '@playwright/test';
import {createServer} from 'vite';

const directory = fileURLToPath(new URL('../artifacts/lab-controls/', import.meta.url));
await mkdir(directory, {recursive: true});
const browser = await chromium.launch({
  executablePath: process.env.CAPTURE_EXECUTABLE,
  args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader']
});
const servers = [];
const errors = [];
const checks = [];

async function openLab(name) {
  const configFile = fileURLToPath(new URL(`../tools/${name}/vite.config.mjs`, import.meta.url));
  const server = await createServer({configFile, server: {host: '127.0.0.1', port: 0}});
  servers.push(server);
  await server.listen();
  const page = await browser.newPage({viewport: {width: 1440, height: 900}});
  page.on('pageerror', error => errors.push(error.message));
  page.on('dialog', dialog => dialog.accept());
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/?backend=webgl`);
  return page;
}

async function exported(page) {
  const pending = page.waitForEvent('download');
  await page.locator('#save').click();
  const download = await pending;
  return JSON.parse(await readFile(await download.path(), 'utf8'));
}

try {
  const form = await openLab('form-lab');
  await expect(form.locator('#diagnostic')).toContainText('Built successfully');
  await form.locator('#live').uncheck();
  await form.getByLabel('tint color picker', {exact: true}).fill('#808080');
  await form.getByLabel('tint R value', {exact: true}).fill('2');
  await form.getByLabel('tint R value', {exact: true}).press('Tab');
  await form.getByLabel('tint color picker', {exact: true}).fill('#000000');
  await expect(form.getByLabel('tint color picker', {exact: true})).toHaveValue('#808080');
  await expect(form.getByLabel('tint R value', {exact: true})).toHaveValue('2');
  await form.getByLabel('tint R value', {exact: true}).fill('0.21586050011389926');
  await form.getByLabel('tint R value', {exact: true}).press('Tab');
  await form.locator('#live').check();
  checks.push('FORM rejected picker preserves accepted color and invalid draft with Live off');
  await form.locator('#examples').selectOption({label: 'Material studio'});
  await expect(form.locator('#preview-status')).toHaveAttribute('data-state', 'generated');
  const material = form.locator('[data-resource-parameter="polished"]');
  const initialPicker = await material.getByLabel('polished color picker', {exact: true}).inputValue();
  await material.getByLabel('polished Red', {exact: true}).fill('2');
  await material.getByLabel('polished Red', {exact: true}).press('Tab');
  await material.getByLabel('polished color picker', {exact: true}).fill('#000000');
  await expect(material.getByLabel('polished color picker', {exact: true})).toHaveValue(initialPicker);
  await expect(material.getByLabel('polished Red', {exact: true})).toHaveValue('2');
  await material.getByLabel('polished Red', {exact: true}).fill('0.95');
  await material.getByLabel('polished Red', {exact: true}).press('Tab');
  await material.getByRole('button', {name: 'Use script default'}).click();
  await material.getByLabel('polished color picker', {exact: true}).fill('#808080');
  let saved = await exported(form);
  assert.ok(Math.abs(saved.overrides.polished.material.baseColor[0] - 0.21586050011389926) < 1e-12);
  assert.equal(saved.overrides.polished.material.baseColor[3], 1);
  await form.locator('#property-undo').click();
  assert.equal((await exported(form)).overrides.polished, undefined);
  await form.locator('#property-redo').click();
  await material.getByLabel('polished roughness', {exact: true}).fill('0.123456');
  await material.getByLabel('polished roughness', {exact: true}).press('Tab');
  await material.getByLabel('polished Red', {exact: true}).fill('0.456789');
  await material.getByLabel('polished Red', {exact: true}).press('Tab');
  saved = await exported(form);
  assert.equal(saved.overrides.polished.material.roughness, 0.123456);
  assert.equal(saved.overrides.polished.material.baseColor[0], 0.456789);
  await material.getByRole('button', {name: 'Use script default'}).click();
  assert.equal((await exported(form)).overrides.polished, undefined);
  await form.locator('#property-undo').click();
  assert.equal((await exported(form)).overrides.polished.material.baseColor[0], 0.456789);
  checks.push('FORM material picker, linear RGB, exact edits, reset, and undo/redo round-trip');
  await expect(form.locator('#preview-status')).toHaveAttribute('data-state', 'generated');
  await form.screenshot({path: `${directory}/form-after.png`});
  await form.locator('#materials-tab').click();
  await form.locator('#material-baseColor-3').fill('0.456789');
  await form.locator('#material-baseColor-3').press('Tab');
  await form.getByLabel('Base Color color picker', {exact: true}).fill('#808080');
  await form.locator('#material-roughness').fill('0.234567');
  saved = await exported(form);
  assert.equal(saved.materials[0].baseColor[3], 0.456789);
  assert.equal(saved.materials[0].roughness, 0.234567);
  assert.ok(Math.abs(saved.materials[0].baseColor[0] - 0.21586050011389926) < 1e-12);
  await form.screenshot({path: `${directory}/form-materials.png`});
  checks.push('FORM palette color picker, alpha, exact factor precision, and focused Save');
  await form.locator('#parameters-tab').click();

  const particle = await openLab('particle-lab');
  await expect(particle.locator('#diagnostic')).toHaveText('LIVE');
  const particlePicker = await particle.getByLabel('tint color picker', {exact: true}).inputValue();
  await particle.getByLabel('tint A value', {exact: true}).fill('2');
  await particle.getByLabel('tint A value', {exact: true}).press('Tab');
  await particle.getByLabel('tint color picker', {exact: true}).fill('#000000');
  await expect(particle.getByLabel('tint color picker', {exact: true})).toHaveValue(particlePicker);
  await expect(particle.getByLabel('tint A value', {exact: true})).toHaveValue('2');
  await particle.getByLabel('tint A value', {exact: true}).fill('0.321');
  await particle.getByLabel('tint A value', {exact: true}).press('Tab');
  await particle.getByLabel('tint color picker', {exact: true}).fill('#808080');
  saved = await exported(particle);
  assert.equal(saved.parameters.tint.value[3], 0.321);
  assert.ok(Math.abs(saved.parameters.tint.value[0] - 0.21586050011389926) < 1e-12);
  await particle.locator('#property-undo').click();
  assert.equal((await exported(particle)).parameters.tint.value[0], 1);
  await particle.locator('#property-redo').click();
  const wind = particle.getByLabel('wind X value', {exact: true});
  await wind.fill('9.123456');
  await wind.press('Tab');
  assert.equal((await exported(particle)).parameters.wind.value[0], 9.123456);
  await wind.focus();
  await wind.press('Shift+ArrowUp');
  await expect(wind).toHaveValue('9.133456');
  checks.push('PARTICLE picker preserves alpha, linear RGB, exact vector edits, history, and fine keys');
  await particle.screenshot({path: `${directory}/particle-after.png`});

  for(const [name, page] of [['form', form], ['particle', particle]]) {
    const range = page.locator('.lab-range:visible').first();
    await range.focus();
    await range.press('ArrowLeft');
    await expect(range).toHaveCSS('border-top-width', '0px');
    await expect(range).toHaveCSS('outline-style', 'solid');
    await page.setViewportSize({width: 1100, height: 800});
    await expect.poll(() => page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
    await page.screenshot({path: `${directory}/${name}-narrow.png`});
    checks.push(`${name} borderless slider retains keyboard focus; narrow viewport has no horizontal overflow`);
  }
  assert.deepEqual(errors, []);
  await writeFile(`${directory}/results.json`, JSON.stringify({passed: true, checks, errors}, null, 2));
  console.log(JSON.stringify({passed: true, checks, directory}));
} finally {
  await browser.close();
  for(const server of servers) {
    await server.close();
  }
}
