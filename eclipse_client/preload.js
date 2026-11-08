'use strict';
const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('eclipse', {
  status: () => ipcRenderer.invoke('eclipse:status'),
  install: (version) => ipcRenderer.invoke('eclipse:install', version),
  launch: (version, options) => ipcRenderer.invoke('eclipse:launch', version, options || {}),
  getMods: (version) => ipcRenderer.invoke('eclipse:getMods', version),
  removeMod: (version, name) => ipcRenderer.invoke('eclipse:removeMod', version, name),
  searchMods: (opts) => ipcRenderer.invoke('eclipse:searchMods', opts || {}),
  downloadMod: (opts) => ipcRenderer.invoke('eclipse:downloadMod', opts || {}),
  onProgress: (callback) => {
    const listener = (_event, payload) => callback(payload);
    ipcRenderer.on('eclipse:progress', listener);
    return () => ipcRenderer.removeListener('eclipse:progress', listener);
  }
});
