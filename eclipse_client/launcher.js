'use strict';
// DivineClient launcher: installa e avvia Minecraft da solo (nessun Prism, nessun altro launcher).
// I file di gioco stanno in una cartella dati del client, ogni versione supportata ha la sua.
const fs = require('fs');
const os = require('os');
const path = require('path');

const { mkdirp, exists, UA } = require('./engine/util');
const { vanillaVersion, chainFor, installLibraries, installAssets } = require('./engine/minecraft');
const { installFabric, installForge, installForgeLegacy } = require('./engine/loaders');
const { findJava } = require('./engine/java');
const { launch: launchVersion } = require('./engine/launch');

// Le uniche versioni supportate dal client.
const SUPPORTED = [
  { version: '1.8.9', loader: 'Forge', label: 'PvP classico', forge: '1.8.9-11.15.1.2318-1.8.9', java: 8, legacyForge: true },
  { version: '1.20.1', loader: 'Forge', label: 'Moderne', forge: '1.20.1-47.4.28', java: 17 },
  { version: '1.21.11', loader: 'Fabric', label: 'Ultima release', java: 21 },
  { version: '26.2', loader: 'Fabric', label: 'Snapshot', java: 25 },
  { version: '26.3', loader: 'Fabric', label: 'Snapshot', java: 25 }
];

const APP_DIR = __dirname;

function dataDir() {
  const home = os.homedir();
  if (process.platform === 'win32') {
    return path.join(process.env.APPDATA || path.join(home, 'AppData', 'Roaming'), 'EclipseClient');
  }
  if (process.platform === 'darwin') {
    return path.join(home, 'Library', 'Application Support', 'EclipseClient');
  }
  return path.join(home, '.local', 'share', 'EclipseClient');
}

const instanceDir = version => path.join(dataDir(), 'instances', version);
const modsSourceDir = version => path.join(APP_DIR, 'mods', version);
const markerFile = version => path.join(instanceDir(version), 'installed.json');

function readMarker(version) {
  try { return JSON.parse(fs.readFileSync(markerFile(version), 'utf8')); } catch { return null; }
}

function spec(version) {
  return SUPPORTED.find(v => v.version === version) || null;
}

/** Copia i mod del client nella cartella mods dell'istanza (sovrascrive se cambiano). */
function installMods(version) {
  const source = modsSourceDir(version);
  const target = path.join(instanceDir(version), 'mods');
  mkdirp(target);
  if (!fs.existsSync(source)) return [];
  const copied = [];
  for (const name of fs.readdirSync(source)) {
    if (!name.endsWith('.jar')) continue;
    const from = path.join(source, name);
    const to = path.join(target, name);
    const size = fs.statSync(from).size;
    if (exists(to) && fs.statSync(to).size === size) continue;
    fs.copyFileSync(from, to);
    copied.push(name);
  }
  return copied;
}

function statusFor(version) {
  const info = spec(version);
  const marker = readMarker(version);
  const installed = !!(marker && marker.versionId && exists(path.join(dataDir(), 'versions', marker.versionId, marker.versionId + '.json')));
  const java = info ? findJava(info.java) : null;
  return {
    version: info.version,
    loader: info.loader,
    label: info.label,
    java: info.java,
    installed,
    versionId: installed ? marker.versionId : null,
    javaPath: java ? java.path : null,
    instanceDir: instanceDir(version),
    mods: installed ? listMods(version) : []
  };
}

function listMods(version) {
  const dir = path.join(instanceDir(version), 'mods');
  try { return fs.readdirSync(dir).filter(f => f.endsWith('.jar')); } catch { return []; }
}

function listSourceMods(version) {
  const dir = modsSourceDir(version);
  try { return fs.readdirSync(dir).filter(f => f.endsWith('.jar')); } catch { return []; }
}

function status() {
  return {
    dataDir: dataDir(),
    versions: SUPPORTED.map(v => statusFor(v.version)),
    offline: true
  };
}

/**
 * Installa una versione per intero: vanilla, loader (Fabric o Forge), librerie, asset e mod.
 * `onProgress(testo)` aggiorna la UI; lancia se qualcosa non va.
 */
