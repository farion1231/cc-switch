// Shared pieces of the parity probes: in-page scripts, a W3C WebDriver client, PNG decoding,
// settle detection and run summaries. Platform probes add only launch, input and teardown.
import { createHash } from "node:crypto";
import { deflateSync, inflateSync } from "node:zlib";

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export const sha = (b) => createHash("sha256").update(b).digest("hex").slice(0, 16);

export function args(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) out[argv[i].slice(2)] = argv[++i];
    else out._.push(argv[i]);
  }
  return out;
}

export function webdriver(base) {
  async function wd(method, path, body, timeoutMs = 60000) {
    const res = await fetch(base + path, {
      method,
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(timeoutMs),
    });
    const json = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(`${method} ${path} -> ${res.status} ${JSON.stringify(json).slice(0, 300)}`);
    return json.value;
  }
  const exec = (sid, script, a = []) => wd("POST", `/session/${sid}/execute/sync`, { script, args: a });
  const execAsync = (sid, script, a = []) => wd("POST", `/session/${sid}/execute/async`, { script, args: a });
  return { wd, exec, execAsync };
}

// In-page serialiser: role, accessible name and states, never DOM structure or CSS.
export const TREE = `return (() => {
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
export const TARGET = `return (() => {
  const ok = (b) => !b.disabled && b.offsetParent !== null && (b.getAttribute('aria-label') || b.textContent.trim());
  const all = [...document.querySelectorAll('button, [role=button]')].filter(ok);
  const inDialog = all.filter((b) => b.closest('[role=dialog], [role=alertdialog], dialog'));
  const b = inDialog[0] || all[0];
  if (!b) return null;
  const r = b.getBoundingClientRect();
  return { name: (b.getAttribute('aria-label') || b.textContent).trim().replace(/\\s+/g, ' '),
           x: r.left + r.width / 2, y: r.top + r.height / 2, zoom: window.devicePixelRatio };
})();`;

// What has focus, so a step that must not move it can check.
export const FOCUS = `const a = document.activeElement;
  return a ? [a.tagName, a.id, a.getAttribute('aria-label') || '', (a.textContent || '').trim().slice(0, 40)].join('|') : '';`;

export const RECORD = `window.__parity = []; for (const t of ['pointerdown','mousedown','click'])
  window.addEventListener(t, (e) => window.__parity.push({ type: e.type, trusted: e.isTrusted, x: e.clientX, y: e.clientY,
    target: (e.target.closest('button,[role=button]')?.textContent || '').trim().replace(/\\s+/g,' ').slice(0,60) }), true);
  return true;`;

export function decodePng(buf) {
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

// Minimal PNG encoder for RGBA or RGB rows, used for artifacts and for screen captures that are not PNG.
export function encodePng({ w, h, bpp, px }) {
  const crcTable = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 255] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = bpp === 4 ? 6 : 2;
  const stride = w * bpp;
  const raw = Buffer.alloc(h * (stride + 1));
  for (let y = 0; y < h; y++) px.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

export function pixelDiff(a, b) {
  if (a.w !== b.w || a.h !== b.h || a.bpp !== b.bpp) return { sameSize: false, differing: a.w * a.h };
  let differing = 0;
  for (let i = 0; i < a.px.length; i += a.bpp) {
    for (let k = 0; k < a.bpp; k++) if (a.px[i + k] !== b.px[i + k]) { differing++; break; }
  }
  return { sameSize: true, differing };
}

export async function screenshot(wd, sid) {
  const png = Buffer.from(await wd("GET", `/session/${sid}/screenshot`), "base64");
  return { png, img: decodePng(png) };
}

// A short script timeout, so one evaluation whose result never comes back costs seconds and the
// settle loop retries, instead of one call eating the whole settle budget. Returns true or the error.
export function scriptTimeout(wd, sid, ms = 5000) {
  return wd("POST", `/session/${sid}/timeouts`, { script: ms }).then(() => true, (e) => String(e.message || e).slice(0, 200));
}

// Waits until the document has loaded. The driver can accept a script before the first page has
// committed, and on macOS such an evaluation sometimes never returns, so this phase uses a 1 s
// script timeout and retries. It ends at the first evaluation that sees the app's document complete;
// after that, settle treats any lost evaluation as a failure. Returns what it retried.
export async function waitReady(wd, exec, sid, timeoutMs = 30000) {
  const start = Date.now();
  const retried = [];
  await scriptTimeout(wd, sid, 1000);
  try {
    while (Date.now() - start < timeoutMs) {
      const state = await exec(sid, "return [document.readyState, location.href];").catch((e) => {
        retried.push(`+${Date.now() - start} ms ${String(e.message || e).slice(0, 160)}`);
        return null;
      });
      // The initial about:blank is also "complete"; only the app's own document counts.
      if (state && state[0] === "complete" && state[1] !== "about:blank") return { readyMs: Date.now() - start, href: state[1], retried };
      await sleep(200);
    }
  } finally {
    await scriptTimeout(wd, sid);
  }
  const err = new Error(`document never reached readyState complete in ${timeoutMs} ms`);
  err.detail = { retried: retried.slice(-8) };
  throw err;
}

// Polls the tree until 3 reads in a row agree. Failed script calls are kept in `errors`, which the
// caller may pass in, so a settle that recovered from a lost evaluation is still visible.
export async function settle(exec, sid, timeoutMs = 30000, errors = []) {
  const start = Date.now();
  let last = null, same = 0;
  const seen = [];
  let attempts = 0;
  while (Date.now() - start < timeoutMs) {
    attempts++;
    const tree = await exec(sid, TREE).catch((e) => { errors.push(`+${Date.now() - start} ms ${String(e.message || e).slice(0, 200)}`); return null; });
    if (tree && tree !== "[]" && tree === last) { if (++same >= 3) return tree; }
    else same = 0;
    if (tree && tree !== seen[seen.length - 1]) seen.push(tree);
    last = tree;
    await sleep(300);
  }
  const err = new Error(`tree did not settle in ${timeoutMs} ms`);
  err.detail = { attempts, distinctTrees: seen.length, lastTrees: seen.slice(-2).map((t) => JSON.parse(t)), errors: errors.slice(-8) };
  throw err;
}

export function summarise(runs) {
  const failures = [];
  runs.forEach((r) => r.failures.forEach((f) => failures.push(`run ${r.index}: ${f}`)));
  const ok = runs.filter((r) => r.treeHash);
  const distinct = (k) => [...new Set(ok.map((r) => r[k]))];
  if (distinct("treeHash").length > 1) failures.push(`tree differs across runs: ${distinct("treeHash").join(", ")}`);
  if (distinct("pixelsHash").length > 1) failures.push(`pixels differ across runs: ${distinct("pixelsHash").join(", ")}`);
  const stamp = (p) => p.replace(/\d{8}_\d{6}/g, "<ts>");
  const homes = [...new Set(ok.map((r) => JSON.stringify(r.redirectedHome.map(stamp))))];
  if (homes.length > 1) {
    const [a, b] = homes.slice(0, 2).map((h) => JSON.parse(h));
    const only = (x, y) => x.filter((p) => !y.includes(p));
    failures.push(`redirected HOME file list differs across runs (${homes.length} variants; first two differ by ${JSON.stringify([...only(a, b), ...only(b, a)])})`);
  }
  return { pass: failures.length === 0, failures, treeHashes: distinct("treeHash"), pixelsHashes: distinct("pixelsHash") };
}
