// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.
const {contextBridge, ipcRenderer} = require('electron');
contextBridge.exposeInMainWorld('joyDesktop', {
  openBrowser: url => ipcRenderer.invoke('joy-editor:open-browser', url),
  chooseProject: () => ipcRenderer.invoke('joy-editor:choose-project')
});
