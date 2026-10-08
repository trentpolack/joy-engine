// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import { mkdir, writeFile, copyFile, readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium, expect } from '@playwright/test';
import { createServer, preview } from 'vite';
import { compile, defaultMaterial } from 'joy-engine/form';
import { exportGlb } from '../src/export-glb.ts';

const root = fileURLToPath(new URL('../', import.meta.url));
const built = process.argv.includes('--built');
const directory = new URL(
  `../../../../../artifacts/captures/${Date.now()}-form-lab/`,
  import.meta.url,
);
await mkdir(directory, { recursive: true });
const server = built
  ? await preview({
      root,
      configFile: false,
      preview: { host: '127.0.0.1', port: 0 },
    })
  : await createServer({
      configFile: `${root}vite.config.mjs`,
      server: { host: '127.0.0.1', port: 0 },
    });
const captures = [],
  errors = [],
  logs = [];
let browser;
let page;
async function capture(name) {
  await expect.poll(() => page.locator('#viewport').evaluate(canvas => canvas.dataset.requestedRevision === canvas.dataset.renderedRevision)).toBe(true);
  await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  await page.screenshot({
    path: fileURLToPath(new URL(`${name}.png`, directory)),
    fullPage: true,
  });
  captures.push(name);
}
async function edit(source) {
  const editor = page.locator('.cm-content');
  await editor.click();
  await page.keyboard.press('ControlOrMeta+a');
  await page.keyboard.insertText(source);
}
try {
  if(!built) {
    await server.listen();
  }
  const address = server.httpServer.address();
  const url = process.env.CAPTURE_URL ?? `http://127.0.0.1:${address.port}/`;
  browser = await chromium.launch({
    executablePath: process.env.CAPTURE_EXECUTABLE,
    channel: process.env.CAPTURE_CHANNEL,
    args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'],
  });
  page = await browser.newPage({ viewport: { width: 1480, height: 900 } });
  page.on('pageerror', (error) => errors.push(error.message));
  page.on('console', (message) => {
    if(message.type() === 'error') {
      errors.push(message.text());
    }
    logs.push(message.text());
  });
  let confirmations = 0;
  page.on('dialog', (dialog) => {
    confirmations++;
    return dialog.accept();
  });
  await page.addInitScript(() =>
    Object.defineProperty(navigator, 'gpu', { value: undefined }),
  );
  await page.goto(url);
  await expect(page.locator('#diagnostic')).toContainText(
    'Built successfully',
    { timeout: 20000 },
  );
  await expect(page.locator('#backend')).toContainText('WEBGL 2');
  await expect(page.locator('#gpu-error')).toBeHidden();
  await page.locator('#document-name').fill('Suite study');
  await page.locator('#document-name').press('Tab');
  await expect(page.locator('#editor-filename')).toHaveText('suite-study.form');
  await expect(page.locator('.cm-content')).toContainText('name="Suite study"');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await expect(page.locator('#scene-title')).toHaveText('MAKE\nSOME WAVES.');
  await expect(page.locator('#export')).toContainText('EXPORT DATA');
  await expect(page.locator('#export')).toHaveCSS('box-shadow', 'none');
  const optionBounds = await page.locator('.preview-bottom').boundingBox();
  const resultBounds = await page.locator('.generation').boundingBox();
  assert.ok(
    optionBounds.y + optionBounds.height <= resultBounds.y + 1,
    'viewport options precede generation results',
  );
  assert.equal(await page.locator('.preview-bottom #backend').count(), 1);
  await expect(page.locator('#point-size')).toHaveCSS('--range-fill', '20%');
  await page.locator('#point-size').focus();
  await page.keyboard.press('End');
  await expect(page.locator('#point-size')).toHaveCSS('--range-fill', '100%');
  await page.keyboard.press('Home');
  await expect(page.locator('#point-size')).toHaveCSS('--range-fill', '0%');
  await page.keyboard.press('ArrowRight');
  await page.keyboard.press('ArrowRight');
  await page.evaluate(() => document.fonts.ready);
  const parameter = page
    .locator('.cm-script-parameter')
    .filter({ hasText: /^height$/ })
    .first();
  await expect(parameter).toHaveCSS('color', 'rgb(135, 220, 227)');
  await parameter.dblclick();
  assert.equal(
    await page.evaluate(() => getSelection()?.toString()),
    'height',
    'double click selects the complete parameter',
  );
  await expect(page.locator('.cm-activeLine')).toHaveCSS(
    'background-color',
    'color(srgb 0.145098 0.207843 0.298039 / 0.4)',
  );
  await capture('parameter-selection');
  await page.keyboard.press('ArrowRight');
  await capture('01-wave-study');
  const previewBeforeFullscreen = await page.locator('#viewport').boundingBox();
  await page.locator('.layout-maximize').click();
  await expect(page.locator('.layout-maximize')).toHaveAttribute('aria-pressed', 'true');
  await expect.poll(async () => (await page.locator('#viewport').boundingBox()).width).toBeCloseTo(previewBeforeFullscreen.width, 0);
  await capture('preview-fullscreen');
  await page.keyboard.press('Escape');
  await expect(page.locator('.layout-maximize')).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(async () => (await page.locator('#viewport').boundingBox()).width).toBeCloseTo(previewBeforeFullscreen.width, 0);
  await expect(page.locator('.layout-maximize')).toBeFocused();

  await expect(page.locator('.workspace-bar')).toBeHidden();
  await page.getByRole('separator', {name:'Resize editing panel'}).focus();
  await page.keyboard.press('ArrowRight');
  const customWidth = (await page.locator('.editor-panel').boundingBox()).width;
  await page.reload();
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  assert.ok(Math.abs((await page.locator('.editor-panel').boundingBox()).width - customWidth) < 2);
  await page.getByRole('separator', {name:'Resize editing panel'}).dblclick();
  await page.getByLabel('Find parameter').fill('height');
  await expect(page.locator('#param-height')).toBeVisible();
  await expect(page.locator('#param-frequency')).toBeHidden();
  await page.getByLabel('Find parameter').fill('');
  await page.getByRole('tab', {name:'Materials', exact:true}).click();
  await expect(page.locator('#materials')).toBeVisible();
  await expect(page.locator('#parameters')).toBeHidden();
  await page.getByRole('tab', {name:'Parameters', exact:true}).click();
  await capture('workspace-balanced');
  await page.getByRole('tab', {name:'Variants', exact:true}).click();
  await page.locator('#variant-name').fill('Script defaults');
  await page.locator('#variant-save').click();
  await page.getByRole('tab', {name:'Parameters', exact:true}).click();
  await page.locator('#param-height').fill('4');
  await page.locator('#param-height').press('Tab');
  await page.getByRole('tab', {name:'Variants', exact:true}).click();
  await page.locator('#variant-name').fill('Tall waves');
  await page.locator('#variant-save').click();
  await page.locator('#variant-select').selectOption({label:'Script defaults'});
  await page.locator('#variant-apply').click();
  await page.getByRole('tab', {name:'Parameters', exact:true}).click();
  await expect(page.locator('#param-height')).toHaveValue('2.4');
  await page.reload();
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await page.getByRole('tab', {name:'Variants', exact:true}).click();
  await page.locator('#variant-select').selectOption({label:'Tall waves'});
  await page.locator('#variant-apply').click();
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await capture('workspace-variants');
  await page.getByRole('tab', {name:'Parameters', exact:true}).click();
  await expect(page.locator('#param-height')).toHaveValue('4');
  await page.getByRole('tab', {name:'Variants', exact:true}).click();
  await page.locator('#variant-select').selectOption({label:'Script defaults'});
  await page.locator('#variant-apply').click();
  await page.getByRole('tab', {name:'Parameters', exact:true}).click();
  for(const [width, height] of [
    [1480, 900],
    [1280, 720],
    [1024, 768],
  ]) {
    await page.setViewportSize({ width, height });
    assert.ok(
      await page.evaluate(
        () =>
          document.documentElement.scrollHeight <= innerHeight &&
          document.documentElement.scrollWidth <= innerWidth,
      ),
      `${width}x${height} must fit one page`,
    );
    const editorBounds = await page.locator('#editor').boundingBox();
    assert.ok(editorBounds.height > 200, 'editor keeps usable height');
    await capture(`desktop-${width}x${height}`);
  }
  await page.setViewportSize({ width: 1480, height: 900 });
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await page.locator('#reference-link').click();
  await expect(page.locator('#reference-dialog')).toBeVisible();
  await page.keyboard.press('Escape');
  assert.equal(await page.locator('#triangle-count').textContent(), '8,192');
  // Canvas pixel comparison proves orbit interaction actually changes the render.
  const canvas = page.locator('#viewport');
  const before = await canvas.screenshot();
  const bounds = await canvas.boundingBox();
  await page.mouse.move(
    bounds.x + bounds.width / 2,
    bounds.y + bounds.height / 2,
  );
  await page.mouse.down();
  await page.mouse.move(
    bounds.x + bounds.width / 2 + 100,
    bounds.y + bounds.height / 2 + 35,
    { steps: 10 },
  );
  await page.mouse.up();
  assert.notDeepEqual(
    await canvas.screenshot(),
    before,
    'orbit must change pixels',
  );
  await page.getByRole('button', { name: 'Frame', exact: false }).click();
  await page.locator('#param-height').focus();
  await page.keyboard.press('End');
  await expect(
    page.getByRole('spinbutton', { name: 'height value' }),
  ).toHaveValue('5');
  await expect(page.locator('#diagnostic')).toContainText('Built successfully');
  await page.locator('#examples').selectOption('1');
  await expect(page.locator('#point-count')).toHaveText('16,000');
  await capture('02-orbital-bloom');
  await page.locator('#examples').selectOption('2');
  await expect(page.locator('#triangle-count')).toHaveText('1,452');
  await capture('03-pocket-city');
  await page.locator('#examples').selectOption('3');
  await expect(page.locator('#diagnostic')).toContainText('Built successfully');
  await capture('04-satellite-garden');
  await page.locator('#live').uncheck();
  await edit(
    'param offset = vector(1,2,3) meta(min=-5,max=5,step=0.1)\nparam tint = rgba(1,0.5,0,1)\ncolor(tint)\nbox(offset.x,offset.y,offset.z,2,2,2)',
  );
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','pending');
  await page.locator('#run').click();
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await expect(page.locator('#parameters select')).toHaveCount(0);
  const colorBefore = await canvas.screenshot();
  await page
    .getByRole('spinbutton', { name: 'offset X value', exact: true })
    .fill('4');
  await page.keyboard.press('Tab');
  await page
    .getByRole('spinbutton', { name: 'offset Y value', exact: true })
    .fill('5');
  await page.keyboard.press('Tab');
  await page
    .getByRole('spinbutton', { name: 'tint A value', exact: true })
    .fill('0.3');
  await page.keyboard.press('Tab');
  await expect(page.locator('#parameters .color-swatch i')).toHaveCSS(
    'background-color',
    'rgba(255, 128, 0, 0.3)',
  );
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','pending');
  await page.locator('#run').click();
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  assert.notDeepEqual(
    await canvas.screenshot(),
    colorBefore,
    'typed edits change rendered geometry',
  );
  await page.locator('#export-format').selectOption('json');
  const typedDownload = page.waitForEvent('download');
  await page.locator('#export').click();
  const typedPath = fileURLToPath(new URL('typed-geometry.json', directory));
  await (await typedDownload).saveAs(typedPath);
  const typedGeometry = JSON.parse(await readFile(typedPath, 'utf8'));
  assert.deepEqual(typedGeometry.parameters[0].value, [4, 5, 3]);
  assert.ok(typedGeometry.alphas.every((value) => value === 0.3));
  await page.reload();
  await expect(
    page.getByRole('spinbutton', { name: 'offset X value', exact: true }),
  ).toHaveValue('4');
  await expect(
    page.getByRole('spinbutton', { name: 'tint A value', exact: true }),
  ).toHaveValue('0.3');
  await capture('typed-parameters');
  await page.locator('#reset').click();
  await expect(
    page.getByRole('spinbutton', { name: 'offset X value', exact: true }),
  ).toHaveValue('1');
  await expect(
    page.getByRole('spinbutton', { name: 'tint A value', exact: true }),
  ).toHaveValue('1');
  await expect(page.locator('#param-tint-a')).toHaveCSS('--range-fill', '100%');
  await page.locator('#export-format').selectOption('obj');
  await edit('point(0,0,0)\npoint(missing,0,0)');
  await expect(page.locator('#diagnostic')).toContainText('Line 2');
  await page.locator('#error-location').click();
  await expect(page.locator('#cursor-position')).toContainText('Ln 2');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','failed');
  await capture('05-error-retains-preview');
  await edit(
    'param height = 2, 1, 5, 1\ncolor(1,0.5,0)\nbox(0,0,0,2,height,2)\npoint(0,3,0)',
  );
  await expect(page.locator('#triangle-count')).toHaveText('12');
  await expect(page.locator('#point-count')).toHaveText('1');
  await page.locator('#live').uncheck();
  await edit('point(1,2,3)');
  await expect(page.locator('#diagnostic')).toContainText('Run to update');
  await expect(page.locator('#triangle-count')).toHaveText('12');
  await page.locator('#run').click();
  await expect(page.locator('#triangle-count')).toHaveText('0');
  const downloadPromise = page.waitForEvent('download');
  await page.locator('#export').click();
  const download = await downloadPromise;
  await download.saveAs(fileURLToPath(new URL('export.obj', directory)));
  const savePromise = page.waitForEvent('download');
  await page.locator('#save').click();
  const saved = await savePromise;
  const savePath = fileURLToPath(new URL('saved.formlab', directory));
  await saved.saveAs(savePath);
  await page.locator('#file-input').setInputFiles(savePath);
  await expect(page.locator('#diagnostic')).toContainText('Built successfully');
  await page.reload();
  await expect(page.locator('#point-count')).toHaveText('1');
  await expect(page.locator('.cm-content')).toContainText('point(1,2,3)');
  await page.locator('#help').click();
  await expect(page.locator('#reference-dialog')).toBeVisible();
  await capture('06-language-reference');
  await page.keyboard.press('Escape');
  const beforeReplace = confirmations;
  await page.locator('#examples').selectOption('0');
  assert.equal(
    confirmations,
    beforeReplace + 1,
    'restored autosave must receive replacement protection',
  );
  await expect(page.locator('#triangle-count')).toHaveText('8,192');
  await page.locator('#points-mode').click();
  await expect(page.locator('#points-mode')).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await capture('07-points-preview');
  await page.locator('#shaded').click();
  await edit('param __proto__ = 1, 0, 10, 1\npoint(__proto__,0,0)');
  await expect(page.locator('#point-count')).toHaveText('1');
  await page.getByRole('spinbutton', { name: '__proto__ value' }).fill('7');
  await page.keyboard.press('Tab');
  await expect(page.locator('#diagnostic')).toContainText('Built successfully');
  await page.locator('#export-format').selectOption('json');
  const prototypeDownload = page.waitForEvent('download');
  await page.locator('#export').click();
  const prototypePath = fileURLToPath(
    new URL('prototype-parameter.json', directory),
  );
  await (await prototypeDownload).saveAs(prototypePath);
  assert.equal(
    JSON.parse(await readFile(prototypePath, 'utf8')).positions[0],
    7,
  );
  await page.locator('#examples').selectOption('0');
  await expect(page.locator('#triangle-count')).toHaveText('8,192');
  await edit(
    'project meta(name="Metadata study", info="AUTHORED INFO", title="SCRIPT TITLE")\nparam radius = 2 meta(min=1,max=5,step=0.1)\npoint(radius,0,0)',
  );
  await expect(page.locator('#scene-title')).toHaveText('SCRIPT TITLE');
  await expect(page.locator('#example-kind')).toHaveText('AUTHORED INFO');
  await expect(page.locator('#document-name')).toHaveValue('Metadata study');
  await expect(page.locator('#editor-filename')).toHaveText(
    'metadata-study.form',
  );
  await page.reload();
  await expect(page.locator('#scene-title')).toHaveText('SCRIPT TITLE');
  await expect(page.locator('#document-name')).toHaveValue('Metadata study');
  await capture('project-metadata');
  await page.locator('#examples').selectOption('0');
  await expect(page.locator('#triangle-count')).toHaveText('8,192');
  // Resource overrides exercise the same recipe inputs a runtime caller supplies.
  await page.locator('#examples').selectOption({label: 'Parametric bridge'});
  await expect(page.locator('#document-name')).toHaveValue('Parametric bridge');
  await expect(page.locator('#diagnostic')).toContainText('Built successfully');
  await expect(page.getByLabel('plank mesh source')).toHaveValue('current');
  await capture('runtime-bridge-default');
  const tintControl = page.getByRole('slider', {name:'tint R', exact:true});
  await tintControl.focus();
  await page.keyboard.press('ArrowLeft');
  await page.waitForTimeout(600);
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await expect(tintControl).toBeFocused();
  await page.getByLabel('plank mesh source').selectOption('sphere');
  await expect(page.getByLabel('plank mesh source')).toHaveValue('current');
  await expect(page.locator('#triangle-count')).not.toHaveText('312');
  await page.getByLabel('finish roughness', {exact: true}).fill('0.2');
  await page.getByLabel('finish roughness', {exact: true}).press('Tab');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await page.reload();
  await expect(page.getByLabel('plank mesh source')).toContainText('Sphere');
  await expect(page.getByLabel('finish roughness', {exact: true})).toHaveValue('0.2');
  await page.locator('.resource-parameter').first().scrollIntoViewIfNeeded();
  await capture('runtime-resource-overrides');
  await page.locator('.resource-parameter').filter({has: page.getByLabel('plank mesh source')}).getByRole('button', {name:'Use script default'}).click();
  await expect(page.getByLabel('plank mesh source').locator('option:checked')).toHaveText('Box');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  const replacementMesh = exportGlb(compile('sphere(0, 0, 0, 0.5, 6)'));
  await page.getByLabel('plank GLB geometry').setInputFiles({
    name: 'replacement.glb', mimeType: 'model/gltf-binary', buffer: Buffer.from(replacementMesh)
  });
  await expect(page.getByLabel('plank mesh source')).toContainText('replacement.glb');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await page.reload();
  await expect(page.getByLabel('plank mesh source')).toContainText('replacement.glb');
  await page.locator('#examples').selectOption({label: 'Terrain workshop'});
  await expect(page.locator('#document-name')).toHaveValue('Terrain workshop');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await expect(page.locator('#output-select')).toContainText('Terrain');
  await capture('procedural-workshop');
  await page.locator('#output-select').selectOption('Terrain');
  await page.locator('#attribute-panel summary').click();
  await expect(page.locator('#attribute-table')).toContainText('density');
  await page.getByRole('button', {name: 'Select points 0', exact: true}).click();
  await expect(page.locator('#selection-info')).toContainText('points 0');
  await expect(page.locator('#viewport')).toHaveAttribute('data-selected-element', '0');
  await page.locator('#attribute-view').selectOption('density');
  await expect(page.locator('#attribute-range')).toContainText('density');
  await capture('procedural-attributes');
  await page.locator('#output-select').selectOption('Sites');
  await expect(page.locator('#selection-info')).toContainText('Select');
  await expect(page.locator('#attribute-table')).toContainText('scale');
  await page.locator('#output-select').selectOption('Columns');
  await expect(page.locator('#attribute-domain')).toHaveValue('instances');
  await expect(page.locator('#attribute-table')).toContainText('rotation');
  await page.reload();
  await expect(page.locator('#diagnostic')).toContainText('Built successfully');
  await expect(page.locator('#output-select')).toContainText('Columns');
  await page.locator('#output-select').selectOption('Terrain');
  await edit('let g = geometry() output("Empty", g)');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await expect(page.locator('#output-select')).toHaveValue('');
  await page.locator('#output-select').selectOption('Empty');
  await expect(page.locator('#attribute-summary')).toContainText('0 points');
  await page.locator('#reference-link').click();
  await page.getByLabel('Search procedural recipes').fill('primitive');
  await page.getByRole('button', {name: 'Append Shape a primitive', exact: true}).click();
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await expect(page.locator('#output-select')).toContainText('recipe_1');
  await page.locator('#output-select').selectOption('recipe_1');
  await capture('procedural-primitive-recipe');
  await page.locator('#file-input').setInputFiles({name: 'invalid.formlab', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify({version:1,name:'Invalid',source:'point(missing,0,0)',overrides:{}}))});
  await expect(page.locator('#diagnostic')).toContainText('Unknown variable');
  await expect(page.locator('#output-select')).toBeDisabled();
  await expect(page.locator('#output-select')).not.toContainText('recipe_1');
  await expect(page.locator('#attribute-table tbody tr')).toHaveCount(0);
  await page.locator('#examples').selectOption({label:'Vessel workshop'});
  await expect(page.locator('#output-select')).toContainText('Vessel');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await capture('profile-workshop');
  await page.locator('#examples').selectOption({label:'Path workshop'});
  await expect(page.locator('#output-select')).toContainText('Pipe');
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await capture('path-workshop');
  for(const study of [
    {name: 'Harvest grove', output: 'Resource nodes', kind: 'resource-node', attribute: 'remaining', capture: 'harvest-grove'},
    {name: 'Patrol circuit', output: 'Waypoints', kind: 'waypoint', attribute: 'speed', capture: 'patrol-circuit'},
    {name: 'Tactical terraces', output: 'Tiles', kind: 'tile', attribute: 'moveCost', capture: 'tactical-terraces'}
  ]) {
    await page.locator('#examples').selectOption({label: study.name});
    await expect(page.locator('#output-select')).toContainText(study.output);
    await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
    await page.locator('#attribute-panel').evaluate(panel => {
      panel.open = false;
    });
    await page.locator('#frame').click();
    await capture(study.capture);
    await page.locator('#output-select').selectOption(study.output);
    await page.locator('#attribute-panel').evaluate(panel => {
      panel.open = true;
    });
    await expect(page.locator('#attribute-table')).toContainText(study.attribute);
    await expect(page.locator('#attribute-table')).toContainText(study.kind);
    await capture(`${study.capture}-data`);
  }
  const goodSource = 'param finish=materialPbr(rgba(1,1,1,1),0,1) let g=grid(1,1,2,2) output("Good",g)';
  const broken = defaultMaterial();
  broken.textures.baseColor = 'data:image/png;base64,AAAA';
  await page.locator('#file-input').setInputFiles({name: 'texture-failure.formlab', mimeType: 'application/json', buffer: Buffer.from(JSON.stringify({version:1,name:'Texture failure',source:goodSource,overrides:{finish:{kind:'material',material:broken}}}))});
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  await page.locator('#output-select').selectOption('Good');
  await expect(page.locator('#attribute-summary')).toContainText('4 points');
  await edit('param finish=materialPbr(rgba(1,1,1,1),0,1) material(finish) let g=grid(2,2,4,4) output("Bad",g)');
  await expect(page.locator('#gpu-error')).toBeVisible();
  await expect(page.locator('#output-select')).toHaveValue('Good');
  await expect(page.locator('#attribute-summary')).toContainText('4 points');
  await expect(page.locator('#output-select')).not.toContainText('Bad');

  await page.locator('#examples').selectOption('0');
  await expect(page.locator('#triangle-count')).toHaveText('8,192');
  for(const [name, width, height, density] of [
    ['tablet', 820, 1180, 1],
    ['mobile', 390, 844, 2],
    ['narrow-mobile', 320, 740, 1],
  ]) {
    const context = await browser.newContext({
      viewport: { width, height },
      deviceScaleFactor: density,
    });
    const previous = page;
    page = await context.newPage();
    await page.addInitScript(() =>
      Object.defineProperty(navigator, 'gpu', { value: undefined }),
    );
    await page.goto(url);
    await expect(page.locator('#diagnostic')).toContainText(
      'Built successfully',
    );
    assert.ok(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= innerWidth,
      ),
      'no horizontal overflow',
    );
    await capture(`08-${name}`);
    await context.close();
    page = previous;
  }
  assert.deepEqual(errors, []);
  if(built) {
    assert.ok(
      !logs.some((message) => message.startsWith('[Form Lab]')),
      'production must be silent',
    );
  }
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.locator('#frame').click();
  await page.screenshot({
    path: fileURLToPath(
      new URL('../../../../../platform/site/content/previews/form-lab.png', import.meta.url),
    ),
  });
  await writeFile(
    new URL('results.json', directory),
    JSON.stringify(
      { passed: true, backend: 'webgl', built, errors, captures },
      null,
      2,
    ),
  );
  console.log(
    `FORM LAB browser checks passed. Gallery: ${fileURLToPath(new URL('index.html', directory))}`,
  );
} catch (error) {
  if(page) {
    await capture('failure').catch(() => {});
  }
  await writeFile(
    new URL('results.json', directory),
    JSON.stringify({ passed: false, error: String(error), errors }, null, 2),
  );
  throw error;
} finally {
  await writeFile(
    new URL('index.html', directory),
    `<!doctype html><title>FORM LAB validation</title><style>body{background:#1b293d;color:#fff3cf;font:16px system-ui;padding:30px}img{width:100%;border:1px solid #ed6c39}section{margin-bottom:40px}</style><h1>FORM LAB · Browser validation</h1>${captures.map((name) => `<section><h2>${name}</h2><img src="${name}.png"></section>`).join('')}`,
  );
  await browser?.close();
  await server.close();
}

