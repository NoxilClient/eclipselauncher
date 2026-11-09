'use strict';
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const uid = () => Math.random().toString(36).slice(2, 10);
const esc = s => String(s).replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const store = {
  get(k, d) { try { const v = JSON.parse(localStorage.getItem(k)); return v ?? d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch {} }
};

// Fallback se il bridge non c'e' (pagina aperta nel browser): stesse versioni del backend.
const FALLBACK = [
  { version: '1.8.9', loader: 'Forge', label: 'PvP classico', java: 8, installed: false, javaPath: null },
  { version: '1.20.1', loader: 'Forge', label: 'Moderne', java: 17, installed: false, javaPath: null },
  { version: '1.21.11', loader: 'Fabric', label: 'Ultima release', java: 21, installed: false, javaPath: null },
  { version: '26.2', loader: 'Fabric', label: 'Snapshot', java: 25, installed: false, javaPath: null },
  { version: '26.3', loader: 'Fabric', label: 'Snapshot', java: 25, installed: false, javaPath: null }
];

let settings = Object.assign({ ram: 4, java: '', username: 'Player', shooting: true, cfKey: '' }, store.get('ec_settings', {}));
let profiles = (() => {
  try {
    const s = localStorage.getItem('ec_profiles');
    if (s) return JSON.parse(s);
  } catch {}
  return [];
})();
let selected = store.get('ec_selected', null);
let launcherInfo = { dataDir: '', versions: FALLBACK, offline: true };
let editingId = null;
let busy = false;
let startedAt = 0;
let startedVersion = '';

const bridge = () => window.eclipse || null;
const versions = () => (launcherInfo.versions && launcherInfo.versions.length ? launcherInfo.versions : FALLBACK);
const supportedVersion = v => versions().find(x => x.version === v) || null;
const loaderOf = v => { const info = supportedVersion(v); return info ? info.loader : 'Vanilla'; };
const javaOf = v => { const info = supportedVersion(v); return info ? info.java : 21; };

const save = () => { try { localStorage.setItem('ec_profiles', JSON.stringify(profiles || [])); } catch {} store.set('ec_profiles', profiles); store.set('ec_selected', selected); store.set('ec_settings', settings); };
const current = () => profiles.find(p => p.id === selected) || profiles[0] || null;
function toast(msg) { const t = $('#toast'); t.textContent = msg; t.classList.add('show'); clearTimeout(toast.t); toast.t = setTimeout(() => t.classList.remove('show'), 3200); }

function setBusy(value) {
  busy = value;
  $('#playBtn').disabled = value;
  $('#playBtn').textContent = value ? 'Working…' : 'Play';
}

// ---- navigation ----
function go(page) {
  $$('.page').forEach(p => p.classList.toggle('on', p.id === 'page-' + page));
  $$('#nav button').forEach(b => b.classList.toggle('on', b.dataset.page === page));
}
$$('#nav button').forEach(b => b.addEventListener('click', () => {
  // ricarica profili prima di cambiare pagina
  try {
    const s = localStorage.getItem('ec_profiles');
    if (s) {
      const tmp = JSON.parse(s);
      if (Array.isArray(tmp)) profiles = tmp;
    }
  } catch {}
  go(b.dataset.page);
  renderAll();
}));

// ---- launcher backend ----
async function refreshStatus() {
  const api = bridge();
  if (!api) return launcherInfo;
  try {
    const info = await api.status();
    if (info && Array.isArray(info.versions)) launcherInfo = info;
  } catch { toast('Cannot read the launcher install'); }
  return launcherInfo;
}

function pruneProfiles() {
  if (!Array.isArray(profiles)) {
    try { const s = localStorage.getItem('ec_profiles'); if (s) profiles = JSON.parse(s); } catch {}
  }
  let list = Array.isArray(profiles) ? profiles.slice() : [];
  list = list.filter(p => p && p.version);
  if (list.length === 0) {
    try {
      const s = localStorage.getItem('ec_profiles');
      if (s) {
        const tmp = JSON.parse(s);
        if (Array.isArray(tmp)) list = tmp.filter(p => p && p.version);
      }
    } catch {}
  }
  if (list.length === 0) {
    try {
      const s2 = store.get('ec_profiles', []);
      if (Array.isArray(s2)) list = s2.filter(p => p && p.version);
    } catch {}
  }
  profiles = list;
  if (!profiles.length) return;
  if (!selected || !profiles.some(p => p.id === selected)) selected = profiles[0].id;
  try { localStorage.setItem('ec_profiles', JSON.stringify(profiles)); } catch {}
  save();
}

// ---- home ----
function renderHome() {
  const p = current();
  if (!p) return;
  $('#heroName').textContent = p.name;
  const info = supportedVersion(p.version);
  const where = info && info.installed ? 'installed' : 'not installed';
  $('#heroMeta').textContent = `${p.version} · ${p.loader} · ${p.ram} GB RAM · ${where}`;
  $('#heroSelect').innerHTML = profiles.map(x => `<option value="${esc(x.id)}"${x.id === selected ? ' selected' : ''}>${esc(x.name)}</option>`).join('');
  $('#accountName').textContent = settings.username || 'Player';
  $('#accountAvatar').textContent = (settings.username || 'P').charAt(0).toUpperCase();
  if (!busy) {
    const running = startedAt && Date.now() - startedAt < 120000 && startedVersion === p.version;
    $('#playBtn').textContent = 'Play';
    $('#playBtn').disabled = false;
    $('#playState').textContent = !bridge()
      ? 'Start the app with Electron to launch Minecraft'
      : (running ? 'Minecraft ' + p.version + ' is starting'
        : (!info || !info.installed ? 'Play installs ' + p.version + ' the first time'
          : (info.javaPath ? 'Ready · Java ' + info.java : 'Java ' + info.java + ' not found — set it in Settings')));
  }
}
$('#heroSelect').addEventListener('change', e => { selected = e.target.value; save(); renderAll(); });

async function play() {
  const p = current();
  if (!p || busy) return;
  const api = bridge();
  if (!api) return toast('Start the app with Electron to launch Minecraft');
  const info = supportedVersion(p.version);
  if (info && !info.javaPath) {
    return toast('Java ' + info.java + ' not found — set the Java path in Settings');
  }
  setBusy(true);
  $('#playState').textContent = info && info.installed ? 'Starting Minecraft…' : 'Installing ' + p.version + '…';
  $('#vProgress').textContent = '';
  try {
    const res = await api.launch(p.version, {
      username: settings.username || 'Player',
      ramMb: (p.ram || settings.ram) * 1024,
      javaPath: settings.java || ''
    });
    if (res && res.ok) {
      startedAt = Date.now();
      startedVersion = p.version;
      $('#playState').textContent = 'Minecraft ' + p.version + ' is starting';
      toast('Minecraft ' + p.version + ' is starting');
    } else {
      const msg = (res && res.error) || 'Launch failed';
      $('#playState').textContent = 'Launch failed';
      toast(msg);
    }
  } catch (err) {
    toast('Launch failed');
  } finally {
    setBusy(false);
    await refreshStatus();
    renderAll();
  }
}
$('#playBtn').addEventListener('click', play);

// ---- profiles ----
function renderProfiles() {
  $('#profileGrid').innerHTML = profiles.map(p => `
    <div class="pcard${p.id === selected ? ' sel' : ''}">
      <div class="prow"><div class="pico" data-color="${esc(p.color)}">${esc(p.name.charAt(0).toUpperCase() || '?')}</div>
        <div><b>${esc(p.name)}${p.id === selected ? '<span class="badge">Active</span>' : ''}</b>
        <small>${esc(p.version)} · ${esc(p.loader)} · ${esc(p.ram)} GB</small></div></div>
      <div class="pact">
        <button class="btn primary" data-a="sel" data-id="${esc(p.id)}">Select</button>
        <button class="btn" data-a="edit" data-id="${esc(p.id)}">Edit</button>
        <button class="btn danger" data-a="del" data-id="${esc(p.id)}">Delete</button>
      </div>
    </div>`).join('');
  // colors are set via the DOM (inline style attributes are blocked by the CSP)
  $$('.pico').forEach(el => { el.style.background = el.dataset.color; });
}
$('#profileGrid').addEventListener('click', e => {
  // forza reload
  try {
    const s = localStorage.getItem('ec_profiles');
    if (s) {
      const tmp = JSON.parse(s);
      if (Array.isArray(tmp)) profiles = tmp;
    }
  } catch {}
  const b = e.target.closest('button[data-a]'); if (!b) return;
  const id = b.dataset.id;
  if (b.dataset.a === 'sel') { selected = id; save(); renderAll(); return; }
  if (b.dataset.a === 'edit') { openDialog(id); return; }
  if (b.dataset.a === 'del') {
    if (!Array.isArray(profiles) || profiles.length === 1) return toast('You need at least one profile');
    profiles = profiles.filter(p => p.id !== id);
    if (selected === id) selected = profiles[0].id;
    try { localStorage.setItem('ec_profiles', JSON.stringify(profiles)); } catch {}
    save(); renderAll();
    return;
  }
});

// ---- create / edit dialog ----
const dlg = $('#dlg');
function versionOptions(select) {
  select.innerHTML = versions().map(v => `<option value="${esc(v.version)}">${esc(v.version)} · ${esc(v.loader)}</option>`).join('');
}
versionOptions($('#fVersion'));
$('#fVersion').addEventListener('change', e => { $('#fLoader').value = loaderOf(e.target.value); renderProfileMods(); });
$('#fRam').addEventListener('input', e => $('#fRamOut').textContent = e.target.value);

// ---- edit profile: mods ----
let modsSource = 'modrinth';

function profileVersion() {
  const sel = $('#fVersion');
  return (sel && sel.value) || (versions()[0] ? versions()[0].version : '1.21.11');
}

function profileLoader() {
  const sel = $('#fLoader');
  return sel && sel.value ? sel.value : loaderOf(profileVersion());
}

function renderProfileMods(version) {
  const v = version || profileVersion();
  const listEl = $('#fModsList');
  const countEl = $('#fModsCount');
  if (!listEl || !countEl) return;
  const api = bridge();
  if (!api || typeof api.getMods !== 'function') {
    countEl.textContent = '0 mods';
    listEl.innerHTML = '<div class="mods-empty">Start the app with npm start to manage mods</div>';
    return;
  }
  listEl.innerHTML = '<div class="mods-empty">Loading…</div>';
  api.getMods(v).then(res => {
    const mods = res && Array.isArray(res.mods) ? res.mods : [];
    countEl.textContent = mods.length + (mods.length === 1 ? ' mod' : ' mods');
    if (!mods.length) {
      listEl.innerHTML = '<div class="mods-empty">No mods yet — open “Download mods”.</div>';
      return;
    }
    listEl.innerHTML = mods.map(m => `
      <div class="mods-item">
        <span class="mods-name">${esc(m.name)}<em class="mods-origin ${m.bundled ? 'eclipse' : 'downloaded'}">${m.bundled ? 'Divine' : 'Downloaded'}</em></span>
        ${m.removable ? `<button type="button" class="mods-del" data-del="${esc(m.name)}" title="Remove">✕</button>` : ''}
      </div>`).join('');
  }).catch(() => {
    countEl.textContent = '0 mods';
    listEl.innerHTML = '<div class="mods-empty">Unable to read the mods folder</div>';
  });
}

function selectModsTab(name) {
  $$('.msub').forEach(b => b.classList.toggle('on', b.dataset.mtab === name));
  const installed = $('#fPaneInstalled');
  const download = $('#fPaneDownload');
  if (installed) installed.hidden = name !== 'installed';
  if (download) download.hidden = name !== 'download';
  if (name === 'installed') {
    renderProfileMods();
  } else if ($('#fModResults') && !$('#fModResults').dataset.loaded) {
    searchDialogMods();
  }
}

async function searchDialogMods() {
  const api = bridge();
  const results = $('#fModResults');
  const status = $('#fModStatus');
  if (!results || !status) return;
  if (!api || typeof api.searchMods !== 'function') {
    status.textContent = 'Start the app with npm start to download mods';
    return;
  }
  const version = profileVersion();
  const loader = profileLoader();
  const query = $('#fModSearch').value.trim();
  results.dataset.loaded = '1';
  results.innerHTML = '';
  status.textContent = modsSource === 'curseforge' && !settings.cfKey
    ? 'CurseForge needs a free API key: add it in Settings.'
    : (query ? 'Searching…' : 'Loading mods for ' + version + ' · ' + loader + '…');
  let res = null;
  try {
    res = await api.searchMods({ source: modsSource, query, version, loader, apiKey: settings.cfKey || '' });
  } catch {
    status.textContent = 'Search failed';
    return;
  }
  if (!res || !res.ok) {
    const msg = (res && res.error) || 'Search failed';
    if (msg === 'CURSEFORGE_KEY_MISSING') status.textContent = 'CurseForge needs a free API key: Settings → CurseForge API key.';
    else if (msg === 'CURSEFORGE_KEY_INVALID') status.textContent = 'CurseForge rejected the key. Check it in Settings.';
    else status.textContent = msg;
    return;
  }
  const list = res.results || [];
  status.textContent = list.length
    ? list.length + ' mods for ' + version
    : 'No mods found for ' + version + ' · ' + loader;
  renderModResults(list);
}

function renderModResults(list) {
  const results = $('#fModResults');
  if (!results) return;
  results.innerHTML = list.map(m => {
    const installed = !!m.installed;
    return `
    <div class="mods-card${installed ? ' is-installed' : ''}">
      <div class="mods-icon">${m.icon ? `<img src="${esc(m.icon)}" alt="">` : ''}</div>
      <div class="mods-info">
        <div class="mods-titleline"><h3>${esc(m.name || '')}</h3><span class="mods-badge">${m.source === 'curseforge' ? 'CurseForge' : 'Modrinth'}</span></div>
        <p class="dim">${esc(m.description || '')}</p>
      </div>
      <div class="mods-actions">
        <button type="button" class="btn${installed ? '' : ' primary'}" data-install="1" data-source="${esc(m.source || modsSource)}" data-id="${esc(m.id)}"${installed ? ' disabled' : ''}>${installed ? 'Installed' : 'Download'}</button>
      </div>
    </div>`;
  }).join('');
}

async function installMod(btn) {
  const api = bridge();
  if (!api || typeof api.downloadMod !== 'function') return;
  const version = profileVersion();
  const status = $('#fModStatus');
  const label = btn.textContent;
  btn.disabled = true;
  btn.textContent = 'Installing…';
  try {
    const res = await api.downloadMod({
      source: btn.dataset.source, id: btn.dataset.id, version,
      loader: profileLoader(), apiKey: settings.cfKey || ''
    });
    if (res && res.ok) {
      const names = [...(res.downloaded || []), ...(res.skipped || [])];
      toast(names.length ? 'Installed ' + names.join(', ') : 'Mod installed');
      if (status) status.textContent = names.length ? 'Installed: ' + names.join(', ') : '';
      btn.textContent = 'Installed';
      btn.disabled = true;
      btn.classList.remove('primary');
      renderProfileMods(version);
    } else {
      const msg = (res && res.error) || 'Install failed';
      if (status) status.textContent = msg;
      toast(msg);
    }
  } catch {
    toast('Install failed');
  } finally {
    if (btn.textContent !== 'Installed') {
      btn.disabled = false;
      btn.textContent = label || 'Download';
    }
  }
}

async function removeMod(name) {
  const api = bridge();
  if (!api || typeof api.removeMod !== 'function') return;
  const version = profileVersion();
  try {
    const res = await api.removeMod(version, name);
    if (res && res.ok) { toast('Removed ' + name); renderProfileMods(version); }
    else toast((res && res.error) || 'Remove failed');
  } catch {
    toast('Remove failed');
  }
}

function openDialog(id) {
  editingId = id || null;
  const p = id ? profiles.find(x => x.id === id) : { name: '', version: versions()[0].version, loader: loaderOf(versions()[0].version), ram: settings.ram, color: '#6c8cff' };
  $('#dlgTitle').textContent = id ? 'Edit profile' : 'New profile';
  versionOptions($('#fVersion'));
  $('#fName').value = p.name; $('#fVersion').value = p.version; $('#fLoader').value = p.loader;
  $('#fRam').value = p.ram; $('#fRamOut').textContent = p.ram; $('#fColor').value = p.color;
  modsSource = 'modrinth';
  $$('.msource').forEach(b => b.classList.toggle('on', b.dataset.source === 'modrinth'));
  const resultsEl = $('#fModResults');
  if (resultsEl) { resultsEl.innerHTML = ''; resultsEl.dataset.loaded = ''; }
  const searchEl = $('#fModSearch');
  if (searchEl) searchEl.value = '';
  const statusEl = $('#fModStatus');
  if (statusEl) statusEl.textContent = '';
  dlg.showModal(); $('#fName').focus();
  // la lista mod dipende dal backend: se fallisce il dialog deve restare aperto
  try { selectModsTab('installed'); } catch (err) { console.warn('mods tab', err); }
}
$('#newProfile').addEventListener('click', () => {
  if (typeof openDialog === 'function') openDialog();
  else {
    const d = document.getElementById('dlg');
    if (d && d.showModal) d.showModal();
  }
});
$('#cancel').addEventListener('click', () => dlg.close());
$('#pForm').addEventListener('submit', (e) => {
  e.preventDefault();
  if (!Array.isArray(profiles)) profiles = [];
  try {
    const saved = localStorage.getItem('ec_profiles');
    if (saved) {
      const tmp = JSON.parse(saved);
      if (Array.isArray(tmp)) profiles = tmp;
    }
  } catch {}
  const version = $('#fVersion').value;
  const data = { name: ($('#fName').value||'').trim() || 'Unnamed', version, loader: $('#fLoader').value, ram: +$('#fRam').value || 4, color: $('#fColor').value || '#6c8cff' };
  if (editingId) {
    const idx = profiles.findIndex(p => p.id === editingId);
    if (idx >= 0) profiles[idx] = Object.assign({}, profiles[idx], data);
  } else {
    const p = { id: uid(), ...data };
    profiles.push(p);
    selected = p.id;
  }
  try { localStorage.setItem('ec_profiles', JSON.stringify(profiles)); } catch {}
  save();
  renderAll();
  const d = document.getElementById('dlg');
  if (d && typeof d.close === 'function') d.close();
});

// ---- versions ----
function renderVersions() {
  const list = versions();
  const installed = list.filter(v => v.installed).length;
  $('#vSummary').textContent = installed + ' / ' + list.length + ' installed';
  $('#vList').innerHTML = list.map(v => `
    <div class="vrow${v.installed ? '' : ' off'}">
      <span><b>${esc(v.version)}</b><span class="tag">${esc(v.loader)}</span><span class="tag">${esc(v.label || '')}</span></span>
      <span class="vstate">${v.installed ? 'Installed · Java ' + v.java : (v.javaPath ? 'Java ' + v.java + ' ready' : 'Java ' + v.java + ' missing')}</span>
      <button class="btn" data-a="install" data-v="${esc(v.version)}"${busy ? ' disabled' : ''}>${v.installed ? 'Reinstall' : 'Install'}</button>
      <button class="btn" data-a="profile" data-v="${esc(v.version)}"${v.installed ? '' : ' disabled'}>Create profile</button>
    </div>`).join('');
}
$('#vList').addEventListener('click', async e => {
  const b = e.target.closest('button[data-a]'); if (!b || busy) return;
  const version = b.dataset.v;
  if (b.dataset.a === 'profile') {
    openDialog(); $('#fVersion').value = version; $('#fName').value = 'Minecraft ' + version; $('#fLoader').value = loaderOf(version);
    return;
  }
  const api = bridge();
  if (!api) return toast('Start the app with Electron to install');
  const info = supportedVersion(version);
  if (info && !info.javaPath) return toast('Java ' + info.java + ' not found — set the Java path in Settings');
  setBusy(true);
  $('#vProgress').textContent = 'Preparing ' + version + '…';
  $$('#vList button').forEach(x => x.disabled = true);
  try {
    const res = await api.install(version);
    if (res && res.ok) { toast('Minecraft ' + version + ' installed'); $('#vProgress').textContent = 'Minecraft ' + version + ' is ready'; }
    else { const msg = (res && res.error) || 'Install failed'; $('#vProgress').textContent = msg; toast(msg); }
  } catch {
    toast('Install failed');
  } finally {
    setBusy(false);
    await refreshStatus();
    renderAll();
  }
});

// ---- mods: clicks and search inside the profile dialog ----
const profDlg = $('#dlg');
if (profDlg) {
  profDlg.addEventListener('click', e => {
    const tab = e.target.closest('.msub');
    if (tab) { selectModsTab(tab.dataset.mtab); return; }
    const src = e.target.closest('.msource');
    if (src) {
      modsSource = src.dataset.source;
      $$('.msource').forEach(b => b.classList.toggle('on', b === src));
      const resultsEl = $('#fModResults');
      if (resultsEl) { resultsEl.innerHTML = ''; resultsEl.dataset.loaded = ''; }
      searchDialogMods();
      return;
    }
    if (e.target.closest('#fModSearchBtn')) { searchDialogMods(); return; }
    const del = e.target.closest('[data-del]');
    if (del) { removeMod(del.dataset.del); return; }
    const inst = e.target.closest('[data-install]');
    if (inst) installMod(inst);
  });
}
const modSearchEl = $('#fModSearch');
if (modSearchEl) {
  modSearchEl.addEventListener('keydown', e => {
    if (e.key === 'Enter') { e.preventDefault(); searchDialogMods(); }
  });
}

function loadSettings() {
  const ram = Number(settings.ram) || 4;
  $('#setRam').value = ram;
  $('#ramOut').textContent = ram;
  $('#setUser').value = settings.username || 'Player';
  $('#setJava').value = settings.java || '';
  $('#setCfKey').value = settings.cfKey || '';
  const shoot = $('#setShoot');
  shoot.checked = settings.shooting !== false;
  if (window.ecFx) window.ecFx.setShooting(shoot.checked);
  const java = settings.java ? settings.java : 'automatic per version';
  $('#dataState').textContent = 'Game folder: ' + (launcherInfo.dataDir || '-') + ' · Java: ' + java;
}
$('#setRam').addEventListener('input', e => $('#ramOut').textContent = e.target.value);
$('#setShoot').addEventListener('change', e => window.ecFx.setShooting(e.target.checked));
$('#saveSettings').addEventListener('click', async () => {
  settings = {
    ram: +$('#setRam').value,
    java: $('#setJava').value.trim(),
    username: ($('#setUser').value.trim() || 'Player').slice(0, 16),
    shooting: $('#setShoot').checked,
    cfKey: $('#setCfKey').value.trim()
  };
  save();
  await refreshStatus();
  renderAll();
  loadSettings();
  toast('Settings saved');
});

// ---- progress from the backend ----
if (bridge() && typeof bridge().onProgress === 'function') {
  bridge().onProgress(payload => {
    if (!payload || !payload.message) return;
    $('#vProgress').textContent = payload.message;
    if (busy) $('#playState').textContent = payload.message;
  });
}

function renderAll() {
  // carica sempre dai profili salvati prima di renderizzare
  try {
    const s = localStorage.getItem('ec_profiles');
    if (s) {
      const tmp = JSON.parse(s);
      if (Array.isArray(tmp) && tmp.length) profiles = tmp;
    }
  } catch {}
  renderHome();
  renderProfiles();
  renderVersions();
}

loadSettings();
refreshStatus().then(() => {
  if (!Array.isArray(profiles)) {
    try { const s = localStorage.getItem('ec_profiles'); if (s) profiles = JSON.parse(s); } catch {}
  }
  if (!Array.isArray(profiles)) profiles = [];
  pruneProfiles();
  renderAll();
  loadSettings();
});

// errori visibili: un launcher che fallisce in silenzio non e' diagnosticabile
window.addEventListener('error', e => { try { toast('Error: ' + (e.message || e.error)); } catch {} });
window.addEventListener('unhandledrejection', e => {
  try { const r = e.reason; toast('Error: ' + (r && r.message ? r.message : r)); } catch {}
});

// ---- auto update: il lavoro lo fa il comando Rust divine_check_updates ----
function tauriListen() {
  return (window.__TAURI__ && window.__TAURI__.event && window.__TAURI__.event.listen) || null;
}

// payload: { downloaded, total, percent } — percent null se il server non manda la dimensione
function updateLabel(payload) {
  const data = payload || {};
  const downloaded = Number(data.downloaded) || 0;
  const total = Number(data.total) || 0;
  const percent = typeof data.percent === 'number' ? data.percent : (total > 0 ? Math.floor(downloaded / total * 100) : null);
  if (percent != null) return Math.max(0, Math.min(100, percent)) + '%';
  if (downloaded > 0) return (downloaded / 1048576).toFixed(1) + ' MB';
  return '0%';
}

function setUpdateStatus(text) {
  const line = $('#updateStatus');
  if (line) line.textContent = text || '';
}

document.addEventListener('click', async e => {
  if (!e.target || e.target.id !== 'checkUpdates') return;
  const core = window.__TAURI__ && window.__TAURI__.core;
  if (!core || typeof core.invoke !== 'function') { toast('Updater non disponibile'); return; }

  const listen = tauriListen();
  let unlisten;
  let last = -1;
  if (listen) {
    unlisten = await listen('update-progress', ev => {
      const data = ev.payload || {};
      const percent = typeof data.percent === 'number' ? data.percent : -1;
      if (percent === last) return; // evita aggiornamenti ridondanti
      last = percent;
      setUpdateStatus('Scaricando aggiornamento: ' + updateLabel(data));
    });
  }
  setUpdateStatus('Controllo aggiornamenti...');
  toast('Controllo aggiornamenti...');
  try {
    const res = await core.invoke('divine_check_updates');
    if (res && res.busy) { setUpdateStatus('Aggiornamento gia in corso...'); return; }
    if (!res || !res.ok) {
      setUpdateStatus('');
      toast('Update non riuscito: ' + ((res && res.error) || 'errore'));
      return;
    }
    if (!res.updated) {
      setUpdateStatus('');
      toast('Sei gia sulla versione piu recente');
      return;
    }
    setUpdateStatus('Aggiornato a ' + res.version + ': riavvio del launcher...');
  } catch (err) {
    setUpdateStatus('');
    toast('Update non riuscito: ' + (err && err.message ? err.message : err));
  } finally {
    if (typeof unlisten === 'function') unlisten();
  }
});


// ---- aggiornamenti: controllo a ogni avvio e a ogni click, senza limiti ----
(function () {
  const listen = tauriListen();
  const core = window.__TAURI__ && window.__TAURI__.core;
  const dismiss = document.getElementById('dismissUpdate');
  const checkBtn = document.getElementById('checkUpdates');
  if (!listen) return;

  let seenVersion = '';

  const showDismiss = show => {
    if (dismiss) dismiss.style.display = show ? 'inline-flex' : 'none';
  };

  const applyStatus = res => {
    if (!res || !res.ok || !res.available || res.dismissed) {
      if (res && res.dismissed) setUpdateStatus('');
      showDismiss(false);
      return;
    }
    seenVersion = res.version || '';
    if (res.policy && res.policy !== 'self') {
      setUpdateStatus('Aggiornamento ' + seenVersion + ': ' + (res.hint || 'aggiorna dal gestore pacchetti'));
      showDismiss(false);
      return;
    }
    setUpdateStatus('Aggiornamento ' + seenVersion + ' disponibile: premi Check updates per installarlo');
    showDismiss(true);
  };

  listen('update-available', ev => {
    const version = ev && ev.payload && ev.payload.version;
    if (!version) return;
    applyStatus({ ok: true, available: true, version, policy: 'self' });
    toast('Aggiornamento ' + version + ' disponibile');
  });
  listen('update-start', ev => {
    const version = ev && ev.payload && ev.payload.version;
    showDismiss(false);
    setUpdateStatus('Scaricando aggiornamento' + (version ? ' ' + version : '') + '...');
  });
  listen('update-done', () => {
    showDismiss(false);
    setUpdateStatus('Aggiornamento installato, riavvio...');
  });
  listen('update-error', ev => {
    setUpdateStatus('');
    const message = ev && ev.payload && ev.payload.message;
    if (message) toast(message);
  });

  // stato iniziale: controllo in rete a ogni avvio
  if (core && typeof core.invoke === 'function') {
    core.invoke('divine_check_update_status', { force: false }).then(applyStatus).catch(() => {});
  }

  if (checkBtn) checkBtn.addEventListener('click', () => showDismiss(false));

  if (dismiss && core) dismiss.addEventListener('click', async () => {
    showDismiss(false);
    setUpdateStatus('');
    try {
      await core.invoke('divine_dismiss_update', { version: seenVersion || null });
    } catch (_) {}
  });
})();
