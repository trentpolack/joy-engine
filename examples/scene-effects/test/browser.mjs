// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import { access, mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { platform, release, cpus, totalmem } from 'node:os';
import { chromium, expect } from '@playwright/test';
import { createServer, preview } from 'vite';

const EFFECTS = ['depthAwareBlur','ssao','gtao','ssgi','outlines','taa','motionBlur','ssr','heightFog','clusteredLighting','adaptiveExposure'];
const built = process.argv.includes('--built');
const directory = new URL(`../../../../artifacts/captures/${Date.now()}-scene-effects/`, import.meta.url);
const configFile = fileURLToPath(new URL('../vite.config.js', import.meta.url));
const server = built
  ? await preview({configFile, preview:{host:'127.0.0.1', port:0}})
  : await createServer({configFile, server:{host:'127.0.0.1', port:0}});
const scenarios = [];
const limitations = [];
const hardware = hardwareMetadata();
let browser;
let page;
let success = false;
await mkdir(directory, {recursive:true});

try {
  if(!built) {
    await server.listen();
  }
  const address = server.httpServer.address();
  assert.ok(address && typeof address !== 'string');
  const url = `http://127.0.0.1:${address.port}/`;
  const executablePath = await browserExecutable();
  browser = await chromium.launch({
    executablePath,
    headless:process.env.SCENE_EFFECTS_HEADLESS === '1' || (process.platform !== 'darwin' && process.env.SCENE_EFFECTS_HEADED !== '1'),
    args:['--enable-unsafe-webgpu']
  });
  const backends = process.env.SCENE_EFFECTS_BACKEND ? [process.env.SCENE_EFFECTS_BACKEND] : ['webgl','webgpu'];
  for(const backend of backends) {
    page = await browser.newPage({viewport:{width:1440,height:1000}, deviceScaleFactor:1});
    const errors = [];
    const routineLogs = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('console', message => {
      if(message.type() === 'error') {
        errors.push(message.text());
      }
      if(['log','info','debug'].includes(message.type())) {
        routineLogs.push(message.text());
      }
    });
    await page.addInitScript(() => {
      const compile = WebGL2RenderingContext.prototype.compileShader;
      WebGL2RenderingContext.prototype.compileShader = function(shader) {
        compile.call(this, shader);
        if(!this.getShaderParameter(shader, this.COMPILE_STATUS)) {
          console.error(this.getShaderInfoLog(shader));
        }
      };
      const link = WebGL2RenderingContext.prototype.linkProgram;
      WebGL2RenderingContext.prototype.linkProgram = function(program) {
        link.call(this, program);
        if(!this.getProgramParameter(program, this.LINK_STATUS)) {
          console.error(this.getProgramInfoLog(program));
        }
      };
    });
    await page.goto(`${url}?backend=${backend}`);
    await expect.poll(async () =>
      (await page.locator('body').getAttribute('data-ready')) === 'true' || await page.locator('#failure').isVisible(),
    {timeout:90000}).toBeTruthy();
    if(await page.locator('#failure').isVisible()) {
      const failure = await page.locator('#failure').textContent();
      if(backend === 'webgpu' && /adapter|No GPU|not supported|device creation|createDevice/i.test(failure ?? '')) {
        limitations.push(`WebGPU unavailable: ${failure}`);
        await page.close();
        continue;
      }
      throw new Error(`${backend}: ${failure}\n${errors.join('\n')}`);
    }
    await expect(page.locator('body')).toHaveAttribute('data-backend', backend);
    await expect(page.locator('#stats')).toContainText('CPU render submission', {timeout:30000});
    const browserMetadata = await deviceMetadata(page);

    await capture(`${backend}-baseline`, backend, browserMetadata);
    for(const effect of EFFECTS) {
      await page.evaluate(key => {
        const example = window.sceneEffectsExample;
        example.baseline();
        example.setTime(2);
        example.setEffect(key, true);
      }, effect);
      await capture(`${backend}-${effect}`, backend, browserMetadata);
    }
    await page.evaluate(() => { window.sceneEffectsExample.baseline(); window.sceneEffectsExample.setBloom(true); });
    await capture(`${backend}-bloom`, backend, browserMetadata);

    // Real controls exercise exclusivity and replacement; snapshots assert the public state.
    // Separate real object and camera motion before/after the velocity blur pass.
    for(const [motionName, objectMotion, cameraMotion] of [['object',true,false],['camera',false,true]]) {
      for(const enabled of [false,true]) {
        await page.evaluate(({enabled,objectMotion,cameraMotion}) => {
          const example = window.sceneEffectsExample;
          example.baseline(); example.setTime(2); example.setMotion(objectMotion,cameraMotion);
          example.setEffect('motionBlur',enabled); example.setPaused(false);
        }, {enabled,objectMotion,cameraMotion});
        await capture(`${backend}-motion-${motionName}-${enabled ? 'blur' : 'baseline'}`, backend, browserMetadata);
      }
    }
    await page.evaluate(() => {const example=window.sceneEffectsExample; example.setPaused(true); example.setMotion(false,false); example.setTime(2); example.baseline();});
    await page.locator('#ssao').check();
    await page.locator('#gtao').check();
    await expect(page.locator('#ssao')).not.toBeChecked();
    await expect(page.locator('#gtao')).toBeChecked();
    await page.locator('#combined').click();
    for(const quality of ['low','medium','high']) {
      await page.selectOption('#quality', quality);
      await capture(`${backend}-combined-${quality}`, backend, browserMetadata);
    }
    await page.selectOption('#quality', 'medium');
    await page.locator('#objectMotion').check();
    await page.locator('#cameraMotion').check();
    await page.locator('#paused').uncheck();
    await capture(`${backend}-moving`, backend, browserMetadata);
    await page.locator('#paused').check();
    await page.locator('#cut').click();
    await capture(`${backend}-camera-cut`, backend, browserMetadata);
    await page.locator('#replace').click();
    await capture(`${backend}-scene-replaced`, backend, browserMetadata);
    await page.locator('#reset').click();
    await page.locator('#step').click();
    await capture(`${backend}-step-reset`, backend, browserMetadata);
    await page.locator('#resize').click();
    await page.setViewportSize({width:1000,height:740});
    await capture(`${backend}-resize`, backend, browserMetadata);
    await page.locator('#resize').click();
    await page.setViewportSize({width:1440,height:1000});
    for(let cycle = 0; cycle < 3; cycle++) {
      await page.locator('#baseline').click();
      await page.locator('#combined').click();
    }
    await capture(`${backend}-toggle-stress`, backend, browserMetadata);
    await page.locator('#baseline').click();
    await capture(`${backend}-disabled-after-effects`, backend, browserMetadata);
    assert.deepEqual(errors, [], `${backend}: no shader, page or console errors`);
    if(built) {
      assert.deepEqual(routineLogs, [], `${backend}: no routine production logs`);
    } else {
      assert.ok(routineLogs.some(message => message.startsWith('Scene Effects Lab ready:')), `${backend}: development startup diagnostics`);
    }
    await page.evaluate(() => { window.sceneEffectsExample.dispose(); });
    await page.close();
  }
  assert.ok(scenarios.length > 0, 'At least one backend must complete the capture matrix.');
  success = true;
} finally {
  if(page && !page.isClosed() && !success) {
    await page.screenshot({path:fileURLToPath(new URL('failure.png', directory))}).catch(() => {});
  }
  const report = {success, mode:built ? 'production' : 'development', createdAt:new Date().toISOString(),
    hardware, browserVersion:browser ? await browser.version() : null,
    gpuTiming:'Unavailable. CPU submission and RAF cadence are separate measurements, neither is GPU time.',
    limitations, scenarios};
  await writeFile(new URL('report.json', directory), JSON.stringify(report, null, 2));
  await writeFile(new URL('index.html', directory), gallery(report));
  await browser?.close();
  await server.close();
  console.log(`Scene effects gallery: ${fileURLToPath(new URL('index.html', directory))}`);
  console.log(`Scene effects metadata: ${fileURLToPath(new URL('report.json', directory))}`);
}

