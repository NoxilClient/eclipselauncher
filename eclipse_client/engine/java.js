'use strict';
const fs = require('fs');
const path = require('path');
const os = require('os');
const { spawnSync } = require('child_process');

const HOME = os.homedir();

function isFile(file) {
  try { return fs.statSync(file).isFile(); } catch { return false; }
}

function listDir(dir) {
  try { return fs.readdirSync(dir); } catch { return []; }
}

function javasIn(dir, suffix) {
  return listDir(dir)
    .map(name => path.join(dir, name, suffix))
    .filter(isFile);
}

/** Tutti i Java installati che conosciamo, dal piu' probabile. */
function candidates(major) {
  const out = [];
  const win = process.platform === 'win32';
  if (process.platform === 'linux') {
    const roots = ['/usr/lib/jvm', '/usr/java', path.join(HOME, '.jdks'), '/opt/java'];
    for (const root of roots) out.push(...javasIn(root, 'bin/java'));
  } else if (process.platform === 'darwin') {
    const root = '/Library/Java/JavaVirtualMachines';
    for (const name of listDir(root)) out.push(path.join(root, name, 'Contents', 'Home', 'bin', 'java'));
    for (const name of listDir(path.join(HOME, 'Library/Java/JavaVirtualMachines'))) {
      out.push(path.join(HOME, 'Library/Java/JavaVirtualMachines', name, 'Contents', 'Home', 'bin', 'java'));
    }
  } else if (win) {
    const roots = [path.join(process.env.ProgramFiles || 'C:\\Program Files', 'Java'),
      path.join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'Java')];
    for (const root of roots) {
      for (const name of listDir(root)) out.push(path.join(root, name, 'bin', 'java.exe'));
    }
  }
  const pattern = major === 8 ? /(^|[^0-9])(1\.8|java-8|jdk8|jre8|jdk1\.8)/i : new RegExp('(^|[^0-9])' + major + '([^0-9]|$)');
  const matches = out.filter(p => pattern.test(p));
  return matches.sort((a, b) => {
    // prima i runtime (jre) rispetto alle JDK complete? no: prima i percorsi piu' semplici
    return a.length - b.length;
  });
}

/** Java maggiore di un eseguibile, letto da `java -version` (null se non parte). */
function majorOf(javaPath) {
  const run = spawnSync(javaPath, ['-version'], { encoding: 'utf8' });
  if (run.status !== 0) return null;
  const text = (run.stderr || '') + (run.stdout || '');
  const match = /version "(\d+)(?:\.(\d+))?/.exec(text);
  if (!match) return null;
  if (match[1] === '1') return Number(match[2]);
  return Number(match[1]);
}

/** Sceglie il Java giusto: override dell'utente, altrimenti uno con la major richiesta. */
function findJava(major, override) {
  if (override) {
    return { path: override, major: majorOf(override) || major, source: 'settings' };
  }
  const list = candidates(major);
  for (const candidate of list) {
    if (majorOf(candidate) === major) return { path: candidate, major, source: 'system' };
  }
  const onPath = process.platform === 'win32' ? 'java.exe' : 'java';
  const fallbackMajor = majorOf(onPath);
  if (fallbackMajor === major) return { path: onPath, major, source: 'path' };
  return { path: null, major, source: 'missing', found: list.slice(0, 3), pathMajor: fallbackMajor };
}

module.exports = { findJava, majorOf, candidates };
