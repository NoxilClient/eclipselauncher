'use strict';
const fs = require('fs');
const fsp = fs.promises;
const path = require('path');
const crypto = require('crypto');
const { Readable } = require('stream');
const { pipeline } = require('stream/promises');
const AdmZip = require('adm-zip');

const UA = 'EclipseClient/0.2 (standalone launcher)';

async function getJson(url) {
  const res = await fetch(url, { headers: { 'User-Agent': UA } });
  if (!res.ok) throw new Error('HTTP ' + res.status + ' ' + url);
  return res.json();
}

function exists(file) {
  try { return fs.statSync(file).isFile(); } catch { return false; }
}

function mkdirp(dir) {
  fs.mkdirSync(dir, { recursive: true });
}

/**
 * Scarica un file sul disco. Salta se c'e' gia' con la dimensione attesa,
 * scrive su .part e rinomina solo a download finito (niente file a meta').
 */
async function download(url, dest, options) {
  const opts = options || {};
  // i file finiscono sul disco solo a download completo, quindi uno gia' presente e' valido
  if (exists(dest)) {
    const size = fs.statSync(dest).size;
    if (size > 0 && (!opts.size || size === opts.size)) return dest;
  }
  mkdirp(path.dirname(dest));
  let lastError = null;
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const res = await fetch(url, { headers: { 'User-Agent': UA } });
      if (!res.ok) throw new Error('HTTP ' + res.status + ' ' + url);
      const tmp = dest + '.part';
      const hash = opts.sha1 ? crypto.createHash('sha1') : null;
      const sink = fs.createWriteStream(tmp);
      const source = Readable.fromWeb(res.body);
      await pipeline(source, async function* (stream) {
        for await (const chunk of stream) {
          if (hash) hash.update(chunk);
          if (opts.onBytes) opts.onBytes(chunk.length);
          yield chunk;
        }
      }, sink);
      if (hash && hash.digest('hex') !== opts.sha1) {
        try { fs.unlinkSync(tmp); } catch {}
        throw new Error('checksum mismatch: ' + url);
      }
      fs.renameSync(tmp, dest);
      return dest;
    } catch (err) {
      lastError = err;
      await new Promise(r => setTimeout(r, 400 * (attempt + 1)));
    }
  }
  throw lastError || new Error('download failed: ' + url);
}

/** Esegue `fn` su tutti gli elementi con al massimo `limit` download in parallelo. */
async function pool(items, limit, fn) {
  const list = items.slice();
  const workers = new Array(Math.max(1, Math.min(limit, list.length))).fill(0).map(async () => {
    while (list.length) {
      const item = list.shift();
      await fn(item);
    }
  });
  await Promise.all(workers);
}

/** Estrae un jar di natives nella cartella data, saltando i file gia' presenti. */
function extractNatives(jarFile, targetDir) {
  mkdirp(targetDir);
  const zip = new AdmZip(jarFile);
  for (const entry of zip.getEntries()) {
    if (entry.isDirectory) continue;
    const name = entry.entryName;
    if (name.startsWith('META-INF/')) continue;
    const out = path.join(targetDir, name);
    if (exists(out)) continue;
    mkdirp(path.dirname(out));
    fs.writeFileSync(out, entry.getData());
    if (process.platform !== 'win32' && /\.(so|dylib|jnilib)$/.test(name)) {
      try { fs.chmodSync(out, 0o755); } catch {}
    }
  }
}

/** Nome maven (group:artifact:version[:classifier]) -> percorso nella cache librerie. */
function mavenPath(name) {
  const parts = String(name).split(':');
  const group = parts[0];
  const artifact = parts[1];
  const version = parts[2];
  const classifier = parts[3];
  const file = artifact + '-' + version + (classifier ? '-' + classifier : '') + '.jar';
  return path.join(group.replace(/\./g, '/'), artifact, version, file);
}

module.exports = { UA, getJson, download, pool, extractNatives, mavenPath, exists, mkdirp };
