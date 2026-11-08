'use strict';
const fs = require('fs');
const path = require('path');
const { spawnSync } = require('child_process');
const AdmZip = require('adm-zip');
const { getJson, download, mkdirp, exists, mavenPath } = require('./util');

const FORGE_MAVEN = 'https://maven.minecraftforge.net/net/minecraftforge/forge/';

/** Profilo Fabric ufficiale (meta.fabricmc.net), creato nella cartella versions del launcher. */
async function installFabric(dataDir, mc, onProgress) {
  const list = await getJson('https://meta.fabricmc.net/v2/versions/loader/' + mc);
  if (!Array.isArray(list) || !list.length) throw new Error('Fabric has no loader for ' + mc);
  const entry = list.find(item => item.loader && item.loader.stable) || list[0];
  const loader = entry.loader.version;
  if (onProgress) onProgress('Fabric ' + loader + ' for ' + mc);
  const profile = await getJson('https://meta.fabricmc.net/v2/versions/loader/' + mc + '/' + loader + '/profile/json');
  const id = profile.id;
  const dir = path.join(dataDir, 'versions', id);
  mkdirp(dir);
  fs.writeFileSync(path.join(dir, id + '.json'), JSON.stringify(profile, null, 2));
  return { id, loader, json: profile };
}

/** Installa Forge col suo installer ufficiale (client side, headless). */
async function installForge(dataDir, mc, spec, javaPath, onProgress) {
  if (!javaPath) throw new Error('No Java found for Forge ' + mc);
  const versionPath = spec; // es. "1.20.1-47.4.28" oppure "1.8.9-11.15.1.2318-1.8.9"
  const url = FORGE_MAVEN + versionPath + '/forge-' + versionPath + '-installer.jar';
  const installer = path.join(dataDir, 'meta', 'forge-' + versionPath + '-installer.jar');
  if (onProgress) onProgress('Forge installer ' + versionPath);
  await download(url, installer, {});

  // l'installer pretende un launcher_profiles.json nella cartella di destinazione
  const profilesFile = path.join(dataDir, 'launcher_profiles.json');
  if (!exists(profilesFile)) {
    fs.writeFileSync(profilesFile, JSON.stringify({ profiles: {}, settings: {}, version: 3 }, null, 2));
  }

  const run = spawnSync(javaPath, ['-jar', installer, '--installClient', dataDir], {
    cwd: dataDir, encoding: 'utf8', timeout: 15 * 60 * 1000, maxBuffer: 32 * 1024 * 1024
  });
  if (run.status !== 0) {
    const detail = ((run.stderr || '') + '\n' + (run.stdout || '')).trim().split('\n').slice(-6).join(' | ');
    throw new Error('Forge installer failed (' + run.status + '): ' + detail);
  }

  const versionsDir = path.join(dataDir, 'versions');
  const found = [];
  for (const name of fs.readdirSync(versionsDir)) {
    const file = path.join(versionsDir, name, name + '.json');
    if (!exists(file)) continue;
    try {
      const json = JSON.parse(fs.readFileSync(file, 'utf8'));
      if (json.inheritsFrom === mc && /forge/i.test(name)) found.push({ id: name, json });
    } catch {}
  }
  if (!found.length) throw new Error('Forge installed but no version profile for ' + mc);
  found.sort((a, b) => b.id.length - a.id.length);
  return found[0];
}

/**
 * Forge vecchio (1.8.9): il suo installer non ha --installClient, ma contiene gia'
 * install_profile.json con il version JSON e il jar universal. Li sistemiamo noi,
 * senza far partire la GUI.
 */
async function installForgeLegacy(dataDir, mc, spec, onProgress) {
  const url = FORGE_MAVEN + spec + '/forge-' + spec + '-installer.jar';
  const installer = path.join(dataDir, 'meta', 'forge-' + spec + '-installer.jar');
  if (onProgress) onProgress('Forge installer ' + spec);
  await download(url, installer, {});

  const zip = new AdmZip(installer);
  const profileEntry = zip.getEntry('install_profile.json');
  if (!profileEntry) throw new Error('Forge installer has no install_profile.json');
  const profile = JSON.parse(profileEntry.getData().toString('utf8'));
  const info = profile.versionInfo;
  if (!info || !info.id) throw new Error('Forge installer has no version info');

  const fileName = (profile.install && profile.install.filePath) || 'forge-' + spec + '-universal.jar';
  const universal = zip.getEntry(fileName) || zip.getEntries().find(e => /-universal\.jar$/.test(e.entryName));
  if (!universal) throw new Error('Forge installer has no universal jar');

  const forgeLib = (info.libraries || []).find(lib => String(lib.name).startsWith('net.minecraftforge:forge:'));
  if (!forgeLib) throw new Error('Forge version has no forge library');
  if (String(forgeLib.name).split(':').length === 3) forgeLib.name = forgeLib.name + ':universal';
  if (!forgeLib.url) forgeLib.url = FORGE_MAVEN;
  const target = path.join(dataDir, 'libraries', mavenPath(forgeLib.name));
  mkdirp(path.dirname(target));
  fs.writeFileSync(target, universal.getData());

  const dir = path.join(dataDir, 'versions', info.id);
  mkdirp(dir);
  fs.writeFileSync(path.join(dir, info.id + '.json'), JSON.stringify(info, null, 2));
  return { id: info.id, json: info };
}

module.exports = { installFabric, installForge, installForgeLegacy };
