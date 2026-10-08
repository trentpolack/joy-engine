// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
const {app, BrowserWindow, dialog, ipcMain, shell} = require('electron');
const path = require('node:path');
const {realpath} = require('node:fs/promises');
const {randomUUID} = require('node:crypto');

// The desktop editor is hosted by a game workspace whose Vite config mounts the editor at
// /editor/, its games at /games/<id>/ and the editor file service. Inside the JoyGames
// monorepo that workspace is three levels up (joy-engine/editor/desktop); a standalone joy-editor checkout (or an
// installed package) names it explicitly with these environment variables.
const WORKSPACE_ROOT_ENV = 'JOY_EDITOR_WORKSPACE';
const WORKSPACE_VITE_CONFIG_ENV = 'JOY_EDITOR_WORKSPACE_VITE_CONFIG';
const DEFAULT_WORKSPACE_VITE_CONFIG = 'scripts/development/vite-site.config.mjs';
const PROJECTS_DIRECTORY = 'projects';
const root = process.env[WORKSPACE_ROOT_ENV] ? path.resolve(process.env[WORKSPACE_ROOT_ENV]) : path.resolve(__dirname, '../../..');
const workspaceViteConfig = path.resolve(root, process.env[WORKSPACE_VITE_CONFIG_ENV] ?? DEFAULT_WORKSPACE_VITE_CONFIG);
const desktopToken = randomUUID();
let server;
let window;
const previews = new Set();
const PREVIEW_PATH = '/editor/preview.html';
// Validation instances use a separate preference store and never surface windows.
if(process.env.JOY_EDITOR_TEST_USER_DATA) {
  app.setPath('userData', process.env.JOY_EDITOR_TEST_USER_DATA);
}

/** Repository desktop entry. Vite and all watchers belong to this process. */
app.whenReady().then(async () => {
  process.env.JOY_EDITOR_DESKTOP_TOKEN = desktopToken;
  const {createServer} = await import('vite');
  server = await createServer({configFile: workspaceViteConfig, server: {host: '127.0.0.1', port: 0, open: false}});
  await server.listen();
  const address = server.httpServer.address();
  const origin = `http://127.0.0.1:${address.port}`;
  window = new BrowserWindow({
    show: process.env.JOY_EDITOR_TEST_HIDDEN !== '1', width: 1680, height: 1050, minWidth: 900, minHeight: 650,
    backgroundColor: '#101722', title: 'Joy Editor', autoHideMenuBar: true,
    webPreferences: {preload: path.join(__dirname, 'workspace-preload.cjs'), nodeIntegration: false, contextIsolation: true, sandbox: true}
  });
  window.webContents.setWindowOpenHandler(({url}) => {
    const target = new URL(url);
    if(target.origin === origin && target.pathname === PREVIEW_PATH) {
      const game = target.searchParams.get('game') ?? '';
      if(!/^\/games\/[a-z0-9-]+\/$/.test(game)) { return {action: 'deny'}; }
      const width = Math.max(320, Math.min(8192, Number(target.searchParams.get('width')) || 1280));
      const height = Math.max(240, Math.min(8192, Number(target.searchParams.get('height')) || 800));
      return {action: 'allow', overrideBrowserWindowOptions: {show: process.env.JOY_EDITOR_TEST_HIDDEN !== '1', useContentSize: true, width, height, title: 'Joy Game Preview', backgroundColor: '#101722', autoHideMenuBar: true,
        webPreferences: {nodeIntegration: false, contextIsolation: true, sandbox: true}}};
    }
    if(target.origin === origin && target.pathname.startsWith('/games/')) {
      void shell.openExternal(url);
    }
    return {action: 'deny'};
  });
  window.webContents.on('did-create-window', child => {
    previews.add(child);
    child.webContents.setWindowOpenHandler(() => ({action: 'deny'}));
    child.webContents.on('will-navigate', (event, url) => {
      const target = new URL(url);
      if(target.origin !== origin || target.pathname !== PREVIEW_PATH) {
        event.preventDefault();
      }
    });
    child.on('closed', () => previews.delete(child));
  });
  window.on('closed', () => { for(const child of previews) { child.close(); } });
  window.webContents.on('will-navigate', (event, url) => {
    if(!url.startsWith(`${origin}/editor/`)) {
      event.preventDefault();
    }
  });
  ipcMain.handle('joy-editor:open-browser', async (event, url) => {
    if(event.sender !== window.webContents || !event.senderFrame.url.startsWith(`${origin}/editor/`)) {
      throw new Error('Unsupported preview owner.');
    }
    const target = new URL(url);
    if(target.origin !== origin || target.pathname !== PREVIEW_PATH || !/^\/games\/[a-z0-9-]+\/$/.test(target.searchParams.get('game') ?? '')) {
      throw new Error('Unsupported browser preview.');
    }
    await shell.openExternal(target.href);
  });
  ipcMain.handle('joy-editor:choose-project', async event => {
    if(event.sender !== window.webContents || !event.senderFrame.url.startsWith(`${origin}/editor/`)) {
      return null;
    }
    const result = await dialog.showOpenDialog(window, {title: 'Open a Joy project folder', defaultPath: path.join(root, PROJECTS_DIRECTORY), properties: ['openDirectory']});
    if(result.canceled) {
      return null;
    }
    const selected = await realpath(result.filePaths[0]);
    const response = await fetch(`${origin}/__joy_editor/register-project`, {
      method: 'POST',
      headers: {'Content-Type': 'application/json', 'X-Joy-Editor': 'workspace', 'X-Joy-Editor-Desktop': desktopToken, Origin: origin},
      body: JSON.stringify({path: selected})
    });
    const project = await response.json();
    if(!response.ok) {
      await dialog.showMessageBox(window, {type: 'error', message: project.error ?? 'This is not a compatible Joy project.'});
      return null;
    }
    return project;
  });
  await window.loadURL(`${origin}/editor/`);
}).catch(error => {
  dialog.showErrorBox('Joy Editor could not start', error.stack ?? String(error));
  app.quit();
});
app.on('window-all-closed', () => app.quit());
app.on('before-quit', () => {
  ipcMain.removeHandler('joy-editor:choose-project');
  ipcMain.removeHandler('joy-editor:open-browser');
  void server?.close();
});