async function capture(name, backend, browserMetadata) {
  const snapshot = await page.evaluate(async () => {
    await window.sceneEffectsExample.sample(60);
    return window.sceneEffectsExample.sample(120);
  });
  assert.ok(snapshot.timing.cpuRenderSubmission.samples > 0);
  assert.ok(snapshot.timing.rafCadence.samples > 0);
  assert.equal(snapshot.timing.gpuExecution, null);
  await expect(page.locator('#failure')).toBeHidden();
  await page.screenshot({path:fileURLToPath(new URL(`${name}.png`, directory))});
  scenarios.push({name, backend, browser:browserMetadata, ...snapshot});
}

async function browserExecutable() {
  if(process.env.CAPTURE_EXECUTABLE) {
    return process.env.CAPTURE_EXECUTABLE;
  }
  const nativeChrome = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
  if(process.platform === 'darwin') {
    try {
      await access(nativeChrome);
      return nativeChrome;
    } catch {
      limitations.push('Installed Google Chrome unavailable; using bundled Playwright Chromium.');
    }
  }
  return undefined;
}

async function deviceMetadata(activePage) {
  return activePage.evaluate(async () => {
    const canvas = document.querySelector('#world');
    const gl = canvas.getContext('webgl2');
    const debug = gl?.getExtension('WEBGL_debug_renderer_info');
    let webgpuAdapter = null;
    if(navigator.gpu) {
      const adapter = await navigator.gpu.requestAdapter();
      if(adapter) {
        const info = adapter.info;
        webgpuAdapter = {vendor:info.vendor, architecture:info.architecture, device:info.device, description:info.description};
      }
    }
    return {userAgent:navigator.userAgent, platform:navigator.platform, hardwareConcurrency:navigator.hardwareConcurrency,
      webglRenderer:debug ? gl.getParameter(debug.UNMASKED_RENDERER_WEBGL) : null,
      webglVendor:debug ? gl.getParameter(debug.UNMASKED_VENDOR_WEBGL) : null, webgpuAdapter};
  });
}

