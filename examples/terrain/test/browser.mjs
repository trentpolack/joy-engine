// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium, expect } from '@playwright/test';
import { createServer, preview } from 'vite';
const built = process.argv.includes('--built');
const directory = new URL(`../../../../artifacts/captures/${Date.now()}-terrain-lab/`,import.meta.url);
await mkdir(directory,{recursive:true});
const configFile = fileURLToPath(new URL('../vite.config.js',import.meta.url));
const server = built ? await preview({configFile,preview:{host:'127.0.0.1',port:0}}) : await createServer({configFile,server:{host:'127.0.0.1',port:0}});
const captures = [], reports = [];
let browser, page, success = false;
try {
  if(!built) { await server.listen(); }
  const address = server.httpServer.address();
  const url = `http://127.0.0.1:${address.port}/`;
  browser = await chromium.launch({headless:process.env.TERRAIN_HEADLESS === '1' || (process.platform !== 'darwin' && !process.env.TERRAIN_HEADED),executablePath:process.env.CAPTURE_EXECUTABLE,args:['--enable-unsafe-webgpu',...(process.env.TERRAIN_SOFTWARE ? ['--use-angle=swiftshader','--enable-unsafe-swiftshader'] : [])]});
  for(const backend of (process.env.TERRAIN_BACKEND ? [process.env.TERRAIN_BACKEND] : ['webgl','webgpu'])) {
    page = await browser.newPage({viewport:{width:1440,height:1000}});
    const errors = [], routineLogs = [];
    page.on('pageerror',error => (errors.push(error.message),console.error('Page error:',error.message)));
    page.on('console',message => { if(['log','info','debug'].includes(message.type())) { routineLogs.push(message.text()); } if(process.env.TERRAIN_VERBOSE) { console.log(message.text()); } if(message.type() === 'error') { errors.push(message.text()); } });
    await page.addInitScript(() => {
      const original = WebGL2RenderingContext.prototype.compileShader;
      WebGL2RenderingContext.prototype.compileShader = function(shader) {
        original.call(this,shader);
        if(!this.getShaderParameter(shader,this.COMPILE_STATUS)) { console.error(this.getShaderInfoLog(shader)); }
      };
      const link = WebGL2RenderingContext.prototype.linkProgram;
      WebGL2RenderingContext.prototype.linkProgram = function(program) {
        link.call(this,program);
        if(!this.getProgramParameter(program,this.LINK_STATUS)) { console.error(this.getProgramInfoLog(program)); }
      };
    });
    await page.goto(`${url}?backend=${backend}`);
    await expect.poll(async () => await page.locator('body').getAttribute('data-ready') === 'true' || await page.locator('#failure').isVisible(),{timeout:90000}).toBeTruthy();
    if(await page.locator('#failure').isVisible()) {
      const failure = await page.locator('#failure').textContent();
      if(backend === 'webgpu' && /adapter|not supported|No GPU|device creation|createDevice/i.test(failure)) {
        reports.push(`WebGPU unavailable: ${failure}`); await page.close(); continue;
      }
      throw new Error(`${backend}: ${failure}; ${errors.join('\n')}`);
    }
    await expect(page.locator('body')).toHaveAttribute('data-backend',backend);
    await expect(page.locator('#stats')).toContainText('triangles',{timeout:20000});
    await capture(`${backend}-overview`);
    await page.locator('summary').filter({hasText:'CONFIG'}).click();
    await page.selectOption('#diagnostic','2');
    await capture(`${backend}-lod-grid`);
    await page.locator('#ground').click();
    await page.locator('#world').click({position:{x:700,y:350}});
    assert.equal(await page.evaluate(() => Boolean(document.activeElement?.closest('aside'))),false,'Movement must reach the canvas rather than CONFIG controls');
    await page.mouse.move(700,350);
    await page.mouse.down({button:'right'});
    await expect(page.locator('#world')).toHaveCSS('cursor','none');
    await page.keyboard.down('w');
    await page.waitForTimeout(1200);
    await page.keyboard.up('w');
    await page.mouse.up({button:'right'});
    await expect(page.locator('#world')).not.toHaveCSS('cursor','none');
    await capture(`${backend}-grazing`);
    await page.mouse.move(600,400); await page.mouse.down(); await page.mouse.move(1000,480,{steps:15}); await page.mouse.up();
    await page.locator('#overview').click();
    await page.mouse.move(600,400);
    // Actual wheel events exercise scroll zoom and platform-modifier panning.
    await page.mouse.wheel(100,40);
    const panModifier = await page.evaluate(() => /Mac/i.test(navigator.platform) ? 'Meta' : 'Alt');
    await page.keyboard.down(panModifier);
    await page.mouse.wheel(60,20);
    await page.keyboard.up(panModifier);
    await capture(`${backend}-pan`);
    await page.locator('#tour').check(); await page.waitForTimeout(1500); await page.locator('#tour').uncheck();
    await page.locator('#overview').click();
    for(const tolerance of ['0.5','4','12','2']) {
      await page.locator('#tolerance').fill(tolerance); await page.locator('#tolerance').dispatchEvent('input');
      await page.waitForTimeout(150);
    }
    await page.setViewportSize({width:800,height:600});
    await capture(`${backend}-resize`);
    if(backend === 'webgpu' && !built) {
      // One-shot asynchronous readback is a test oracle, never a runtime frame operation.
      const parity = await page.evaluate(async enginePath => {
        const engine = await import(enginePath);
        const canvas = document.createElement('canvas');
        const gpu = await engine.createGpuCanvasDevice(canvas,{backend:'webgpu',fallbackToWebGl:false});
        const asset = await engine.loadTerrain(new URL('dragon-pit/terrain.json',document.baseURI));
        const terrain = new engine.TerrainRenderer(gpu,asset);
        let maxDifference = 0;
        try {
          await terrain.ready();
          for(const eye of [[1000,900,1600],[0,240,30],[100000,100000,100000]]) {
            const view = {eye,projectionScale:700,tolerance:2,target:[0,220,0],targetRadius:90};
            terrain.prepare(new Float32Array(16),view,0); gpu.submit();
            const data = await terrain.descriptors.readAsync();
            const output = new Float32Array(data.buffer,data.byteOffset,data.byteLength/4);
            const cpu = engine.selectTerrain(asset,view);
            const commands = await terrain.indirect.readAsync();
            const draws = new Uint32Array(commands.buffer,commands.byteOffset,commands.byteLength/4);
            cpu.forEach((selection,i) => {
              const values = [asset.patches[i].x,asset.patches[i].z,selection.detail,0,...selection.edges];
              values.forEach((value,j) => { maxDifference = Math.max(maxDifference,Math.abs(value - output[i*8 + j])); });
              if(draws[i*5] !== terrain.grid.ranges[Math.floor(output[i*8 + 2])].count || draws[i*5 + 4] !== 0) {
                throw new Error('Compute indirect draw mismatch');
              }
            });
          }
          return {maxDifference,indirectFirstInstance:gpu.features.has('indirect-first-instance')};
        } finally { terrain.destroy(); terrain.destroy(); gpu.destroy(); }
      },`/@fs/${fileURLToPath(new URL('../../../src/index.ts',import.meta.url))}`);
      assert.ok(parity.maxDifference < 0.001,`CPU/compute descriptor parity: ${parity.maxDifference}`);
      reports.push('Indirect firstInstance stays zero; optional feature is not required.');
      reports.push(`CPU/WebGPU descriptor and indirect-command parity passed; max difference ${parity.maxDifference}`);
    }
    assert.deepEqual(errors,[],`${backend} no page/console errors`);
    if(built) { assert.deepEqual(routineLogs,[],`${backend} production emits no routine logs`); }
    else { assert.ok(routineLogs.some(message => message.startsWith('Terrain Lab ready:')),`${backend} development startup diagnostics`); }
    reports.push(`${backend} passed. ${await page.locator('#stats').textContent()}`);
    await page.close();
  }
  success = true;
} finally {
  if(page && !success && !page.isClosed()) { await capture('failure').catch(() => {}); }
  await writeFile(new URL('index.html',directory),`<!doctype html><meta charset="utf-8"><title>Terrain Lab captures</title><style>body{background:#17252a;color:#edf5ee;font:16px system-ui;max-width:1200px;margin:30px auto}img{width:100%}pre{white-space:pre-wrap}</style><h1>Terrain Lab · ${built ? 'Production' : 'Development'} · ${success ? 'Passed' : 'Failed'}</h1><p>Screenshots validate appearance, not hardware performance.</p><pre>${reports.join('\n')}</pre>${captures.map(name => `<h2>${name}</h2><img src="${name}.png" alt="${name}">`).join('')}`);
  await browser?.close(); await server.close();
  console.log(`Terrain gallery: ${fileURLToPath(new URL('index.html',directory))}`);
  console.log(reports.join('\n'));
}
async function capture(name) {
  await page.screenshot({path:fileURLToPath(new URL(`${name}.png`,directory))}); captures.push(name);
}
