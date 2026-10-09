// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
import assert from 'node:assert/strict';
import {mkdir,writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {chromium} from '@playwright/test';
import {createServer,preview} from 'vite';
const built = process.argv.includes('--built');
const directory = new URL(`../../../../artifacts/captures/${Date.now()}-form-building/`,import.meta.url);
const configFile = fileURLToPath(new URL('../vite.config.js',import.meta.url));
const server = built ? await preview({configFile,preview:{host:'127.0.0.1',port:0}})
  : await createServer({configFile,server:{host:'127.0.0.1',port:0}});
const browser = await chromium.launch({executablePath:process.env.CAPTURE_EXECUTABLE ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:false,args:['--enable-unsafe-webgpu']});
const results = [];
let failure;
await mkdir(directory,{recursive:true});
try {
  if(!built) {
    await server.listen();
  }
  const address = server.httpServer.address();
  assert.ok(address && typeof address !== 'string');
  for(const backend of ['webgl','webgpu']) {
    const page = await browser.newPage({viewport:{width:1440,height:1000}});
    const errors = [];
    const logs = [];
    page.on('pageerror',error => errors.push(error.message));
    page.on('console',message => {
      if(message.type() === 'error') {
        errors.push(message.text());
      }
      if(['info','log','debug'].includes(message.type())) {
        logs.push(message.text());
      }
    });
    await page.goto(`http://127.0.0.1:${address.port}/?backend=${backend}`);
    await page.waitForFunction(() => document.body.dataset.ready === 'true' || document.querySelector('#failure').textContent,{timeout:60000});
    assert.equal(await page.locator('#failure').textContent(),'');
    const actual = await page.evaluate(() => window.buildingExample.snapshot().backend);
    results.push({requestedBackend:backend,actualBackend:actual});
    for(const preset of ['cottage','warehouse','apartments']) {
      await page.selectOption('#preset',preset);
      await page.waitForTimeout(250);
      results.push({preset,...await page.evaluate(() => window.buildingExample.snapshot())});
      await page.screenshot({path:fileURLToPath(new URL(`${backend}-${preset}.png`,directory))});
    }
    await page.evaluate(() => window.buildingExample.regenerate({width:30,depth:30,height:30,floors:8,window_spacing:1.5,door_spacing:3,roof_rise:5}));
    await page.locator('#frame').click();
    await page.waitForTimeout(1000);
    results.push({case:'maximum',...await page.evaluate(() => window.buildingExample.snapshot())});
    await page.screenshot({path:fileURLToPath(new URL(`${backend}-maximum.png`,directory))});
    await page.selectOption('#preset','apartments');
    await page.getByLabel('roof_shape',{exact:true}).fill('1');
    await page.getByLabel('roof_shape',{exact:true}).dispatchEvent('change');
    await page.getByLabel('roof_rise',{exact:true}).fill('4');
    await page.getByLabel('roof_rise',{exact:true}).dispatchEvent('change');
    await page.waitForTimeout(250);
    results.push({case:'roof-edit',...await page.evaluate(() => window.buildingExample.snapshot())});
    await page.screenshot({path:fileURLToPath(new URL(`${backend}-roof-edit.png`,directory))});
    await page.evaluate(() => {
      const example = window.buildingExample;
      for(let index = 0; index < 50; index++) {
        example.regenerate({width:10,height:8,floors:3,random_seed:index + 1});
      }
    });
    assert.equal(await page.evaluate(() => window.buildingExample.snapshot().instances),4);
    const before = await page.evaluate(() => window.buildingExample.snapshot().vertices);
    await page.getByLabel('width',{exact:true}).fill('');
    await page.getByLabel('width',{exact:true}).dispatchEvent('change');
    assert.match(await page.locator('#failure').textContent(),/finite/);
    assert.equal(await page.evaluate(() => window.buildingExample.snapshot().vertices),before);
    await page.selectOption('#preset','cottage');
    await page.evaluate(() => window.buildingExample.dispose());
    assert.equal(await page.evaluate(() => window.buildingExample),undefined);
    assert.deepEqual(errors,[]);
    if(built) {
      assert.deepEqual(logs,[],'Production should have no routine logs');
    }
    await page.goto(`http://127.0.0.1:${address.port}/?backend=${backend}&mode=baked`);
    await page.waitForFunction(() => document.body.dataset.ready === 'true' || document.querySelector('#failure').textContent,{timeout:60000});
    assert.equal(await page.locator('#failure').textContent(),'');
    const baked = await page.evaluate(() => window.buildingExample.snapshot());
    assert.equal(baked.evaluations,0);
    assert.equal(baked.vertices,198);
    results.push({case:'baked',...baked});
    await page.screenshot({path:fileURLToPath(new URL(`${backend}-baked.png`,directory))});
    await page.close();
  }
} catch(error) {
  failure = String(error);
  process.exitCode = 1;
} finally {
  await browser.close();
  await server.close();
  await writeFile(new URL('results.json',directory),JSON.stringify({built,browser:process.env.CAPTURE_EXECUTABLE ?? 'Google Chrome',results,failure},null,2));
  const images = ['webgl','webgpu'].flatMap(backend => ['cottage','warehouse','apartments','roof-edit','maximum','baked'].map(name => `${backend}-${name}.png`));
  await writeFile(new URL('index.html',directory),`<!doctype html><title>FORM building captures</title><style>body{background:#142330;color:white;font:16px system-ui}img{width:95%;max-width:1000px}figure{margin:20px}</style><h1>FORM building captures</h1>${images.map(name => `<figure><figcaption>${name}</figcaption><img src="${name}"></figure>`).join('')}`);
  console.info(`Capture gallery: ${fileURLToPath(new URL('index.html',directory))}`);
  if(failure) {
    console.error(failure);
  }
}
