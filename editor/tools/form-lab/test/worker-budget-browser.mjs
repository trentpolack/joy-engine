// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {fileURLToPath} from 'node:url';
import {chromium,expect} from '@playwright/test';
import {createServer} from 'vite';

// Exercise the actual compiler with delayed module startup and blocked execution.
const server = await createServer({
  configFile:fileURLToPath(new URL('../vite.config.mjs',import.meta.url)),
  server:{host:'127.0.0.1',port:0}
});
let browser;
try {
  await server.listen();
  browser = await chromium.launch({
    executablePath:process.env.CAPTURE_EXECUTABLE,
    args:['--use-angle=swiftshader','--enable-unsafe-swiftshader']
  });
  const page = await browser.newPage({viewport:{width:1512,height:982}});
  await page.addInitScript(() => {
    Object.defineProperty(navigator,'gpu',{value:undefined});
    window.compilerTestMode = 'delayed-start';
    const NativeWorker = window.Worker;
    window.Worker = class extends NativeWorker {
      constructor(url,options) {
        const mode = window.compilerTestMode;
        if(!String(url).includes('compiler-worker')) {
          super(url,options);
          return;
        }
        const originalUrl = new URL(url,location.href).href;
        const bootstrap = `
          const queued = [];
          self.onmessage = event => queued.push(event);
          const mode = ${JSON.stringify(mode)};
          if(mode === 'delayed-start' || mode === 'stalled-start') {
            await new Promise(resolve => setTimeout(resolve,mode === 'delayed-start' ? 2400 : 11000));
          }
          await import(${JSON.stringify(originalUrl)});
          const compile = self.onmessage;
          if(mode === 'late-result') {
            const post = self.postMessage.bind(self);
            self.postMessage = value => post(value.ok ? {...value,milliseconds:2500} : value);
          }
          self.onmessage = event => {
            if(mode === 'blocked-execution') {
              self.postMessage({phase:'compiling'});
              const start = performance.now();
              while(performance.now() - start < 3000) {}
            }
            compile(event);
          };
          for(const event of queued) {self.onmessage(event);}
        `;
        const blobUrl = URL.createObjectURL(new Blob([bootstrap],{type:'text/javascript'}));
        super(blobUrl,options);
        this.blobUrl = blobUrl;
      }
      terminate() {
        super.terminate();
        if(this.blobUrl) {
          URL.revokeObjectURL(this.blobUrl);
        }
      }
    };
  });
  await page.goto(`http://127.0.0.1:${server.httpServer.address().port}/`,{waitUntil:'domcontentloaded'});
  await expect(page.locator('#diagnostic')).toContainText('Built successfully',{timeout:20000});
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  const revision = await page.locator('#viewport').getAttribute('data-requested-revision');
  for(const [mode,message] of [
    ['blocked-execution','Build exceeded 2 seconds'],
    ['late-result','Build exceeded 2 seconds'],
    ['stalled-start','Compiler startup exceeded 10 seconds']
  ]) {
    await page.evaluate(value => {window.compilerTestMode = value;},mode);
    await page.locator('#run').click();
    await expect(page.locator('#diagnostic')).toContainText(message,{timeout:15000});
    await expect(page.locator('#preview-status')).toHaveAttribute('data-state','failed');
    assert.equal(await page.locator('#viewport').getAttribute('data-requested-revision'),revision,'failed compilation retains accepted geometry');
  }
  await page.evaluate(() => {window.compilerTestMode = 'normal';});
  await page.locator('#run').click();
  await expect(page.locator('#diagnostic')).toContainText('Built successfully',{timeout:10000});
  await expect(page.locator('#preview-status')).toHaveAttribute('data-state','generated');
  console.log('PASS delayed module startup, execution timeout, late over-budget result, bounded startup and recovery.');
} finally {
  await browser?.close();
  await server.close();
}
