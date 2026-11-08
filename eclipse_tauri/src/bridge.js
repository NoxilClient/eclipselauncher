'use strict';
// Same `window.eclipse` API the Electron preload exposed, backed by Tauri commands.
(function () {
  const tauri = window.__TAURI__ || {};
  const core = tauri.core || {};
  const invoke = (command, args) => core.invoke(command, args || {});
  const listeners = [];

  if (core.event && typeof core.event.listen === 'function') {
    core.event.listen('eclipse-progress', event => {
      const payload = event && event.payload;
      for (const listener of listeners) {
        try { listener(payload); } catch {}
      }
    });
  }

  window.eclipse = {
    status: () => invoke('eclipse_status'),
    install: version => invoke('eclipse_install', { version }),
    launch: (version, options) => invoke('eclipse_launch', { version, options: options || {} }),
    getMods: version => invoke('eclipse_get_mods', { version }),
    removeMod: (version, name) => invoke('eclipse_remove_mod', { version, name }),
    searchMods: opts => invoke('eclipse_search_mods', { opts: opts || {} }),
    downloadMod: opts => invoke('eclipse_download_mod', { opts: opts || {} }),
    onProgress: callback => {
      listeners.push(callback);
      return () => {
        const index = listeners.indexOf(callback);
        if (index >= 0) listeners.splice(index, 1);
      };
    }
  };
})();