function hardwareMetadata() {
  const result = {platform:platform(), release:release(), cpuModel:cpus()[0]?.model ?? null,
    logicalCpuCount:cpus().length, memoryBytes:totalmem(), graphics:[]};
  if(process.platform === 'darwin') {
    try {
      const display = JSON.parse(execFileSync('/usr/sbin/system_profiler', ['SPDisplaysDataType','-json'], {timeout:15000, encoding:'utf8'}));
      result.graphics = (display.SPDisplaysDataType ?? []).map(device => ({
        chipset:device.sppci_model, vendor:device.spdisplays_vendor, metal:device.spdisplays_metal, cores:device.sppci_cores
      }));
    } catch {
      result.graphics = [];
    }
  }
  return result;
}
function escapeHtml(value) {
  return String(value).replaceAll('&','&amp;').replaceAll('<','&lt;').replaceAll('>','&gt;').replaceAll('"','&quot;');
}
function gallery(report) {
  return `<!doctype html><meta charset="utf-8"><title>Scene effects captures</title>
<style>body{background:#151a24;color:#edf1fa;font:15px system-ui;max-width:1440px;margin:30px auto}img{width:100%}pre{white-space:pre-wrap}h2{margin-top:40px}</style>
<h1>Scene Effects Lab · ${report.mode} · ${report.success ? 'Passed' : 'Failed'}</h1>
<p>Appearance and lifecycle evidence. CPU submission and RAF cadence are distinct; GPU time is unavailable.</p>
<p><a href="report.json">Settings, capabilities, hardware and timing metadata</a></p>
<pre>${escapeHtml(report.limitations.join('\n'))}</pre>
${report.scenarios.map(scenario => `<h2>${escapeHtml(scenario.name)}</h2><p>CPU submission ${scenario.timing.cpuRenderSubmission.meanMilliseconds.toFixed(2)} ms / RAF cadence ${scenario.timing.rafCadence.meanMilliseconds.toFixed(2)} ms</p><img loading="lazy" src="${escapeHtml(scenario.name)}.png" alt="${escapeHtml(scenario.name)}">`).join('\n')}`;
}
