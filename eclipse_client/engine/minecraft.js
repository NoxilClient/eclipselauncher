'use strict';
const fs = require('fs');
const path = require('path');
const { getJson, download, pool, extractNatives, mavenPath, exists, mkdirp } = require('./util');

const MANIFEST = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json';
const RESOURCES = 'https://resources.download.minecraft.net/';
const LIBRARIES = 'https://libraries.minecraft.net/';

function osName() {
  if (process.platform === 'win32') return 'windows';
  if (process.platform === 'darwin') return 'osx';
  return 'linux';
}

function osArch() {
  if (process.arch === 'ia32') return 'x86';
  if (process.arch === 'arm64') return 'arm64';
  return 'x86_64';
}

/** Regole dei JSON di Mojang: os + features. */
function ruleApplies(rule, features) {
  if (rule.os) {
    if (rule.os.name && rule.os.name !== osName()) return false;
    if (rule.os.arch && rule.os.arch !== 'x86' && rule.os.arch !== osArch()) return false;
  }
  if (rule.features) {
    for (const key of Object.keys(rule.features)) {
      if (!!features[key] !== !!rule.features[key]) return false;
    }
  }
  return true;
}

function allowed(rules, features) {
  if (!rules || !rules.length) return true;
  let allow = false;
  for (const rule of rules) {
    if (ruleApplies(rule, features)) allow = rule.action === 'allow';
  }
  return allow;
}

function downloadList(entry, features) {
  // arguments di 1.13+: stringhe oppure {rules, value}
  const out = [];
  for (const item of entry || []) {
    if (typeof item === 'string') out.push(item);
    else if (item && item.value && allowed(item.rules, features)) {
      if (Array.isArray(item.value)) out.push(...item.value);
      else out.push(item.value);
    }
  }
  return out;
}

async function versionManifest(dataDir) {
  mkdirp(path.join(dataDir, 'meta'));
  const cache = path.join(dataDir, 'meta', 'version_manifest_v2.json');
  if (exists(cache) && Date.now() - fs.statSync(cache).mtimeMs < 60 * 60 * 1000) {
    try { return JSON.parse(fs.readFileSync(cache, 'utf8')); } catch {}
  }
  const json = await getJson(MANIFEST);
  fs.writeFileSync(cache, JSON.stringify(json));
  return json;
}

/** JSON + jar del client vanilla, nella cartella versions/<mc>. */
async function vanillaVersion(dataDir, mc, onProgress) {
  const manifest = await versionManifest(dataDir);
  const entry = manifest.versions.find(v => v.id === mc);
  if (!entry) throw new Error('Minecraft ' + mc + ' is not in the Mojang manifest');
  const dir = path.join(dataDir, 'versions', mc);
  const jsonFile = path.join(dir, mc + '.json');
  mkdirp(dir);
  const json = await getJson(entry.url);
  fs.writeFileSync(jsonFile, JSON.stringify(json, null, 2));
  const client = json.downloads && json.downloads.client;
  const jarFile = path.join(dir, mc + '.jar');
  if (!exists(jarFile) || (client && fs.statSync(jarFile).size !== client.size)) {
    if (onProgress) onProgress('Client ' + mc + '.jar');
    await download(client.url, jarFile, { size: client.size, sha1: client.sha1 });
  }
  return { json, jsonFile, jarFile };
}

function libraryAllowed(lib, features) {
  return allowed(lib.rules, features);
}

function artifactOf(lib) {
  if (lib.downloads) {
    // con downloads presenti ma senza artifact (solo natives) non c'e' un jar da mettere in classpath
    return lib.downloads.artifact || null;
  }
  if (lib.name) {
    return { path: mavenPath(lib.name), url: (lib.url || LIBRARIES) + mavenPath(lib.name) };
  }
  return null;
}

function nativeClassifiers(lib, features) {
  const out = [];
  if (!libraryAllowed(lib, features)) return out;
  if (lib.natives) {
    const key = lib.natives[osName()];
    if (!key) return out;
    const classifier = key.replace('${arch}', process.arch === 'ia32' ? '32' : '64');
    const dl = lib.downloads && lib.downloads.classifiers ? lib.downloads.classifiers[classifier] : null;
    if (dl) out.push({ path: dl.path, url: dl.url, size: dl.size, sha1: dl.sha1 });
  } else if (lib.name && /:natives-/.test(lib.name)) {
    out.push({ path: mavenPath(lib.name), url: (lib.url || LIBRARIES) + mavenPath(lib.name), size: null, sha1: null });
  }
  return out;
}

/**
 * Scarica librerie e natives di una catena di version JSON (figlio -> vanilla).
 * Restituisce i jar di natives estratti e la classpath completa.
 */
