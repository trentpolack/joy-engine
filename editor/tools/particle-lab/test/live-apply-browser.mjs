// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
import {chromium,expect} from '@playwright/test';
import {createServer} from 'vite';

const server = await createServer({
  configFile:fileURLToPath(new URL('../vite.config.mjs',import.meta.url)),
  server:{host:'127.0.0.1',port:0,open:false}
});
let browser;
try {
  await server.listen();
  const origin = `http://127.0.0.1:${server.httpServer.address().port}`;
  browser = await chromium.launch({
    executablePath:process.env.CAPTURE_EXECUTABLE,
    args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']
  });
  const page = await browser.newPage({viewport:{width:1480,height:900}});
  await page.addInitScript(() => Object.defineProperty(navigator,'gpu',{value:undefined}));
  await page.goto(origin);
  const status = page.locator('#diagnostic');
  await expect(status).toHaveAttribute('data-state','live',{timeout:20000});
  await page.clock.install();
  await page.clock.pauseAt(new Date());
  const alpha = page.getByLabel('tint A value',{exact:true});
  const edit = async value => {
    await alpha.fill(value);
    await alpha.press('Tab');
  };
  await edit('0.8');
  await expect(status).toHaveAttribute('data-state','pending');
  await page.locator('#live').uncheck();
  await edit('0.7');
  await page.clock.runFor(1000);
  await expect(status).toHaveAttribute('data-state','pending');
  assert.equal(await alpha.inputValue(),'0.7','Live off retains the edited definition');
  console.log('PASS queued automatic apply is cancelled when Live turns off.');

  // Delay completion of real GPU preparation, without replacing its result.
  const viewportUrl = `${origin}/@fs${fileURLToPath(new URL('../src/viewport.ts',import.meta.url))}`;
  await page.evaluate(async url => {
    const {ParticleViewport} = await import(url);
    const prepare = ParticleViewport.prototype.prepareEffect;
    window.preparationEntered = false;
    window.releasePreparation = null;
    ParticleViewport.prototype.prepareEffect = async function(effect) {
      const ready = await prepare.call(this,effect);
      window.preparationEntered = true;
      await new Promise(resolve => {window.releasePreparation = resolve;});
      return ready;
    };
  },viewportUrl);
  await page.locator('#live').check();
  await page.clock.runFor(1000);
  await expect.poll(() => page.evaluate(() => window.preparationEntered)).toBe(true);
  await page.locator('#live').uncheck();
  await page.evaluate(() => window.releasePreparation());
  await page.clock.runFor(1000);
  await expect(status).toHaveAttribute('data-state','pending');
  console.log('PASS in-flight automatic preparation cannot publish after Live turns off.');

  await page.evaluate(() => {window.preparationEntered = false;});
  await page.locator('#live').check();
  await page.locator('#apply').click();
  await expect.poll(() => page.evaluate(() => window.preparationEntered)).toBe(true);
  await page.locator('#live').uncheck();
  await page.evaluate(() => window.releasePreparation());
  await expect(status).toHaveAttribute('data-state','live');
  console.log('PASS explicit Apply accepts the definition with Live off.');

  await edit('2');
  await expect(alpha).toHaveAttribute('aria-invalid','true');
  await page.locator('#apply').click();
  await expect(alpha).toHaveValue('2');
  await expect(alpha).toHaveAttribute('aria-invalid','true');
  await expect(status).toHaveAttribute('data-state','live');
  // LIVE continues to describe the last accepted/current valid definition;
  // the invalid draft is separate and remains repairable rather than exported.
  await edit('0.6');
  await expect(status).toHaveAttribute('data-state','pending');
  console.log('PASS invalid drafts retain their error and cannot replace the accepted definition.');
} finally {
  await browser?.close();
  await server.close();
}
