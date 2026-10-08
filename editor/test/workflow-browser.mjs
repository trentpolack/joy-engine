// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.
import assert from 'node:assert/strict';
import {mkdir, writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {createServer} from 'vite';
import {chromium, expect} from '@playwright/test';

const root = fileURLToPath(new URL('../../../', import.meta.url));
const output = new URL('../../../artifacts/captures/editor-workflow-v2/', import.meta.url);
await mkdir(output, {recursive: true});
const server = await createServer({plugins: [{name: 'workflow-fixture', configureServer(server) { server.middlewares.use((req, res, next) => { if(req.url === '/__workflow_fixture') { res.setHeader('Content-Type', 'text/html'); res.end('<!doctype html><body></body>'); } else { next(); } }); }}], configFile: `${root}scripts/development/vite-site.config.mjs`, server: {host: '127.0.0.1', port: 0, open: false}});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({executablePath: process.env.CAPTURE_EXECUTABLE, args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader']});
  const page = await browser.newPage({viewport: {width: 1440, height: 900}});
  await page.addInitScript(() => Object.defineProperty(navigator, 'gpu', {value: undefined}));
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  const captures = [];
  const base = `http://127.0.0.1:${server.httpServer.address().port}`;
  await page.goto(`${base}/__workflow_fixture`);
  await page.evaluate(async () => {
    const {ViewportInput} = await import('/editor/src/interaction/viewport-input.ts');
    const {guardNumberScroll} = await import('/editor/src/interaction/number-scroll.ts');
    document.body.innerHTML = '<canvas tabindex="0" style="width:400px;height:300px"></canvas><div id="scroller" style="height:100px;overflow:auto"><input type="number" value="7"><div style="height:1000px"></div></div>';
    window.events = [];
    const canvas = document.querySelector('canvas');
    window.interaction = new ViewportInput(canvas, {
      drag: (mode, x, y) => window.events.push([mode, x, y]),
      click: () => window.events.push(['click']),
      shortcut: key => window.events.push([key]),
      zoom: delta => window.events.push(['zoom', delta])
    });
    guardNumberScroll(document, new AbortController().signal);
  });
  await page.keyboard.down('Shift');
  await page.mouse.move(100, 100);
  await page.mouse.down();
  await page.mouse.move(120, 120);
  await page.keyboard.up('Shift');
  await page.mouse.move(140, 140);
  await page.mouse.up();
  assert.deepEqual(await page.evaluate(() => window.events.map(row => row[0])), ['pan', 'pan']);
  await page.keyboard.down('Shift');
  await page.mouse.click(100, 100);
  await page.keyboard.up('Shift');
  assert.equal(await page.evaluate(() => window.events.some(row => row[0] === 'click')), false);
  await page.mouse.down();
  await page.evaluate(() => window.dispatchEvent(new Event('blur')));
  await page.mouse.move(180, 180);
  await page.mouse.up();
  assert.equal(await page.evaluate(() => window.events.length), 2);
  await page.locator('input').focus();
  await page.keyboard.type('f0');
  await page.locator('input').fill('7');
  await page.mouse.move(40, 315);
  await page.mouse.wheel(0, 80);
  await expect(page.locator('input')).toHaveValue('7');
  await expect.poll(() => page.locator('#scroller').evaluate(node => node.scrollTop)).toBeGreaterThan(0);
  await page.locator('canvas').focus();
  await page.keyboard.press('Control+f');
  assert.equal(await page.evaluate(() => window.events.length), 2);
  await page.keyboard.press('f');
  assert.equal(await page.evaluate(() => window.events.at(-1)[0]), 'f');
  console.log('PASS frozen gestures, cancellation, text focus and number scrolling');
  await page.goto(`${base}/editor/?project=god-game`);
  await expect(page.locator('#workspace')).toHaveClass(/preview-collapsed/);
  await expect(page.locator('#play-mode')).toHaveValue('editor');
  await page.locator('button[title="assets/form/villager.form"]').dblclick();
  const authoring = page.frameLocator('iframe[title="Edit assets/form/villager.form"]');
  await expect(authoring.locator('#diagnostic-text')).toContainText('Built successfully', {timeout: 60000});
  await page.locator('#run').click();
  await expect(page.locator('#tabs')).toContainText('Game Preview');
  await expect(page.locator('#game-stage')).toBeVisible();
  const game = page.frameLocator('#game-stage iframe');
  await expect(game.locator('#world')).toHaveAttribute('data-ready', 'true', {timeout: 60000});
  await expect(page.locator('#pause')).toBeEnabled();
  await page.locator('#run').click();
  assert.equal(await page.locator('#game-stage iframe').count(), 1);
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Resume');
  await page.locator('#stop').click();
  await expect(authoring.locator('#viewport')).toBeVisible();
  await expect(page.locator('#tabs')).not.toContainText('Game Preview');
  await page.locator('#play-mode').selectOption('docked');
  await page.locator('#run').click();
  await expect(page.locator('#workspace')).not.toHaveClass(/preview-collapsed/);
  await expect(page.locator('#connection')).toContainText('CONNECTED', {timeout: 60000});
  await page.locator('#stop').click();
  await page.locator('#play-mode').selectOption('browser');
  const popupPromise = page.waitForEvent('popup');
  await page.locator('#run').click();
  const popup = await popupPromise;
  await expect(popup.locator('iframe')).toBeVisible();
  await expect(page.locator('#pause')).toBeEnabled({timeout: 60000});
  await popup.goto('about:blank');
  await expect(page.locator('#pause')).toBeDisabled();
  await expect(page.locator('#connection')).toContainText('DISCONNECTED');
  await page.locator('#stop').click();
  await expect.poll(() => popup.isClosed()).toBe(true);
  await page.locator('#play-mode').selectOption('window');
  await page.evaluate(() => { window.open = () => null; });
  await page.locator('#run').click();
  await expect(page.locator('#connection')).toContainText('BLOCKED');
  await expect(page.locator('#stop')).toBeDisabled();
  await page.reload();
  await expect(page.locator('#workspace')).toHaveClass(/preview-collapsed/);
  await expect(page.locator('#stop')).toBeDisabled();
  await page.locator('#assets-toggle').click();
  await expect(page.locator('#workspace')).toHaveClass(/assets-collapsed/);
  assert.ok(await page.locator('.document-area').evaluate(node => node.clientWidth) > 1000, 'Collapsed Assets must give its width to the authoring workspace.');
  await page.locator('#assets-toggle').click();
  await page.locator('button[title="assets/form/villager.form"]').dblclick();
  const separator = page.getByRole('separator', {name: 'Resize Content Browser'});
  await separator.focus();
  const before = await page.locator('.content-browser').evaluate(node => node.clientWidth);
  await page.keyboard.press('ArrowRight');
  await expect.poll(() => page.locator('.content-browser').evaluate(node => node.clientWidth)).toBeGreaterThan(before);
  const maximize = authoring.getByRole('button', {name: 'Maximize lab workspace'});
  await maximize.click();
  await expect(authoring.getByRole('button', {name: 'Restore workspace'})).toBeVisible();
  await authoring.getByRole('button', {name: 'Restore workspace'}).click();
  await expect(authoring.locator('.cm-content')).toBeVisible();
  await page.evaluate(() => {
    const saved = JSON.parse(localStorage.getItem('joy-editor-workspace-v1'));
    saved.width = 1500;
    localStorage.setItem('joy-editor-workspace-v1', JSON.stringify(saved));
  });
  await page.reload();
  await page.locator('#play-mode').selectOption('docked');
  await page.locator('#run').click();
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  assert.ok(await page.locator('.document-area').evaluate(node => node.clientWidth) > 350);
  await page.setViewportSize({width:1200,height:900});
  assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false);
  await page.setViewportSize({width:1440,height:900});
  await page.locator('#play-mode').selectOption('editor');
  await expect(page.locator('#tabs')).not.toContainText('Game Preview');
  page.once('dialog', dialog => dialog.accept());
  await page.locator('#restart').click();
  await expect(page.locator('#tabs')).toContainText('Game Preview');
  await page.locator('#stop').click();
  await page.goto(`${base}/editor/?project=space-scuffles`);
  await page.locator('#play-mode').selectOption('editor');
  await page.locator('#run').click();
  await expect(page.locator('#pause')).toBeEnabled({timeout:60000});
  await page.locator('#pause').click();
  await expect(page.locator('#pause')).toHaveText('Resume');
  await page.locator('#project').selectOption('luna');
  await expect(page.locator('#stop')).toBeDisabled();
  await expect(page.locator('#game-stage iframe')).toHaveCount(0);
  console.log('PASS cold-open, preview tab return, docked play, external lifecycle and blocked popup');
  // Exercise the representative authoring layouts without modifying saved fixtures.
  for(const viewport of [{width:1440,height:900},{width:1512,height:982},{width:1800,height:1040}]) {
    await page.setViewportSize(viewport);
    for(const [project, path, ready] of [
      ['vehicle-playground', 'assets/playground.joylevel', '#viewport[data-ready="true"]'],
      ['luna', 'assets/form/pillar.form', '#viewport'],
      ['space-scuffles', 'assets/joyfx/explosion.joyfx', '#emitter-list .emitter-item']
    ]) {
      await page.goto(`${base}/editor/?project=${project}`);
      await page.locator(`button[title="${path}"]`).dblclick();
      const frame = page.frameLocator(`iframe[title="Edit ${path}"]`);
      await expect(frame.locator(ready).first()).toBeVisible({timeout:60000});
      if(project === 'luna') { await expect(frame.locator('#diagnostic-text')).toContainText('Built successfully', {timeout:60000}); }
      const name = `${project}-${viewport.width}.png`;
      await page.screenshot({path:fileURLToPath(new URL(name,output)), timeout:60000});
      captures.push(name);
      assert.equal(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth), false, 'Workspace must fit laptop viewport.');
      const width = await frame.locator('#viewport').evaluate(node => node.clientWidth);
      assert.ok(width >= 260, `Authoring viewport too narrow: ${width}`);
      const max = frame.getByRole('button', {name: project === 'vehicle-playground' ? 'Maximize viewport' : 'Maximize lab workspace'});
      await max.click();
      await expect.poll(() => frame.locator('#viewport').evaluate(node => node.clientWidth)).toBeGreaterThan(width);
      await frame.getByRole('button', {name:'Restore workspace'}).click();
      await expect.poll(() => frame.locator('#viewport').evaluate(node => node.clientWidth)).toBe(width);
    }
  }
  await page.evaluate(() => { localStorage.setItem('joy-editor-play-v2','{"mode":"invalid"}'); localStorage.setItem('joy-editor-assets-v2','{"width":"bad"}'); });
  await page.reload();
  await expect(page.locator('#play-mode')).toHaveValue('editor');
  await expect(page.locator('#workspace')).toHaveClass(/preview-collapsed/);
  assert.deepEqual(errors, [], 'Unexpected browser errors');
  await writeFile(new URL('index.html', output), `<!doctype html><title>Editor workflow v2</title><style>body{background:#101722;color:#e8edf4;font:16px system-ui;margin:24px}img{max-width:100%;border:1px solid #40536b}h2{color:#ed6c39}</style><h1>Editor workflow v2 verification</h1><p>Local Chrome / software WebGL2. Camera input, numeric scroll safety, Play destinations, blocked launch, layout restoration and laptop-size checks passed.</p>${captures.map(name => `<h2>${name}</h2><img src="${name}" alt="${name}">`).join('')}`);
  console.log('PASS representative laptop layouts, maximize restoration and invalid preference fallback');
} finally {
  await browser?.close();
  await server.close();
}