async function installLibraries(dataDir, chain, nativesDir, onProgress) {
  const features = { is_demo_user: false, has_custom_resolution: false, has_quick_plays_support: false, is_quick_play_singleplayer: false };
  const libsDir = path.join(dataDir, 'libraries');
  const natives = [];
  const artifacts = new Map();
  const seen = new Set();
  for (const version of chain) {
    for (const lib of version.libraries || []) {
      if (!libraryAllowed(lib, features)) continue;
      const artifact = artifactOf(lib);
      const key = lib.name ? lib.name.split(':').slice(0, 2).join(':') + (lib.name.split(':')[3] || '') : (artifact && artifact.path);
      if (artifact && !seen.has(key)) {
        seen.add(key);
        artifacts.set(artifact.path, artifact);
      }
      for (const native of nativeClassifiers(lib, features)) natives.push(native);
    }
  }
  const list = [...artifacts.values()];
  let done = 0;
  await pool(list, 8, async artifact => {
    const dest = path.join(libsDir, artifact.path);
    await download(artifact.url, dest, { size: artifact.size, sha1: artifact.sha1 });
    done++;
    if (onProgress) onProgress('Libraries ' + done + '/' + list.length);
  });
  mkdirp(nativesDir);
  for (const native of natives) {
    const dest = path.join(libsDir, native.path);
    if (!exists(dest) || fs.statSync(dest).size === 0) {
      await download(native.url, dest, { size: native.size, sha1: native.sha1 });
    }
    try { extractNatives(dest, nativesDir); } catch {}
  }
  // I jar di natives restano anche in classpath: LWJGL moderno li carica da li'
  // (estrarli serve alle versioni vecchie con -Djava.library.path).
  const classpath = [];
  for (const artifact of artifacts.values()) {
    classpath.push(path.join(libsDir, artifact.path));
  }
  return classpath;
}

/** Indice asset + tutti gli oggetti (una volta per indice, condivisi tra versioni). */
async function installAssets(dataDir, version, onProgress) {
  const index = version.assetIndex || version.assets;
  if (!index || !index.url) return { assetsDir: path.join(dataDir, 'assets'), indexId: version.assets || 'legacy' };
  const assetsDir = path.join(dataDir, 'assets');
  const indexesDir = path.join(assetsDir, 'indexes');
  mkdirp(indexesDir);
  const indexFile = path.join(indexesDir, index.id + '.json');
  if (!exists(indexFile)) await download(index.url, indexFile, { size: index.size, sha1: index.sha1 });
  let json;
  try { json = JSON.parse(fs.readFileSync(indexFile, 'utf8')); } catch { return { assetsDir, indexId: index.id }; }
  const objects = Object.keys(json.objects || {}).map(key => json.objects[key]);
  const pending = [];
  const seen = new Set();
  for (const obj of objects) {
    const sub = obj.hash.slice(0, 2);
    if (seen.has(obj.hash)) continue;
    seen.add(obj.hash);
    const dest = path.join(assetsDir, 'objects', sub, obj.hash);
    if (exists(dest) && (!obj.size || fs.statSync(dest).size === obj.size)) continue;
    pending.push({ hash: obj.hash, size: obj.size, url: RESOURCES + sub + '/' + obj.hash, dest });
  }
  let done = 0;
  const total = pending.length;
  await pool(pending, 12, async item => {
    await download(item.url, item.dest, { size: item.size });
    done++;
    if (onProgress && done % 40 === 0) onProgress('Assets ' + done + '/' + total);
  });
  return { assetsDir, indexId: index.id };
}

/** Catena figlio -> vanilla, per fondere arguments e librerie. */
async function chainFor(dataDir, id, seen) {
  const guard = seen || new Set();
  if (guard.has(id)) return [];
  guard.add(id);
  const file = path.join(dataDir, 'versions', id, id + '.json');
  if (!exists(file)) throw new Error('Version ' + id + ' is not installed');
  const json = JSON.parse(fs.readFileSync(file, 'utf8'));
  const parent = json.inheritsFrom ? await chainFor(dataDir, json.inheritsFrom, guard) : [];
  return [json, ...parent];
}

function clientJar(dataDir, chain) {
  const root = chain[chain.length - 1];
  return path.join(dataDir, 'versions', root.id, root.id + '.jar');
}

function classpath(dataDir, chain, libraries) {
  const cp = libraries.slice();
  const jar = clientJar(dataDir, chain);
  if (exists(jar)) cp.push(jar);
  return cp.join(path.delimiter);
}

function substitute(value, vars) {
  return String(value).replace(/\$\{([^}]+)\}/g, (match, key) => (vars[key] !== undefined ? String(vars[key]) : match));
}

/** Argomenti JVM e di gioco completi, con placeholder sostituiti. */
function buildCommand(chain, options) {
  const features = { is_demo_user: false, has_custom_resolution: false, has_quick_plays_support: false, is_quick_play_singleplayer: false };
  const child = chain[0];
  const vanilla = chain[chain.length - 1];
  const jvm = [];
  const game = [];
  // parent prima, figlio dopo: le versioni caricate (Forge/Fabric) aggiungono i loro argomenti
  for (let i = chain.length - 1; i >= 0; i--) {
    const v = chain[i];
    if (v.arguments) {
      jvm.push(...downloadList(v.arguments.jvm, features));
      game.push(...downloadList(v.arguments.game, features));
    }
  }
  if (!game.length) {
    const legacy = child.minecraftArguments || vanilla.minecraftArguments;
    if (legacy) game.push(...legacy.split(' '));
  }
  if (!jvm.some(a => a === '-cp' || a === '-classpath' || a === '--class-path')) {
    jvm.push('-Djava.library.path=${natives_directory}', '-cp', '${classpath}');
  }
  const vars = options.vars;
  const jvmArgs = jvm.map(a => substitute(a, vars));
  const gameArgs = game.map(a => substitute(a, vars));
  return {
    jvm: jvmArgs,
    game: gameArgs,
    mainClass: child.mainClass || vanilla.mainClass
  };
}

module.exports = {
  osName, allowed, downloadList, vanillaVersion, installLibraries, installAssets,
  chainFor, classpath, buildCommand, clientJar
};
