'use strict';
const { contextBridge, ipcRenderer } = require('electron');

const listeners = new Set();
ipcRenderer.on('bc:signal', (_e, msg) => {
  for (const fn of listeners) {
    try { fn(msg); } catch (err) { console.error(err); }
  }
});

contextBridge.exposeInMainWorld('bc', {
  call: (method, params) => ipcRenderer.invoke('bc:call', method, params || {}),
  onSignal: (fn) => { listeners.add(fn); return () => listeners.delete(fn); },
  info: () => ipcRenderer.invoke('bc:info'),
  openFile: (opts) => ipcRenderer.invoke('bc:openFile', opts || {}),
  openFolder: () => ipcRenderer.invoke('bc:openFolder'),
  saveJson: (name, data) => ipcRenderer.invoke('bc:saveJson', name, data),
  loadJson: () => ipcRenderer.invoke('bc:loadJson'),
  readImage: (path) => ipcRenderer.invoke('bc:readImage', path),
  applyAppSettings: (s) => ipcRenderer.invoke('bc:applyAppSettings', s),
});