async function install(version, onProgress) {
  const info = spec(version);
  if (!info) throw new Error('Unsupported version: ' + version);
  const say = msg => { if (onProgress) onProgress(String(msg)); };
  mkdirp(dataDir());
  mkdirp(instanceDir(version));

  say('Downloading Minecraft ' + version);
  const vanilla = await vanillaVersion(dataDir(), version, say);

  let versionId;
  if (info.loader === 'Fabric') {
    const fabric = await installFabric(dataDir(), version, say);
    versionId = fabric.id;
  } else if (info.legacyForge) {
    const forge = await installForgeLegacy(dataDir(), version, info.forge, say);
    versionId = forge.id;
  } else {
    const java = findJava(info.java);
    if (!java.path) throw new Error('Java ' + info.java + ' is required for Forge ' + version + '. Install it or set the Java path in Settings.');
    const forge = await installForge(dataDir(), version, info.forge, java.path, say);
    versionId = forge.id;
  }

  say('Downloading libraries');
  const chain = await chainFor(dataDir(), versionId);
  await installLibraries(dataDir(), chain, path.join(dataDir(), 'natives', versionId), say);

  say('Downloading assets');
  await installAssets(dataDir(), vanilla.json, say);

  say('Installing Crosshair Client');
  const copied = installMods(version);
  mkdirp(path.join(instanceDir(version), 'saves'));
  fs.writeFileSync(markerFile(version), JSON.stringify({
    version, versionId, loader: info.loader, installedAt: new Date().toISOString(), mods: listMods(version)
  }, null, 2));
  say('Ready');
  return { version, versionId, mods: listMods(version), copied };
}

/** Avvia Minecraft. Se la versione non e' ancora installata la installa prima. */
async function launch(version, options, onProgress) {
  const info = spec(version);
  if (!info) return { ok: false, error: 'Unsupported version: ' + version };
  const opts = options || {};
  const username = (opts.username || 'Player').trim().slice(0, 16) || 'Player';
  const ramMb = Math.max(1024, Math.min(16384, Number(opts.ramMb) || 4096));
  const say = msg => { if (onProgress) onProgress(String(msg)); };

  const before = statusFor(version);
  if (!before.installed) await install(version, onProgress);

  const java = findJava(info.java, opts.javaPath || '');
  if (!java.path) {
    return { ok: false, error: 'Java ' + info.java + ' not found. Install it or set the Java path in Settings.' };
  }
  const marker = readMarker(version);
  if (!marker) return { ok: false, error: 'Version ' + version + ' is not installed' };
  installMods(version);

  return launchVersion({
    dataDir: dataDir(),
    instanceDir: instanceDir(version),
    versionId: marker.versionId,
    mcVersion: version,
    username,
    ramMb,
    javaPath: java.path
  }, say);
}

function modLoaderSlug(loader) {
  return String(loader || '').toLowerCase() === 'fabric' ? 'fabric' : 'forge';
}

function curseForgeLoaderType(loader) {
  switch (String(loader || '').toLowerCase()) {
    case 'fabric':
    case 'quilt':
      return 4;
    case 'neoforge':
      return 5;
    default:
      return 1;
  }
}

// Registro dei jar scaricati dal browser mod: serve a distinguere "Downloaded" da "Eclipse".
const manifestFile = version => path.join(instanceDir(version), 'mods-manifest.json');

function readManifest(version) {
  try {
    const json = JSON.parse(fs.readFileSync(manifestFile(version), 'utf8'));
    return { mods: Array.isArray(json.mods) ? json.mods : [] };
  } catch {
    return { mods: [] };
  }
}

function writeManifest(version, manifest) {
  try {
    mkdirp(instanceDir(version));
    fs.writeFileSync(manifestFile(version), JSON.stringify({ mods: manifest.mods }, null, 2));
  } catch {}
}

