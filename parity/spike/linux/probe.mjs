#!/usr/bin/env node
// Parity suite, delivery unit 2 (first slice): Linux probe, run inside the parity container.
//
// Drives the app through tauri-driver and WebKitWebDriver under Xvfb, with no network and a
// fresh HOME per launch. Fails closed when:
//   1. the accessibility-style tree or the webview pixels differ across fresh launches;
//   2. two screenshots in one session differ;
//   3. a W3C pointer click does not reach the page as a trusted event, or does not change the tree;
//   4. the process HOME the app was not given (a sentinel) gains any file.
//
//   node probe.mjs run --app /app/cc-switch --runs <n> --out <file.json>
import { spawn, execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { args, scriptTimeout, encodePng, FOCUS, pixelDiff, RECORD, settle, sha, sleep, summarise, TARGET, TREE, webdriver } from "../common.mjs";

const PORT = 4444;
const { wd, exec } = webdriver(`http://127.0.0.1:${PORT}`);

function cleanEnv(home) {
  const env = {};
  for (const [k, v] of Object.entries(process.env)) {
    if (/^(https?|all|no|ftp)_proxy$/i.test(k)) continue;
    env[k] = v;
  }
  return {
    ...env,
    HOME: home,
    XDG_CONFIG_HOME: join(home, ".config"),
    XDG_DATA_HOME: join(home, ".local/share"),
    XDG_CACHE_HOME: join(home, ".cache"),
    TAURI_WEBVIEW_AUTOMATION: "true",
    LANG: "en_US.UTF-8",
    TZ: "UTC",
  };
}

// WebKitWebDriver screenshots never return for this app under Xvfb (it runs WebKitGTK with
// compositing off), so the lane captures what the X server shows: xwd of the root window,
// cropped to the window rect. Both builds are captured the same way.
function decodeXwd(buf) {
  const u32 = (i) => buf.readUInt32BE(i * 4);
  const [headerSize, version, format, , width, height, , byteOrder] = Array.from({ length: 8 }, (_, i) => u32(i));
  const bpp = u32(11), bytesPerLine = u32(12), masks = [u32(14), u32(15), u32(16)], ncolors = u32(19);
  if (version !== 7 || format !== 2 || (bpp !== 24 && bpp !== 32)) throw new Error(`unsupported xwd (version ${version}, format ${format}, bpp ${bpp})`);
  const step = bpp / 8, data = headerSize + ncolors * 12;
  const shift = masks.map((m) => Math.clz32(1) - Math.clz32(m & -m));
  const px = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const at = data + y * bytesPerLine + x * step;
      const v = byteOrder === 0 ? buf.readUIntLE(at, step) : buf.readUIntBE(at, step);
      const o = (y * width + x) * 4;
      for (let c = 0; c < 3; c++) px[o + c] = (v & masks[c]) >>> shift[c];
      px[o + 3] = 255;
    }
  }
  return { w: width, h: height, bpp: 4, px };
}

function crop(img, { x, y, width, height }) {
  const px = Buffer.alloc(width * height * 4);
  for (let row = 0; row < height; row++) img.px.copy(px, row * width * 4, ((y + row) * img.w + x) * 4, ((y + row) * img.w + x + width) * 4);
  return { w: width, h: height, bpp: 4, px };
}

// The X server holds whatever WebKit last painted, built up from partial repaints during
// animations, so its edges depend on frame timing. A screen capture is taken only after one
// full repaint, the way the macOS lane's snapshot renders the whole view at capture time.
// The repaint toggles the root's opacity: hiding the root instead would move focus to <body>.
async function fullRepaint(sid) {
  const before = [await exec(sid, TREE), await exec(sid, FOCUS)];
  await exec(sid, `window.__parityOpacity = document.documentElement.style.opacity; document.documentElement.style.opacity = "0.5";`);
  await sleep(250);
  await exec(sid, `document.documentElement.style.opacity = window.__parityOpacity; delete window.__parityOpacity;`);
  await sleep(400);
  const after = [await exec(sid, TREE), await exec(sid, FOCUS)];
  if (before[0] !== after[0]) throw new Error("the full repaint changed the tree");
  if (before[1] !== after[1]) throw new Error(`the full repaint moved focus from "${before[1]}" to "${after[1]}"`);
}

async function xshot(sid) {
  const rect = await wd("GET", `/session/${sid}/window/rect`, undefined, 10000);
  const screen = decodeXwd(execFileSync("xwd", ["-root", "-silent"], { maxBuffer: 64 << 20 }));
  const img = crop(screen, rect);
  return { rect, img, png: encodePng(img) };
}

