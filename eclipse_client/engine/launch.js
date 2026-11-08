'use strict';
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { spawn } = require('child_process');
const { mkdirp } = require('./util');
const mc = require('./minecraft');

/** UUID offline identico a quello di Minecraft (nameUUIDFromBytes("OfflinePlayer:"+name)). */
function offlineUuid(name) {
  const hash = crypto.createHash('md5').update('OfflinePlayer:' + name, 'utf8').digest();
  hash[6] = (hash[6] & 0x0f) | 0x30;
  hash[8] = (hash[8] & 0x3f) | 0x80;
  const hex = hash.toString('hex');
  return hex.slice(0, 8) + '-' + hex.slice(8, 12) + '-' + hex.slice(12, 16) + '-' + hex.slice(16, 20) + '-' + hex.slice(20);
}

function tail(file, lines) {
  try {
    const text = fs.readFileSync(file, 'utf8');
    return text.trim().split('\n').slice(-(lines || 8)).join('\n');
  } catch { return ''; }
}

/**
 * Prepara (se serve) e avvia il gioco. Il processo e' staccato: il launcher puo' chiudersi.
 * Restituisce { ok, pid } oppure { ok:false, error } con la coda del log.
 */
async function launch(options, onProgress) {
  const { dataDir, instanceDir, versionId, mcVersion, username, ramMb, javaPath } = options;
  if (!javaPath) return { ok: false, error: 'Java not found' };
  mkdirp(instanceDir);
  const nativesDir = path.join(dataDir, 'natives', versionId);
  const chain = await mc.chainFor(dataDir, versionId);
  const vanilla = chain[chain.length - 1];

  if (onProgress) onProgress('Checking libraries');
  const libraries = await mc.installLibraries(dataDir, chain, nativesDir, onProgress);
  if (onProgress) onProgress('Checking assets');
  const assets = await mc.installAssets(dataDir, vanilla, onProgress);

  const jar = mc.clientJar(dataDir, chain);
  const classpath = libraries.concat([jar]).join(path.delimiter);
  const vars = {
    auth_player_name: username,
    auth_uuid: offlineUuid(username),
    auth_access_token: '0',
    auth_session: '0',
    user_type: 'legacy',
    user_properties: '{}',
    clientid: '',
    auth_xuid: '',
    version_name: mcVersion,
    version_type: 'release',
    game_directory: instanceDir,
    assets_root: assets.assetsDir,
    assets_index_name: assets.indexId,
    game_assets: path.join(assets.assetsDir, 'virtual', 'legacy'),
    natives_directory: nativesDir,
    library_directory: path.join(dataDir, 'libraries'),
    classpath,
    classpath_separator: path.delimiter,
    launcher_name: 'EclipseClient',
    launcher_version: '0.2'
  };
  const command = mc.buildCommand(chain, { vars });
  if (!command.mainClass) return { ok: false, error: 'Version ' + versionId + ' has no main class' };

  const jvm = [
    '-Xms512M',
    '-Xmx' + Math.max(1, ramMb) + 'M',
    '-Dminecraft.launcher.brand=EclipseClient',
    '-Dminecraft.launcher.version=0.2',
    ...command.jvm
  ];
  const args = [...jvm, command.mainClass, ...command.game];

  const logsDir = path.join(dataDir, 'logs');
  mkdirp(logsDir);
  const logFile = path.join(logsDir, mcVersion + '-latest.log');
  const fd = fs.openSync(logFile, 'a');
  fs.writeSync(fd, '\n=== Eclipse Client launch ' + new Date().toISOString() + ' ===\n');

  const child = spawn(javaPath, args, {
    cwd: instanceDir,
    detached: true,
    stdio: ['ignore', fd, fd]
  });
  child.on('error', () => {});
  child.unref();
  try { fs.closeSync(fd); } catch {}

  await new Promise(resolve => setTimeout(resolve, 1800));
  if (child.exitCode !== null && child.exitCode !== 0) {
    return { ok: false, error: 'Minecraft exited with code ' + child.exitCode + ': ' + tail(logFile, 10) };
  }
  return { ok: true, pid: child.pid, log: logFile };
}

module.exports = { launch, offlineUuid };