/** Mod dell'istanza: quelli del client + quelli scaricati dall'utente. */
function getMods(version) {
  const manifest = readManifest(version);
  const byFile = new Map(manifest.mods.map(m => [m.file, m]));
  const found = new Map();
  const collect = (dir, bundled) => {
    let names = [];
    try { names = fs.readdirSync(dir); } catch { return; }
    for (const name of names) {
      if (!name.toLowerCase().endsWith('.jar')) continue;
      let size = 0;
      try { size = fs.statSync(path.join(dir, name)).size; } catch {}
      const existing = found.get(name);
      if (existing && existing.bundled) continue; // il jar del client vince sulla copia nell'istanza
      const record = byFile.get(name);
      found.set(name, {
        name,
        size,
        bundled,
        origin: bundled ? 'eclipse' : 'downloaded',
        project: record ? record.source + ':' + record.id : null,
        removable: !bundled
      });
    }
  };
  collect(modsSourceDir(version), true);
  collect(path.join(instanceDir(version), 'mods'), false);
  return [...found.values()].sort((a, b) => a.name.localeCompare(b.name));
}

/** Progetti ("source:id") gia' installati in una versione. */
function installedProjects(version) {
  return readManifest(version).mods.map(m => String(m.source || 'modrinth') + ':' + m.id);
}

function removeMod(version, name) {
  const file = path.basename(String(name || ''));
  if (!file.toLowerCase().endsWith('.jar')) throw new Error('Not a mod file');
  const full = path.join(instanceDir(version), 'mods', file);
  if (!exists(full)) return { removed: false, name: file };
  fs.unlinkSync(full);
  const manifest = readManifest(version);
  const rest = manifest.mods.filter(m => m.file !== file);
  if (rest.length !== manifest.mods.length) writeManifest(version, { mods: rest });
  return { removed: true, name: file };
}

async function searchMods(opts) {
  const { source, query, version, loader, apiKey } = opts || {};
  if (!version) throw new Error('Missing Minecraft version');
  if (source === 'curseforge') return searchCurseForge(query, version, loader, apiKey);
  return searchModrinth(query, version, loader);
}

async function searchModrinth(query, version, loader) {
  const facets = JSON.stringify([
    ['project_type:mod'],
    ['versions:' + version],
    ['categories:' + modLoaderSlug(loader)]
  ]);
  const url = 'https://api.modrinth.com/v2/search?query=' + encodeURIComponent(query || '') +
    '&facets=' + encodeURIComponent(facets) + '&limit=30&index=relevance';
  const res = await fetch(url, { headers: { 'User-Agent': UA } });
  if (!res.ok) throw new Error('Modrinth HTTP ' + res.status);
  const json = await res.json();
  return (json.hits || []).map(h => ({
    id: h.project_id,
    slug: h.slug,
    name: h.title,
    description: h.description,
    icon: h.icon_url,
    downloads: h.downloads,
    source: 'modrinth'
  }));
}

async function searchCurseForge(query, version, loader, apiKey) {
  const key = String(apiKey || '').trim();
  if (!key) throw new Error('CURSEFORGE_KEY_MISSING');
  const params = new URLSearchParams({
    gameId: '432',
    classId: '6',
    gameVersion: version,
    modLoaderType: String(curseForgeLoaderType(loader)),
    pageSize: '30',
    sortField: '2',
    sortOrder: 'desc'
  });
  if (query) params.set('searchFilter', query);
  const res = await fetch('https://api.curseforge.com/v1/mods/search?' + params.toString(), {
    headers: { 'x-api-key': key, Accept: 'application/json', 'User-Agent': UA }
  });
  if (res.status === 403) throw new Error('CURSEFORGE_KEY_INVALID');
  if (!res.ok) throw new Error('CurseForge HTTP ' + res.status);
  const json = await res.json();
  return (json.data || []).map(m => ({
    id: m.id,
    slug: m.slug,
    name: m.name,
    description: m.summary,
    icon: m.logo ? (m.logo.thumbnailUrl || m.logo.url) : null,
    downloads: m.downloadCount,
    source: 'curseforge'
  }));
}

