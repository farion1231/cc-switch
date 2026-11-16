// 在 bin\claude.exe 不能改名时升级 npm 全局安装的 Claude Code。
// 用法：node npm_claude_inplace.cjs <bin\claude.exe> <平台包目录> <npm pack 输出目录>
// 输出目录里要有同一版本的主包和平台包 tarball。所有文件原地写入，
// 新的 claude.exe 跑不起来时把写过的文件恢复原样。
'use strict';

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { spawnSync } = require('child_process');

const MAIN_PACKAGE = '@anthropic-ai/claude-code';
const MAX_UNPACKED = 1024 * 1024 * 1024;

function fail(message) {
  throw new Error(message);
}

function sleep(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

function cString(buf) {
  const end = buf.indexOf(0);
  return buf.subarray(0, end < 0 ? buf.length : end).toString('utf8');
}

// pax 扩展头里的 path 记录，格式为 "<长度> path=<值>\n"。
function paxPath(data) {
  let offset = 0;
  while (offset < data.length) {
    const space = data.indexOf(0x20, offset);
    if (space < 0) break;
    const length = parseInt(data.subarray(offset, space).toString('utf8'), 10);
    if (!(length > 0)) break;
    const record = data.subarray(space + 1, offset + length - 1).toString('utf8');
    const eq = record.indexOf('=');
    if (eq > 0 && record.slice(0, eq) === 'path') return record.slice(eq + 1);
    offset += length;
  }
  return '';
}

function readTarball(file) {
  const raw = zlib.gunzipSync(fs.readFileSync(file), { maxOutputLength: MAX_UNPACKED });
  const members = new Map();
  let offset = 0;
  let nextName = '';
  while (offset + 512 <= raw.length) {
    const header = raw.subarray(offset, offset + 512);
    if (header[0] === 0) break;
    const size = parseInt(cString(header.subarray(124, 136)).trim() || '0', 8) || 0;
    const type = header[156] === 0 ? '0' : String.fromCharCode(header[156]);
    let name = cString(header.subarray(0, 100));
    const prefix = cString(header.subarray(345, 500));
    if (prefix) name = prefix.replace(/\/$/, '') + '/' + name;
    offset += 512;
    const data = raw.subarray(offset, offset + size);
    offset += Math.ceil(size / 512) * 512;
    if (type === 'L') {
      nextName = cString(data);
      continue;
    }
    if (type === 'x') {
      nextName = paxPath(data) || nextName;
      continue;
    }
    if (type === 'g') continue;
    if (nextName) {
      name = nextName;
      nextName = '';
    }
    if (type === '0' || type === '7') members.set(name.replace(/\\/g, '/'), Buffer.from(data));
  }
  return members;
}

// 按 package.json 的包名收集输出目录里的 tarball，文件路径去掉顶层目录（通常是 package/）。
function loadPackages(dir) {
  const packages = new Map();
  for (const entry of fs.readdirSync(dir)) {
    if (!entry.endsWith('.tgz')) continue;
    const files = new Map();
    for (const [name, data] of readTarball(path.join(dir, entry))) {
      const slash = name.indexOf('/');
      if (slash > 0) files.set(name.slice(slash + 1), data);
    }
    const meta = files.get('package.json');
    if (!meta) continue;
    const pkg = JSON.parse(meta.toString('utf8'));
    packages.set(pkg.name, { version: pkg.version, files });
  }
  return packages;
}

function overwrite(file, data) {
  const fd = fs.openSync(file, 'r+');
  try {
    let written = 0;
    while (written < data.length) {
      written += fs.writeSync(fd, data, written, data.length - written, written);
    }
    fs.ftruncateSync(fd, data.length);
  } finally {
    fs.closeSync(fd);
  }
}

const journal = [];

// 先记下原内容再写。已存在的文件用 r+ 覆盖，新文件直接创建。
function writeInPlace(file, data) {
  const previous = fs.existsSync(file) ? fs.readFileSync(file) : null;
  journal.push({ file, previous });
  if (previous === null) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, data);
  } else {
    overwrite(file, data);
  }
}

