// Copyright (c) 2026 Trent Polack. Licensed under the MIT License.
import assert from 'node:assert/strict';
import {createServer} from 'node:http';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {transformWithOxc} from 'vite';
import {chromium, expect} from '@playwright/test';

// Build with the editor enabled first. Serve the actual built host and its chunks;
// a deterministic runtime fixture isolates URL/transport behavior from game GPU startup.
// Compile native source fixtures with the same transformer used by the browser build.
const sessionPath = new URL('../site/src/workspace/game-preview-session.ts', import.meta.url);
const protocolPath = new URL('../site/src/workspace/preview-protocol.ts', import.meta.url);
const session = await transformWithOxc(await readFile(sessionPath, 'utf8'), fileURLToPath(sessionPath));
const protocol = await transformWithOxc(await readFile(protocolPath, 'utf8'), fileURLToPath(protocolPath));
const built = new URL('../dist/', import.meta.url);
const server = createServer(async (request, response) => {
  const url = new URL(request.url, 'http://localhost');
  try {
    if(url.pathname === '/editor/owner.html') {
      response.setHeader('Content-Type', 'text/html');
      response.end('<!doctype html><button id="play">Play</button><div id="stage"></div><script type="module">import {GamePreviewSession} from "/session.js"; window.preview=new GamePreviewSession(document.querySelector("#stage"), state=>window.state=state); addEventListener("message",event=>preview.receive(event)); document.querySelector("#play").onclick=()=>preview.start({name:"Fixture",preview:"/games/fixture/"},"browser",null);</script>');
    } else if(url.pathname === '/session.js') {
      response.setHeader('Content-Type', 'text/javascript');
      response.end(session.code);
    } else if(url.pathname === '/preview-protocol.ts') {
      response.setHeader('Content-Type', 'text/javascript');
      response.end(protocol.code);
    } else if(url.pathname === '/games/fixture/') {
      response.setHeader('Content-Type', 'text/html');
      response.end('<!doctype html><script>parent.postMessage({channel:"joy-editor-runtime",state:"CONNECTED",paused:false},location.origin)</script>Fixture game');
    } else if(/^\/editor\/(preview\.html|assets\/[a-zA-Z0-9_.-]+)$/.test(url.pathname)) {
      response.setHeader('Content-Type', url.pathname.endsWith('.html') ? 'text/html' : 'text/javascript');
      response.end(await readFile(new URL(url.pathname.slice('/editor/'.length), built)));
    } else {
      response.writeHead(404).end();
    }
  } catch(error) {
    response.writeHead(500).end(String(error));
  }
});
let browser;
try {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const address = server.address();
  browser = await chromium.launch({executablePath: process.env.CAPTURE_EXECUTABLE});
  const page = await browser.newPage();
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(`http://127.0.0.1:${address.port}/editor/owner.html`);
  const opened = page.waitForEvent('popup');
  await page.locator('#play').click();
  const popup = await opened;
  popup.on('pageerror', error => errors.push(error.message));
  assert.equal(new URL(popup.url()).pathname, '/editor/preview.html');
  await expect.poll(() => page.evaluate(() => window.state?.connected)).toBe(true);
  assert.equal(await page.evaluate(() => window.preview.receive(new MessageEvent('message', {
    origin:location.origin, source:window.preview.external,
    data:{channel:'joy-editor-session',session:'retired-session',type:'runtime',data:{channel:'joy-editor-runtime',paused:true}}
  }))), false, 'Retired session reports must be ignored.');
  await page.evaluate(() => { window.preview.connected = false; window.state = {connected:false}; });
  await expect.poll(() => page.evaluate(() => window.state?.connected)).toBe(true);
  await page.evaluate(() => window.preview.stop());
  await expect.poll(() => popup.isClosed()).toBe(true);
  assert.deepEqual(errors, []);
  console.log(`PASS built preview host entry/chunks, connection and stop: ${fileURLToPath(built)}`);
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
