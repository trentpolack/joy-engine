// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
const { app, BrowserWindow, protocol, net } = require('electron');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

/**
 * Own the desktop application lifecycle for a bundled Joy Engine lab.
 * Call once before Electron is ready. The renderer gets a private local origin,
 * without Node integration or filesystem access. root is an absolute build path.
 * @param {{title:string,scheme:string,root:string}} options
 */
function launchDesktopApplication({ title, scheme, root }) {
  protocol.registerSchemesAsPrivileged([
    { scheme, privileges: { standard: true, secure: true, supportFetchAPI: true } },
  ]);

  /** Create an isolated renderer window for bundled content on the registered local origin. */
  function createWindow() {
    const window = new BrowserWindow({
      width: 1480,
      height: 960,
      minWidth: 720,
      minHeight: 600,
      backgroundColor: '#1b293d',
      title,
      autoHideMenuBar: true,
      webPreferences: { nodeIntegration: false, contextIsolation: true, sandbox: true },
    });
    window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
    window.webContents.on('will-navigate', event => event.preventDefault());
    window.loadURL(`${scheme}://app/index.html`);
  }

  app.whenReady().then(() => {
    protocol.handle(scheme, request => {
      try {
        const url = new URL(request.url);
        if(url.host !== 'app') {
          return new Response('Not found', { status: 404 });
        }
        // Resolve decoded paths before checking containment so encoded traversal cannot escape the bundle.
        const resource = path.resolve(root, '.' + decodeURIComponent(url.pathname));
        const relative = path.relative(root, resource);
        if(relative.startsWith('..') || path.isAbsolute(relative)) {
          return new Response('Not found', { status: 404 });
        }
        return net.fetch(pathToFileURL(resource).href);
      } catch {
        return new Response('Invalid resource', { status: 400 });
      }
    });
    createWindow();
    app.on('activate', () => {
      if(BrowserWindow.getAllWindows().length === 0) {
        createWindow();
      }
    });
  });
  app.on('window-all-closed', () => {
    if(process.platform !== 'darwin') {
      app.quit();
    }
  });
}

module.exports = { launchDesktopApplication };