// 校验用的 claude.exe 刚退出时镜像可能还没放开，恢复时重试几秒。
function restore({ file, previous }) {
  for (let attempt = 0; ; attempt++) {
    try {
      if (previous === null) fs.rmSync(file, { force: true });
      else overwrite(file, previous);
      return;
    } catch (err) {
      if (attempt >= 20) throw err;
      sleep(250);
    }
  }
}

function rollback() {
  for (const entry of journal.reverse()) {
    try {
      restore(entry);
    } catch (err) {
      console.error(`restore ${entry.file} failed: ${err.message}`);
    }
  }
}

function sameFile(a, b) {
  try {
    const left = fs.statSync(a, { bigint: true });
    const right = fs.statSync(b, { bigint: true });
    return left.dev === right.dev && left.ino === right.ino;
  } catch {
    return false;
  }
}

function targetIn(base, rel) {
  const root = path.resolve(base);
  const target = path.resolve(root, rel);
  if (!target.startsWith(root + path.sep)) fail(`tarball entry escapes package: ${rel}`);
  return target;
}

function verify(exe, version) {
  const result = spawnSync(exe, ['--version'], {
    encoding: 'utf8',
    timeout: 60000,
    windowsHide: true,
  });
  const output = `${result.stdout || ''}${result.stderr || ''}`.trim();
  if (result.error || result.status !== 0 || !output.includes(version)) {
    fail(`new claude.exe does not run: ${result.error ? result.error.message : output}`);
  }
}

function main() {
  const [dest, platformDir, packDir] = process.argv.slice(2);
  if (!dest || !platformDir || !packDir) {
    fail('usage: npm_claude_inplace.cjs <claude.exe> <platform package dir> <pack dir>');
  }
  const root = path.resolve(dest, '..', '..');
  const platformName = '@anthropic-ai/' + path.basename(platformDir);
  const packages = loadPackages(packDir);
  const mainPackage = packages.get(MAIN_PACKAGE);
  const platform = packages.get(platformName);
  if (!mainPackage) fail(`no ${MAIN_PACKAGE} tarball in ${packDir}`);
  if (!platform) fail(`no ${platformName} tarball in ${packDir}`);
  if (mainPackage.version !== platform.version) {
    fail(`${MAIN_PACKAGE}@${mainPackage.version} and ${platformName}@${platform.version} differ`);
  }
  const exe = platform.files.get('claude.exe');
  if (!exe || exe.length < 2 || exe[0] !== 0x4d || exe[1] !== 0x5a) {
    fail(`${platformName} has no Windows claude.exe`);
  }

  const platformExe = path.join(platformDir, 'claude.exe');
  try {
    // postinstall 通常把 bin\claude.exe 硬链到平台包里的 claude.exe，写一份两边都更新。
    const linked = sameFile(dest, platformExe);
    writeInPlace(dest, exe);
    if (!linked) writeInPlace(platformExe, exe);
    for (const [rel, data] of platform.files) {
      if (rel !== 'claude.exe') writeInPlace(targetIn(platformDir, rel), data);
    }
    // 主包里的 bin/claude.exe 是安装前的占位文件。
    for (const [rel, data] of mainPackage.files) {
      if (rel !== 'bin/claude.exe') writeInPlace(targetIn(root, rel), data);
    }
    verify(dest, mainPackage.version);
  } catch (err) {
    rollback();
    throw err;
  }
  // 隐藏锁文件还记着旧版本。删掉后 npm 按 node_modules 的实际内容重建。
  fs.rmSync(path.join(root, '..', '..', '.package-lock.json'), { force: true });
  console.log(mainPackage.version);
}

try {
  main();
} catch (err) {
  console.error(err && err.message ? err.message : String(err));
  process.exit(1);
}
