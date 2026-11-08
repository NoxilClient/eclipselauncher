const { app, BrowserWindow, shell, ipcMain } = require('electron');
const path = require('path');
const launcher = require('./launcher');

function safeLaunchOptions(options) {
  const raw = options || {};
  return {
    username: typeof raw.username === 'string' ? raw.username : 'Player',
    ramMb: Number(raw.ramMb) || 4096,
    javaPath: typeof raw.javaPath === 'string' ? raw.javaPath.trim() : ''
  };
}

ipcMain.handle('eclipse:status', () => launcher.status());

ipcMain.handle('eclipse:getMods', (event, version) => {
  try {
    return { ok: true, mods: launcher.getMods(String(version || '')) };
  } catch (err) {
    return { ok: false, error: String((err && err.message) || err), mods: [] };
  }
});

ipcMain.handle('eclipse:removeMod', (event, version, name) => {
  try {
    return { ok: true, ...launcher.removeMod(String(version || ''), String(name || '')) };
  } catch (err) {
    return { ok: false, error: String((err && err.message) || err) };
  }
});

ipcMain.handle('eclipse:searchMods', async (event, opts) => {
  try {
    const { source, query, version, loader, apiKey } = opts || {};
    const results = await launcher.searchMods({ source, query, version, loader, apiKey });
    const installed = new Set(version ? launcher.installedProjects(String(version)) : []);
    const normalize = value => String(value || '').toLowerCase().replace(/[^a-z0-9]/g, '');
    const files = version ? launcher.getMods(String(version)).map(m => normalize(m.name)) : [];
    return {
      ok: true,
      results: results.map(r => {
        const byId = installed.has(String(r.source) + ':' + String(r.id));
        const slug = normalize(r.slug);
        const byFile = slug.length > 2 && files.some(f => f.startsWith(slug));
        return { ...r, installed: byId || byFile };
      })
    };
  } catch (err) {
    return { ok: false, error: String((err && err.message) || err), results: [] };
  }
});

ipcMain.handle('eclipse:downloadMod', async (event, opts) => {
  try {
    const { source, id, version, loader, apiKey } = opts || {};
    const target = String(version || '');
    const res = await launcher.downloadMod({ source, id, version, loader, apiKey }, message =>
      event.sender.send('eclipse:progress', { version: target, message }));
    return { ok: true, ...res };
  } catch (err) {
    return { ok: false, error: String((err && err.message) || err) };
  }
});

ipcMain.handle('eclipse:install', (event, version) => runInstall(event, version));

ipcMain.handle('eclipse:launch', async (event, version, options) => {
  const target = String(version || '');
  try {
    return await launcher.launch(target, safeLaunchOptions(options), message =>
      event.sender.send('eclipse:progress', { version: target, message }));
  } catch (err) {
    return { ok: false, error: String((err && err.message) || err) };
  }
});

async function runInstall(event, version) {
  try {
    const result = await launcher.install(String(version || ''), message =>
      event.sender.send('eclipse:progress', { version, message }));
    return { ok: true, ...result };
  } catch (err) {
    return { ok: false, error: String((err && err.message) || err) };
  }
}

function createWindow() {
  const win = new BrowserWindow({
    width: 1040, height: 660, minWidth: 860, minHeight: 560,
    backgroundColor: '#050507',
    autoHideMenuBar: true,
    title: 'DivineClient',
    icon: path.join(__dirname, 'src', 'assets', 'icon.png'),
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
      preload: path.join(__dirname, 'preload.js')
    }
  });
  win.loadFile(path.join(__dirname, 'src', 'index.html'));
  win.webContents.setWindowOpenHandler(({ url }) => { shell.openExternal(url); return { action: 'deny' }; });
}

app.whenReady().then(() => {
  createWindow();
  app.on('activate', () => { if (BrowserWindow.getAllWindows().length === 0) createWindow(); });
});
app.on('window-all-closed', () => { if (process.platform !== 'darwin') app.quit(); });