async function oneRun(app, index, outDir) {
  const home = mkdtempSync(join(tmpdir(), `parity-home-${index}-`));
  const sentinel = process.env.HOME;
  const t0 = Date.now();
  const driver = spawn("tauri-driver", ["--port", String(PORT), "--native-driver", "/usr/bin/WebKitWebDriver"], {
    env: cleanEnv(home),
    stdio: ["ignore", "pipe", "pipe"],
  });
  let log = "";
  driver.stdout.on("data", (d) => (log += d));
  driver.stderr.on("data", (d) => (log += d));
  const result = { index, failures: [] };
  const step = (m) => console.error(`[run ${index}] +${Date.now() - t0} ms ${m}`);
  let sid = null;
  try {
    for (let i = 0; ; i++) {
      try { await wd("GET", "/status"); break; } catch { if (i > 100) throw new Error("tauri-driver never answered"); await sleep(200); }
    }
    step("tauri-driver up; creating session");
    const session = await wd("POST", "/session", { capabilities: { alwaysMatch: { "tauri:options": { application: app } } } });
    sid = session.sessionId;
    result.scriptTimeoutSet = await scriptTimeout(wd, sid);
    result.serverUpMs = Date.now() - t0;
    step("session created; waiting for the tree to settle");

    result.settleErrors = [];
    const tree = await settle(exec, sid, 30000, result.settleErrors);
    if (result.settleErrors.length) result.failures.push(`settled only after ${result.settleErrors.length} failed script call(s), first: ${result.settleErrors[0]}`);
    step("settled");
    result.settledMs = Date.now() - t0;
    result.treeHash = sha(tree);
    result.tree = JSON.parse(tree);

    step("capturing screen 1");
    await fullRepaint(sid);
    const s1 = await xshot(sid);
    await sleep(300);
    const s2 = await xshot(sid);
    step("captures taken");
    result.windowRect = s1.rect;
    result.size = [s1.img.w, s1.img.h];
    result.pixelsHash = sha(s1.img.px);
    result.inSessionPixelDiff = pixelDiff(s1.img, s2.img);
    writeFileSync(join(outDir, `shot-${index}.png`), s1.png);
    if (result.inSessionPixelDiff.differing !== 0) result.failures.push("two screenshots in one session differ");

    const target = await exec(sid, TARGET);
    result.target = target;
    if (!target) {
      result.failures.push("no clickable button found");
    } else {
      await exec(sid, RECORD);
      await wd("POST", `/session/${sid}/actions`, {
        actions: [{
          type: "pointer", id: "mouse", parameters: { pointerType: "mouse" },
          actions: [
            { type: "pointerMove", duration: 0, origin: "viewport", x: Math.round(target.x), y: Math.round(target.y) },
            { type: "pointerDown", button: 0 },
            { type: "pointerUp", button: 0 },
          ],
        }],
      });
      await wd("DELETE", `/session/${sid}/actions`).catch(() => {});
      await sleep(500);
      const events = await exec(sid, "return window.__parity;");
      result.click = { events };
      const trusted = events.filter((e) => e.trusted);
      if (trusted.length === 0) {
        result.failures.push(events.length ? "click arrived but not as a trusted event" : "no event reached the page");
      } else if (!trusted.some((e) => e.target === target.name)) {
        result.failures.push(`trusted click landed on "${trusted[0].target}", expected "${target.name}"`);
      }
      const after = await settle(exec, sid).catch(() => tree);
      result.click.treeChanged = after !== tree;
      if (!result.click.treeChanged) result.failures.push("click did not change the tree");
    }
  } catch (e) {
    step(`error: ${e.message || e}`);
    result.failures.push(String(e.message || e));
    if (e.detail) result.settleDetail = e.detail;
  } finally {
    step("tearing down");
    if (sid) await wd("DELETE", `/session/${sid}`, undefined, 15000).catch((e) => step(`delete session: ${e.message}`));
    step("session deleted");
    driver.kill("SIGTERM");
    await Promise.race([new Promise((r) => driver.once("exit", r)), sleep(5000)]);
    try { execFileSync("pkill", ["-x", "cc-switch"]); } catch {}
    result.totalMs = Date.now() - t0;
    const sentinelFiles = readdirSync(sentinel);
    if (sentinelFiles.length) result.failures.push(`sentinel HOME gained ${sentinelFiles.join(", ")}`);
    result.redirectedHome = execFileSync("find", [home, "-maxdepth", "4"]).toString().split("\n").filter(Boolean).map((p) => p.slice(home.length) || "/").sort();
    if (result.failures.length) result.log = log.slice(-4000);
    rmSync(home, { recursive: true, force: true });
  }
  return result;
}

async function main() {
  const a = args(process.argv.slice(2));
  if (a._[0] !== "run" || !a.app) {
    console.error("usage: probe.mjs run --app <binary> [--runs n] [--out file]");
    process.exit(2);
  }
  const out = a.out || "/out/results.json";
  const outDir = dirname(out);
  mkdirSync(outDir, { recursive: true });
  const runs = [];
  for (let i = 0; i < Number(a.runs || 3); i++) {
    const r = await oneRun(a.app, i, outDir);
    console.log(`run ${i}: tree ${r.treeHash} pixels ${r.pixelsHash} start ${r.serverUpMs} ms settle ${r.settledMs} ms total ${r.totalMs} ms ${r.failures.length ? "FAIL " + r.failures.join("; ") : "ok"}`);
    runs.push(r);
  }
  const summary = summarise(runs);
  writeFileSync(out, JSON.stringify({ summary, runs }, null, 2));
  console.log(JSON.stringify(summary, null, 2));
  process.exit(summary.pass ? 0 : 1);
}

main().catch((e) => { console.error(e); process.exit(1); });