async function resolveModrinthFile(projectId, version, loader) {
  // l'API di Modrinth vuole le parentesi quadre letterali nella query
  const gameVersions = '[' + encodeURIComponent('"' + version + '"') + ']';
  const loaders = '[' + encodeURIComponent('"' + modLoaderSlug(loader) + '"') + ']';
  const url = 'https://api.modrinth.com/v2/project/' + encodeURIComponent(projectId) +
    '/version?game_versions=' + gameVersions + '&loaders=' + loaders;
  const res = await fetch(url, { headers: { 'User-Agent': UA } });
  if (!res.ok) throw new Error('Modrinth HTTP ' + res.status);
  const list = await res.json();
  if (!Array.isArray(list) || !list.length) throw new Error('Nessuna versione per ' + version);
  const v = list[0];
  const file = (v.files || []).find(f => f.primary) || (v.files || [])[0];
  if (!file || !file.url) throw new Error('Download non disponibile');
  const deps = (v.dependencies || [])
    .filter(d => d.dependency_type === 'required' && d.project_id)
    .map(d => ({ source: 'modrinth', id: d.project_id }));
  return { url: file.url, name: path.basename(file.filename || 'mod.jar'), deps };
}

async function resolveCurseForgeFile(modId, version, loader, apiKey) {
  const key = String(apiKey || '').trim();
  if (!key) throw new Error('CURSEFORGE_KEY_MISSING');
  const params = new URLSearchParams({ gameVersion: version, modLoaderType: String(curseForgeLoaderType(loader)) });
  const res = await fetch('https://api.curseforge.com/v1/mods/' + encodeURIComponent(modId) + '/files?' + params.toString(), {
    headers: { 'x-api-key': key, Accept: 'application/json', 'User-Agent': UA }
  });
  if (res.status === 403) throw new Error('CURSEFORGE_KEY_INVALID');
  if (!res.ok) throw new Error('CurseForge HTTP ' + res.status);
  const json = await res.json();
  const files = (json.data || []).slice().sort((a, b) => String(b.fileDate || '').localeCompare(String(a.fileDate || '')));
  const file = files[0];
  if (!file) throw new Error('Nessuna versione per ' + version);
  if (!file.downloadUrl) throw new Error("L'autore ha disattivato i download esterni");
  return { url: file.downloadUrl, name: path.basename(file.fileName || 'mod.jar'), deps: [] };
}

/** Scarica una mod (+ dipendenze obbligatorie) nella cartella mods dell'istanza. */
async function downloadMod(opts, onProgress) {
  const { source, id, version, loader, apiKey } = opts || {};
  if (!version) throw new Error('Missing Minecraft version');
  if (!id) throw new Error('Missing mod id');
  const say = msg => { if (onProgress) onProgress(String(msg)); };
  const target = path.join(instanceDir(version), 'mods');
  mkdirp(target);
  const downloaded = [];
  const skipped = [];
  const records = [];
  const queue = [{ source: source === 'curseforge' ? 'curseforge' : 'modrinth', id, depth: 0 }];
  const seen = new Set();
  while (queue.length) {
    const item = queue.shift();
    const key = item.source + ':' + item.id;
    if (seen.has(key)) continue;
    seen.add(key);
    const file = item.source === 'curseforge'
      ? await resolveCurseForgeFile(item.id, version, loader, apiKey)
      : await resolveModrinthFile(item.id, version, loader);
    const dest = path.join(target, file.name);
    records.push({ file: file.name, source: item.source, id: item.id });
    if (exists(dest)) {
      skipped.push(file.name);
    } else {
      say('Downloading ' + file.name);
      const res = await fetch(file.url, { headers: { 'User-Agent': UA } });
      if (!res.ok) throw new Error('HTTP ' + res.status + ' ' + file.name);
      const data = Buffer.from(await res.arrayBuffer());
      fs.writeFileSync(dest + '.part', data);
      fs.renameSync(dest + '.part', dest);
      downloaded.push(file.name);
    }
    if (item.depth < 3) {
      for (const dep of file.deps) queue.push({ source: dep.source, id: dep.id, depth: item.depth + 1 });
    }
  }
  const manifest = readManifest(version);
  const byFile = new Map(manifest.mods.map(m => [m.file, m]));
  for (const record of records) byFile.set(record.file, record);
  writeManifest(version, { mods: [...byFile.values()] });
  return { downloaded, skipped };
}

module.exports = { SUPPORTED, status, install, launch, dataDir, findJava, getMods, installedProjects, removeMod, searchMods, downloadMod };
