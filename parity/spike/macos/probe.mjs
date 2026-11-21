#!/usr/bin/env node
// Parity suite, delivery unit 1: macOS probe.
//
// Questions this answers on a hosted macOS runner, failing closed on each:
//   1. Does the embedded WebDriver plugin serve a parity build in release profile?
//   2. Is the accessibility-style tree identical across fresh launches?
//   3. Are webview screenshots identical (decoded RGBA) within a session and across launches?
//   4. Does a real OS click (CGEvent) reach the page as a trusted event and change the tree?
//   5. Does the app leave the runner's real HOME untouched when HOME is redirected?
//
//   node probe.mjs run --app <CC Switch.app> --helper <helper> --runs <n> --out <file.json>
//   node probe.mjs compare <results.json>...
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { inflateSync } from "node:zlib";

const PORT = 4445;
const BASE = `http://127.0.0.1:${PORT}`;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const sha = (b) => createHash("sha256").update(b).digest("hex").slice(0, 16);

function args(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) out[argv[i].slice(2)] = argv[++i];
    else out._.push(argv[i]);
  }
  return out;
}

async function wd(method, path, body) {
  const res = await fetch(BASE + path, {
    method,
    headers: { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await res.json().catch(() => ({}));
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status} ${JSON.stringify(json).slice(0, 300)}`);
  return json.value;
}

const exec = (sid, script, a = []) => wd("POST", `/session/${sid}/execute/sync`, { script, args: a });

// In-page serialiser: role, accessible name and states, never DOM structure or CSS.
const TREE = `return (() => {
  const TAG = {BUTTON:'button',A:'link',H1:'heading',H2:'heading',H3:'heading',H4:'heading',H5:'heading',H6:'heading',
    TEXTAREA:'textbox',SELECT:'combobox',DIALOG:'dialog',NAV:'navigation',MAIN:'main',ASIDE:'complementary',
    UL:'list',OL:'list',LI:'listitem',IMG:'img',TABLE:'table',TR:'row',TD:'cell',TH:'columnheader'};
  const NAMED_BY_TEXT = new Set(['button','link','heading','tab','menuitem','option','cell','columnheader','checkbox','radio','switch','listitem']);
  const LEAF = new Set(['button','link','heading','tab','menuitem','option']);
  const hidden = (el) => {
    if (el.getAttribute('aria-hidden') === 'true') return true;
    const s = getComputedStyle(el);
    return s.display === 'none' || s.visibility === 'hidden';
  };
  const role = (el) => {
    const r = el.getAttribute('role');
    if (r) return r;
    if (el.tagName === 'INPUT') return ({checkbox:'checkbox',radio:'radio',range:'slider',button:'button',submit:'button'})[el.type] || 'textbox';
    return TAG[el.tagName] || null;
  };
  const text = (s) => (s || '').trim().replace(/\\s+/g, ' ');
  const name = (el, r) => {
    const label = el.getAttribute('aria-label');
    if (label) return text(label);
    const by = el.getAttribute('aria-labelledby');
    if (by) return text(by.split(/\\s+/).map((id) => document.getElementById(id)?.textContent || '').join(' '));
    if (el.getAttribute('alt')) return text(el.getAttribute('alt'));
    if (NAMED_BY_TEXT.has(r)) return text(el.textContent);
    if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA') return text(el.placeholder || el.getAttribute('title'));
    return '';
  };
  const states = (el) => {
    const o = {};
    for (const a of ['aria-checked','aria-expanded','aria-selected','aria-pressed','aria-disabled','aria-current']) {
      const v = el.getAttribute(a); if (v !== null) o[a.slice(5)] = v;
    }
    if (el.disabled) o.disabled = 'true';
    if ((el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') && el.type !== 'password') {
      if (el.type === 'checkbox' || el.type === 'radio') o.checked = String(el.checked); else o.value = String(el.value);
    }
    return o;
  };
  const walk = (el) => {
    const out = [];
    for (const c of el.childNodes) {
      if (c.nodeType === 3) { const t = text(c.textContent); if (t) out.push({ role: 'text', name: t }); continue; }
      if (c.nodeType !== 1 || c.tagName === 'SCRIPT' || c.tagName === 'STYLE' || hidden(c)) continue;
      const r = role(c);
      const kids = walk(c);
      if (!r) { out.push(...kids); continue; }
      const n = { role: r };
      const nm = name(c, r); if (nm) n.name = nm;
      const st = states(c); if (Object.keys(st).length) n.states = st;
      if (kids.length && !LEAF.has(r)) n.children = kids;
      out.push(n);
    }
    return out;
  };
  return JSON.stringify(walk(document.body));
})();`;

// Picks the first named, enabled button, preferring one inside a dialog, and returns its centre in CSS px.
const TARGET = `return (() => {
  const ok = (b) => !b.disabled && b.offsetParent !== null && (b.getAttribute('aria-label') || b.textContent.trim());
  const all = [...document.querySelectorAll('button, [role=button]')].filter(ok);
  const inDialog = all.filter((b) => b.closest('[role=dialog], [role=alertdialog], dialog'));
  const b = inDialog[0] || all[0];
  if (!b) return null;
  const r = b.getBoundingClientRect();
  return { name: (b.getAttribute('aria-label') || b.textContent).trim().replace(/\\s+/g, ' '),
           x: r.left + r.width / 2, y: r.top + r.height / 2, zoom: window.devicePixelRatio };
})();`;

const RECORD = `window.__parity = []; for (const t of ['pointerdown','mousedown','click'])
  window.addEventListener(t, (e) => window.__parity.push({ type: e.type, trusted: e.isTrusted, x: e.clientX, y: e.clientY,
    target: (e.target.closest('button,[role=button]')?.textContent || '').trim().replace(/\\s+/g,' ').slice(0,60) }), true);
  return true;`;

function decodePng(buf) {
  if (buf.readUInt32BE(0) !== 0x89504e47) throw new Error("not a PNG");
  let pos = 8, w, h, depth, ctype;
  const idat = [];
  while (pos < buf.length) {
    const len = buf.readUInt32BE(pos);
    const type = buf.toString("ascii", pos + 4, pos + 8);
    const data = buf.subarray(pos + 8, pos + 8 + len);
    if (type === "IHDR") {
      w = data.readUInt32BE(0); h = data.readUInt32BE(4); depth = data[8]; ctype = data[9];
      if (data[12] !== 0) throw new Error("interlaced PNG");
    } else if (type === "IDAT") idat.push(data);
    else if (type === "IEND") break;
    pos += 12 + len;
  }
  const bpp = { 0: 1, 2: 3, 4: 2, 6: 4 }[ctype];
  if (depth !== 8 || !bpp) throw new Error(`unsupported PNG depth ${depth} type ${ctype}`);
  const raw = inflateSync(Buffer.concat(idat));
  const stride = w * bpp;
  const px = Buffer.alloc(h * stride);
  let prev = Buffer.alloc(stride);
  for (let y = 0; y < h; y++) {
    const f = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const cur = px.subarray(y * stride, (y + 1) * stride);
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? cur[x - bpp] : 0, b = prev[x], c = x >= bpp ? prev[x - bpp] : 0;
      let v = line[x];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) {
        const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c);
        v += pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      } else if (f !== 0) throw new Error(`bad PNG filter ${f}`);
      cur[x] = v & 255;
    }
    prev = cur;
  }
  return { w, h, bpp, px };
}

function pixelDiff(a, b) {
  if (a.w !== b.w || a.h !== b.h || a.bpp !== b.bpp) return { sameSize: false, differing: a.w * a.h };
  let differing = 0;
  for (let i = 0; i < a.px.length; i += a.bpp) {
    for (let k = 0; k < a.bpp; k++) if (a.px[i + k] !== b.px[i + k]) { differing++; break; }
  }
  return { sameSize: true, differing };
}

async function screenshot(sid) {
  const png = Buffer.from(await wd("GET", `/session/${sid}/screenshot`), "base64");
  return { png, img: decodePng(png) };
}

async function settle(sid, timeoutMs = 30000) {
  const start = Date.now();
  let last = null, same = 0;
  const seen = [], errors = [];
  while (Date.now() - start < timeoutMs) {
    const tree = await exec(sid, TREE).catch((e) => { errors.push(String(e.message || e).slice(0, 200)); return null; });
    if (tree && tree !== "[]" && tree === last) { if (++same >= 3) return tree; }
    else same = 0;
    if (tree && tree !== seen[seen.length - 1]) seen.push(tree);
    last = tree;
    await sleep(300);
  }
  const err = new Error(`tree did not settle in ${timeoutMs} ms`);
  err.detail = { distinctTrees: seen.length, lastTrees: seen.slice(-2).map((t) => JSON.parse(t)), errors: errors.slice(-5) };
  throw err;
}

function cleanEnv(home) {
  const env = {};
  for (const [k, v] of Object.entries(process.env)) {
    if (/^(https?|all|no)_proxy$/i.test(k)) continue;
    env[k] = v;
  }
  return { ...env, HOME: home, CFFIXED_USER_HOME: home, TAURI_WEBDRIVER_PORT: String(PORT), LANG: "en_US.UTF-8", TZ: "UTC" };
}

const realHomeMarks = (home) => [".cc-switch", ".claude", ".codex", ".gemini"].filter((d) => existsSync(join(home, d)));

async function oneRun(app, helper, index) {
  const exe = join(app, "Contents/MacOS", execFileSync("plutil", ["-extract", "CFBundleExecutable", "raw", join(app, "Contents/Info.plist")]).toString().trim());
  const home = mkdtempSync(join(tmpdir(), `parity-home-${index}-`));
  const realHome = process.env.HOME;
  const before = realHomeMarks(realHome);
  try { execFileSync(helper, ["move", "1", "1"]); } catch {}
  const t0 = Date.now();
  const child = spawn(exe, [], { env: cleanEnv(home), stdio: ["ignore", "pipe", "pipe"] });
  let log = "";
  child.stdout.on("data", (d) => (log += d));
  child.stderr.on("data", (d) => (log += d));
  const result = { index, failures: [] };
  try {
    for (let i = 0; ; i++) {
      try { await wd("GET", "/status"); break; } catch { if (i > 200) throw new Error("WebDriver server never answered"); await sleep(300); }
    }
    result.serverUpMs = Date.now() - t0;
    const session = await wd("POST", "/session", { capabilities: {} });
    const sid = session.sessionId;
    const handles = await wd("GET", `/session/${sid}/window/handles`).catch(() => []);
    result.windowHandles = handles;
    if (handles.includes("main")) await wd("POST", `/session/${sid}/window`, { handle: "main" });

    const tree = await settle(sid);
    result.settledMs = Date.now() - t0;
    result.treeHash = sha(tree);
    result.tree = JSON.parse(tree);

    const s1 = await screenshot(sid);
    const s2 = await screenshot(sid);
    result.size = [s1.img.w, s1.img.h];
    result.pixelsHash = sha(s1.img.px);
    result.inSessionPixelDiff = pixelDiff(s1.img, s2.img);
    writeFileSync(join(process.env.PARITY_OUT_DIR || ".", `shot-${index}.png`), s1.png);
    if (result.inSessionPixelDiff.differing !== 0) result.failures.push("two screenshots in one session differ");

    const target = await exec(sid, TARGET);
    result.target = target;
    if (!target) {
      result.failures.push("no clickable button found");
    } else {
      await exec(sid, RECORD);
      const helperOut = (...a) => execFileSync(helper, a.map(String)).toString().trim();
      const [wx, wy] = helperOut("bounds", child.pid).split(/\s+/).map(Number);
      const point = { x: wx + target.x, y: wy + target.y };
      result.click = { window: [wx, wy], point, frontmostBefore: Number(helperOut("frontmost")), appPid: child.pid };
      result.click.activate = helperOut("activate", child.pid);
      await sleep(600);
      result.click.frontmostAfterActivate = Number(helperOut("frontmost"));
      result.click.attempts = [];
      for (let attempt = 1; attempt <= 2; attempt++) {
        helperOut("click", point.x, point.y);
        await sleep(800);
        const events = await exec(sid, "return window.__parity;");
        result.click.attempts.push({ attempt, events });
        if (events.length) break;
      }
      const events = result.click.attempts.at(-1).events;
      const trusted = events.filter((e) => e.trusted);
      if (trusted.length === 0) {
        result.failures.push(events.length ? "click arrived but not as a trusted event" : "no event reached the page from two OS clicks");
      } else if (!trusted.some((e) => e.target === target.name)) {
        result.failures.push(`trusted click landed on "${trusted[0].target}", expected "${target.name}"`);
      }
      const after = await settle(sid).catch(() => tree);
      result.click.treeChanged = after !== tree;
      if (!result.click.treeChanged) result.failures.push("OS click did not change the tree");
    }
    await wd("DELETE", `/session/${sid}`).catch(() => {});
  } catch (e) {
    result.failures.push(String(e.message || e));
    if (e.detail) result.settleDetail = e.detail;
  } finally {
    try { execFileSync(helper, ["move", "1", "1"]); } catch {}
    child.kill("SIGTERM");
    const exited = await Promise.race([new Promise((r) => child.once("exit", (code, sig) => r({ code, sig }))), sleep(10000).then(() => null)]);
    if (!exited) { child.kill("SIGKILL"); result.failures.push("app did not exit within 10 s of SIGTERM"); }
    result.exit = exited;
    result.totalMs = Date.now() - t0;
    const touched = realHomeMarks(realHome).filter((d) => !before.includes(d));
    result.realHomeTouched = touched;
    if (touched.length) result.failures.push(`real HOME gained ${touched.join(", ")}`);
    result.redirectedHome = execFileSync("find", [home, "-maxdepth", "3"]).toString().split("\n").filter(Boolean).map((p) => p.slice(home.length) || "/").sort();
    if (result.failures.length) result.log = log.slice(-4000);
    rmSync(home, { recursive: true, force: true });
  }
  return result;
}

function summarise(runs) {
  const failures = [];
  runs.forEach((r) => r.failures.forEach((f) => failures.push(`run ${r.index}: ${f}`)));
  const ok = runs.filter((r) => r.treeHash);
  const distinct = (k) => [...new Set(ok.map((r) => r[k]))];
  if (distinct("treeHash").length > 1) failures.push(`tree differs across runs: ${distinct("treeHash").join(", ")}`);
  if (distinct("pixelsHash").length > 1) failures.push(`pixels differ across runs: ${distinct("pixelsHash").join(", ")}`);
  const stamp = (p) => p.replace(/\d{8}_\d{6}/g, "<ts>");
  const homes = [...new Set(ok.map((r) => JSON.stringify(r.redirectedHome.map(stamp))))];
  if (homes.length > 1) failures.push("redirected HOME file list differs across runs");
  return { pass: failures.length === 0, failures, treeHashes: distinct("treeHash"), pixelsHashes: distinct("pixelsHash") };
}

async function main() {
  const a = args(process.argv.slice(2));
  if (a._[0] === "compare") {
    const all = a._.slice(1).map((f) => JSON.parse(readFileSync(f, "utf8")));
    const runs = all.flatMap((r, i) => r.runs.map((x) => ({ ...x, index: `${i}.${x.index}` })));
    const s = summarise(runs);
    console.log(JSON.stringify({ files: a._.slice(1), axTrusted: all.map((r) => r.axTrusted), ...s }, null, 2));
    process.exit(s.pass ? 0 : 1);
  }
  if (a._[0] !== "run" || !a.app || !a.helper) {
    console.error("usage: probe.mjs run --app <.app> --helper <helper> [--runs n] [--out file] | compare <files...>");
    process.exit(2);
  }
  if (process.env.PARITY_OUT_DIR) mkdirSync(process.env.PARITY_OUT_DIR, { recursive: true });
  const axTrusted = execFileSync(a.helper, ["trusted"]).toString().trim();
  const runs = [];
  for (let i = 0; i < Number(a.runs || 3); i++) {
    const r = await oneRun(a.app, a.helper, i);
    console.log(`run ${i}: tree ${r.treeHash} pixels ${r.pixelsHash} start ${r.serverUpMs} ms settle ${r.settledMs} ms total ${r.totalMs} ms ${r.failures.length ? "FAIL " + r.failures.join("; ") : "ok"}`);
    runs.push(r);
  }
  const summary = summarise(runs);
  const out = { axTrusted, summary, runs };
  writeFileSync(a.out || "probe-results.json", JSON.stringify(out, null, 2));
  console.log(JSON.stringify({ axTrusted, ...summary }, null, 2));
  process.exit(summary.pass ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(1); });
